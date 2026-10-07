//! Displays the rendered photo with fit / 100% / free zoom and panning.
//!
//! Holds two renderings of the same photo: a screen-sized preview that is used while
//! the whole image is visible, and an optional full-resolution image that is used once
//! the zoom level exceeds the preview's resolution. Zoom is expressed in physical pixels
//! per full-resolution image pixel, so 1.0 is a true 100% view on HiDPI screens too.

use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Painter, PointerButton, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use iris_core::crop::Affine;
use iris_core::{Crop, Mask};

use crate::crop_tool::{CropTool, FrameMapping};
use crate::mask_editor::{Brush, Mapping, MaskEditor, Tool};
use crate::session::Framing;
use crate::texture::{Magnify, TiledTexture};
use crate::theme;

const MAX_ZOOM: f32 = 16.0;
/// Points around the image in fit mode.
const FIT_MARGIN: f32 = 12.0;
const ZOOM_STEPS: [f32; 12] = [0.0625, 0.125, 0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0, 2.0, 3.0, 4.0, 8.0, 16.0];
/// Points of scrolling per mouse-wheel notch.
const POINTS_PER_NOTCH: f32 = 50.0;
const SPLIT_GRAB: f32 = 8.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompareMode {
    #[default]
    Off,
    /// The unedited photo.
    Before,
    /// Unedited on the left, edited on the right.
    Split,
}

/// What the user did in the view.
pub enum ViewEvent {
    /// Eyedropper click at a normalised image position.
    PointPicked(f64, f64),
    PickCancelled,
    /// A mask gesture (one brush stroke, one handle drag) begins, changes the mask, and ends.
    MaskGestureStarted,
    MaskEdited {
        mask: Mask,
        label: &'static str,
    },
    MaskGestureFinished,
    /// A crop drag begins, changes the crop, and ends.
    CropGestureStarted,
    CropEdited(Crop),
    CropGestureFinished,
    /// Double-click inside the crop: done cropping.
    CropDone,
}

pub struct ImageView {
    preview: Option<TiledTexture>,
    full: Option<TiledTexture>,
    /// Full-resolution size of the rendering (after the crop); defines the image
    /// coordinate system.
    image_size: [usize; 2],
    /// Where the photo lies in the rendering.
    framing: Option<Framing>,
    message: String,
    loading: bool,

    fit: bool,
    zoom: f32,
    /// Image point shown at the centre of the view.
    center: Pos2,
    rect: Rect,
    pixels_per_point: f32,

    before_preview: Option<TiledTexture>,
    before_full: Option<TiledTexture>,
    compare: CompareMode,
    /// Divider position as a fraction of the view width.
    split: f32,
    dragging_split: bool,

    pick_mode: bool,
    panning: bool,
    last_pan_pos: Pos2,

    mask_editor: MaskEditor,
    mask_overlay: Option<TiledTexture>,
    /// For the brush outline.
    cursor_pos: Option<Pos2>,
    /// While cropping (the rendering then shows the whole straightened frame).
    crop_tool: Option<CropTool>,
}

impl Default for ImageView {
    fn default() -> Self {
        Self {
            preview: None,
            full: None,
            image_size: [0, 0],
            framing: None,
            message: "Open a RAW photo to start  (Ctrl+O)".into(),
            loading: false,
            fit: true,
            zoom: 1.0,
            center: Pos2::ZERO,
            rect: Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0)),
            pixels_per_point: 1.0,
            before_preview: None,
            before_full: None,
            compare: CompareMode::Off,
            split: 0.5,
            dragging_split: false,
            pick_mode: false,
            panning: false,
            last_pan_pos: Pos2::ZERO,
            mask_editor: MaskEditor::default(),
            mask_overlay: None,
            cursor_pos: None,
            crop_tool: None,
        }
    }
}

impl ImageView {
    /// Shows a loading indicator; the next set_preview() starts a new photo (resets to fit).
    pub fn begin_loading(&mut self) {
        self.loading = true;
        self.message.clear();
        self.before_preview = None;
        self.before_full = None;
        self.mask_overlay = None;
    }

    pub fn set_load_failed(&mut self, message: String) {
        self.loading = false;
        self.preview = None;
        self.full = None;
        self.image_size = [0, 0];
        self.message = message;
    }

    pub fn set_preview(&mut self, preview: TiledTexture, framing: Framing) {
        let new_photo = self.loading || self.image_size[0] == 0;
        let resized = framing.result_size != self.image_size;
        self.preview = Some(preview);
        self.framing = Some(framing);
        if new_photo || resized {
            // A new photo, or a new crop: show all of it.
            self.loading = false;
            self.image_size = framing.result_size;
            self.full = None;
            self.fit_to_window();
        }
    }

    /// Cropping: shows the crop rectangle over the whole frame (None = done cropping).
    pub fn set_crop_tool(&mut self, crop: Option<(Crop, [usize; 2])>) {
        match (crop, &mut self.crop_tool) {
            (Some((crop, photo)), Some(tool)) => tool.set_crop(crop, photo),
            (Some((crop, photo)), None) => self.crop_tool = Some(CropTool::new(crop, photo)),
            (None, _) => self.crop_tool = None,
        }
    }

    pub fn set_full_image(&mut self, full: Option<TiledTexture>) {
        if let Some(full) = &full
            && [full.width, full.height] != self.image_size
        {
            // Keep the same relative position if the decoder's final size differs slightly.
            let sx = full.width as f32 / self.image_size[0].max(1) as f32;
            let sy = full.height as f32 / self.image_size[1].max(1) as f32;
            self.center = Pos2::new(self.center.x * sx, self.center.y * sy);
            self.image_size = [full.width, full.height];
            if self.fit {
                self.fit_to_window();
            }
        }
        self.full = full;
    }

    pub fn set_compare_mode(&mut self, mode: CompareMode) {
        self.compare = mode;
    }

    pub fn compare_mode(&self) -> CompareMode {
        self.compare
    }

    pub fn set_before_preview(&mut self, image: TiledTexture) {
        self.before_preview = Some(image);
    }

    pub fn set_before_full(&mut self, image: TiledTexture) {
        self.before_full = Some(image);
    }

    pub fn has_image(&self) -> bool {
        self.preview.is_some()
    }

    #[cfg(test)]
    pub fn is_fit(&self) -> bool {
        self.fit
    }

    #[cfg(test)]
    pub fn has_mask_overlay(&self) -> bool {
        self.mask_overlay.is_some()
    }

    #[cfg(test)]
    pub fn has_full_image(&self) -> bool {
        self.full.is_some()
    }

    /// Where the view was last drawn.
    #[cfg(test)]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    #[cfg(test)]
    pub fn test_framing(&self) -> Option<Framing> {
        self.framing
    }

    /// Where the rendering is on screen.
    #[cfg(test)]
    pub fn test_image_rect(&self) -> Rect {
        self.image_rect()
    }

    /// True when the current zoom shows more detail than the preview rendering has.
    pub fn needs_full_resolution(&self) -> bool {
        match &self.preview {
            Some(preview) if self.image_size[0] > 0 && !self.fit => {
                self.zoom > preview.width as f32 / self.image_size[0] as f32 * 1.001
            }
            _ => false,
        }
    }

    /// Eyedropper mode: the next click reports an image position instead of panning.
    pub fn set_pick_mode(&mut self, enabled: bool) {
        self.pick_mode = enabled;
    }

    pub fn is_picking(&self) -> bool {
        self.pick_mode
    }

    /// Mask editing: while a mask is set, dragging paints or reshapes it instead of panning
    /// (pan with the middle button or Space + drag).
    pub fn set_mask(&mut self, mask: Option<&Mask>) {
        let was_editing = self.mask_editor.is_active();
        self.mask_editor.set_mask(mask);
        if mask.is_none() {
            self.mask_overlay = None;
            if was_editing {
                self.panning = false;
            }
        }
    }

    pub fn set_mask_tool(&mut self, tool: Tool) {
        self.mask_editor.set_tool(tool);
    }

    pub fn set_brush(&mut self, brush: Brush) {
        self.mask_editor.set_brush(brush);
    }

    /// Tinted coverage of the mask being edited; `None` hides the overlay.
    pub fn set_mask_overlay(&mut self, overlay: Option<TiledTexture>) {
        self.mask_overlay = overlay;
    }

    // --- Zoom --------------------------------------------------------------------

    fn fit_zoom(&self) -> f32 {
        if self.image_size[0] == 0 {
            return 1.0;
        }
        let w = (self.rect.width() - 2.0 * FIT_MARGIN).max(1.0) * self.pixels_per_point;
        let h = (self.rect.height() - 2.0 * FIT_MARGIN).max(1.0) * self.pixels_per_point;
        (w / self.image_size[0] as f32).min(h / self.image_size[1] as f32)
    }

    /// Screen points per image pixel.
    fn logical_scale(&self) -> f32 {
        self.zoom / self.pixels_per_point
    }

    fn widget_to_image(&self, pos: Pos2) -> Pos2 {
        self.center + (pos - self.rect.center()) / self.logical_scale()
    }

    pub fn fit_to_window(&mut self) {
        self.fit = true;
        self.zoom = self.fit_zoom();
        self.center = Pos2::new(self.image_size[0] as f32 / 2.0, self.image_size[1] as f32 / 2.0);
    }

    pub fn zoom_to_actual_pixels(&mut self) {
        self.set_zoom(1.0, self.rect.center());
    }

    pub fn zoom_in(&mut self) {
        if let Some(&step) = ZOOM_STEPS.iter().find(|&&s| s > self.zoom * 1.01) {
            self.set_zoom(step, self.rect.center());
        }
    }

    pub fn zoom_out(&mut self) {
        match ZOOM_STEPS.iter().rev().find(|&&s| s < self.zoom / 1.01) {
            Some(&step) => self.set_zoom(step, self.rect.center()),
            None => self.fit_to_window(),
        }
    }

    fn set_zoom(&mut self, zoom: f32, anchor: Pos2) {
        if self.image_size[0] == 0 {
            return;
        }
        // Zooming out never goes below "fit": the whole photo is always reachable.
        if zoom <= self.fit_zoom() * 1.0001 {
            self.fit_to_window();
            return;
        }
        let anchor_image = self.widget_to_image(anchor);
        self.zoom = zoom.min(MAX_ZOOM);
        self.fit = false;
        self.center = anchor_image - (anchor - self.rect.center()) / self.logical_scale();
        self.clamp_center();
    }

    fn clamp_center(&mut self) {
        let s = self.logical_scale();
        let clamp_axis = |center: f32, image_length: f32, view_length: f32| {
            if image_length <= view_length {
                image_length / 2.0
            } else {
                center.clamp(view_length / 2.0, image_length - view_length / 2.0)
            }
        };
        self.center.x = clamp_axis(self.center.x, self.image_size[0] as f32, self.rect.width() / s);
        self.center.y = clamp_axis(self.center.y, self.image_size[1] as f32, self.rect.height() / s);
    }

    /// Human-readable zoom, e.g. "Fit  ·  27%".
    pub fn zoom_label(&self) -> String {
        if !self.has_image() {
            return String::new();
        }
        let percent =
            if self.zoom < 0.1 { format!("{:.1}%", self.zoom * 100.0) } else { format!("{:.0}%", self.zoom * 100.0) };
        if self.fit { format!("Fit  ·  {percent}") } else { percent }
    }

    /// Screen position of the rendering's top-left corner.
    fn origin(&self) -> Pos2 {
        self.rect.center() - self.center.to_vec2() * self.logical_scale()
    }

    /// Photo pixels -> screen, for the mask editor.
    fn mapping(&self) -> Mapping {
        let s = self.logical_scale();
        let origin = self.origin();
        let (photo_to_result, photo_size) =
            self.framing.map_or((Affine::IDENTITY, self.image_size), |f| (f.photo_to_result, f.photo_size));
        Mapping {
            photo_to_screen: Affine::translate(f64::from(origin.x), f64::from(origin.y))
                * Affine::scale(f64::from(s), f64::from(s))
                * photo_to_result,
            scale: s,
            image_size: photo_size,
            clip: self.image_rect(),
        }
    }

    /// Frame pixels -> screen, for the crop tool (while cropping, the rendering is the
    /// whole frame).
    fn frame_mapping(&self) -> FrameMapping {
        FrameMapping { origin: self.origin(), scale: self.logical_scale(), frame_size: self.image_size }
    }

    fn image_rect(&self) -> Rect {
        Rect::from_min_size(
            self.origin(),
            Vec2::new(self.image_size[0] as f32, self.image_size[1] as f32) * self.logical_scale(),
        )
    }

    fn split_x(&self) -> f32 {
        self.rect.left() + self.rect.width() * self.split
    }

    // --- Interaction -----------------------------------------------------------------

    /// Draws the view into the remaining space and handles input. `space_held` comes from
    /// the app, which knows whether a text field has the keyboard.
    pub fn ui(&mut self, ui: &mut Ui, space_held: bool) -> Vec<ViewEvent> {
        let mut events = Vec::new();
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        if rect != self.rect || ppp != self.pixels_per_point {
            self.rect = rect;
            self.pixels_per_point = ppp;
            if self.fit {
                self.fit_to_window();
            } else {
                self.clamp_center();
            }
        }
        let has_image = self.image_size[0] > 0 && self.preview.is_some();
        let hovered = response.hovered() || response.is_pointer_button_down_on();
        let input = ui.input(|i| i.clone());
        let pointer = &input.pointer;
        let pos = pointer.interact_pos();
        let mapping = self.mapping();

        // Escape cancels the eyedropper (the app handles Escape otherwise).
        if self.pick_mode && input.key_pressed(egui::Key::Escape) {
            events.push(ViewEvent::PickCancelled);
        }

        // Wheel and pinch zoom.
        if hovered && has_image {
            // Ctrl + wheel already arrives as zoom_delta.
            let notches: f32 = input
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, modifiers, .. } if !modifiers.command => Some(match unit {
                        egui::MouseWheelUnit::Point => delta.y / POINTS_PER_NOTCH,
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y,
                    }),
                    _ => None,
                })
                .sum();
            let factor = 1.25f32.powf(notches) * input.zoom_delta();
            if factor != 1.0
                && let Some(anchor) = pointer.hover_pos()
            {
                self.set_zoom(self.zoom * factor, anchor);
            }
        }

        // Presses.
        let primary_pressed = pointer.button_pressed(PointerButton::Primary) && response.hovered();
        let middle_pressed = pointer.button_pressed(PointerButton::Middle) && response.hovered();
        if (primary_pressed || middle_pressed) && has_image {
            let at = pos.unwrap_or_default();
            if self.pick_mode && primary_pressed {
                // Rendering pixels -> photo pixels.
                let p = self.widget_to_image(at);
                let (to_photo, [pw, ph]) = self
                    .framing
                    .map_or((Affine::IDENTITY, self.image_size), |f| (f.photo_to_result.inverted(), f.photo_size));
                let (x, y) = to_photo.map(f64::from(p.x), f64::from(p.y));
                if x >= 0.0 && y >= 0.0 && x < pw as f64 && y < ph as f64 {
                    events.push(ViewEvent::PointPicked(x / pw as f64, y / ph as f64));
                }
            } else if self.compare == CompareMode::Split
                && primary_pressed
                && (at.x - self.split_x()).abs() <= SPLIT_GRAB
            {
                self.dragging_split = true;
            } else {
                // Middle button, or Space + drag, pans; while editing a mask a plain drag edits it.
                let pan_gesture = middle_pressed || (primary_pressed && space_held);
                let frame = self.frame_mapping();
                if !pan_gesture && let Some(tool) = &mut self.crop_tool {
                    if primary_pressed && tool.press(at, &frame) {
                        events.push(ViewEvent::CropGestureStarted);
                    } else if !self.fit {
                        self.panning = true;
                        self.last_pan_pos = at;
                    }
                } else if !pan_gesture && self.mask_editor.is_active() {
                    if primary_pressed && self.mask_editor.press(at, &mapping, input.modifiers.alt) {
                        events.push(ViewEvent::MaskGestureStarted);
                        if self.mask_editor.tool() == Tool::Brush {
                            // A click paints a dab.
                            events.push(self.mask_edited());
                        }
                    }
                } else if !self.fit {
                    self.panning = true;
                    self.last_pan_pos = at;
                }
            }
        }

        // Moves.
        if self.mask_editor.is_active() {
            self.cursor_pos = pointer.hover_pos().filter(|p| rect.contains(*p));
        }
        let frame = self.frame_mapping();
        if let Some(at) = pos {
            if let Some(tool) = self.crop_tool.as_mut().filter(|t| t.is_dragging()) {
                if let Some(crop) = tool.drag_to(at, &frame) {
                    events.push(ViewEvent::CropEdited(crop));
                }
            } else if self.mask_editor.is_dragging() {
                if self.mask_editor.drag_to(at, &mapping) {
                    events.push(self.mask_edited());
                }
            } else if self.dragging_split {
                self.split = ((at.x - rect.left()) / rect.width().max(1.0)).clamp(0.02, 0.98);
            } else if self.panning {
                let delta = at - self.last_pan_pos;
                self.last_pan_pos = at;
                self.center -= delta / self.logical_scale();
                self.clamp_center();
            }
        }

        // Releases.
        if pointer.button_released(PointerButton::Primary) || pointer.button_released(PointerButton::Middle) {
            if self.dragging_split {
                self.dragging_split = false;
            } else if let Some(tool) = self.crop_tool.as_mut().filter(|t| t.is_dragging()) {
                tool.release();
                events.push(ViewEvent::CropGestureFinished);
            } else if self.mask_editor.is_dragging() && pointer.button_released(PointerButton::Primary) {
                self.mask_editor.release();
                events.push(ViewEvent::MaskGestureFinished);
            } else if self.panning && !pointer.any_down() {
                self.panning = false;
            }
        }

        // Double-click toggles fit / 100% at the cursor; inside the crop it finishes cropping.
        let double_clicked = pointer.button_double_clicked(PointerButton::Primary) && response.hovered() && has_image;
        let in_crop = |p: Pos2| self.crop_tool.as_ref().and_then(|t| t.hit(p, &frame)).is_some();
        if double_clicked && pos.is_some_and(in_crop) {
            events.push(ViewEvent::CropDone);
        } else if double_clicked && !self.pick_mode && !self.mask_editor.is_active() && self.crop_tool.is_none() {
            if self.fit {
                self.set_zoom(1.0, pos.unwrap_or(rect.center()));
            } else {
                self.fit_to_window();
            }
        }

        if response.hovered() || self.panning {
            ui.ctx().set_cursor_icon(self.cursor(pointer.hover_pos(), space_held));
        }
        // Smooth when mildly enlarged; crisp pixels when inspecting detail.
        let magnify = if self.zoom >= 2.0 { Magnify::Pixels } else { Magnify::Smooth };
        for full in [&mut self.full, &mut self.before_full].into_iter().flatten() {
            full.set_magnify(magnify);
        }
        self.paint(&ui.painter_at(rect), space_held);
        events
    }

    fn mask_edited(&self) -> ViewEvent {
        ViewEvent::MaskEdited {
            mask: self.mask_editor.mask().cloned().expect("editing a mask"),
            label: self.mask_editor.gesture_label(),
        }
    }

    fn cursor(&self, hover: Option<Pos2>, space_held: bool) -> CursorIcon {
        let has_image = self.image_size[0] > 0;
        if self.pick_mode {
            CursorIcon::Crosshair
        } else if self.panning {
            CursorIcon::Grabbing
        } else if self.dragging_split
            || (self.compare == CompareMode::Split
                && !self.mask_editor.is_active()
                && hover.is_some_and(|p| (p.x - self.split_x()).abs() <= SPLIT_GRAB))
        {
            CursorIcon::ResizeHorizontal
        } else if let Some(handle) = self
            .crop_tool
            .as_ref()
            .filter(|_| !space_held)
            .zip(hover)
            .and_then(|(t, p)| t.hit(p, &self.frame_mapping()))
        {
            handle.cursor()
        } else if self.mask_editor.is_active() && has_image && !space_held {
            if hover.is_some_and(|p| self.mask_editor.is_over_handle(p, &self.mapping())) {
                CursorIcon::Move
            } else {
                CursorIcon::Crosshair
            }
        } else if has_image && !self.fit {
            CursorIcon::Grab
        } else {
            CursorIcon::Default
        }
    }

    // --- Drawing ---------------------------------------------------------------------

    fn draw_photo(&self, painter: &Painter, preview: Option<&TiledTexture>, full: Option<&TiledTexture>, clip: Rect) {
        let Some(preview) = preview else { return };
        let image_rect = self.image_rect();
        // Use the full-resolution rendering once the preview would be magnified.
        let preview_zoom = preview.width as f32 / self.image_size[0] as f32;
        let image = match full {
            Some(full) if self.zoom > preview_zoom * 1.001 => full,
            _ => preview,
        };
        image.paint(painter, image_rect, clip);
    }

    fn paint(&self, painter: &Painter, space_held: bool) {
        let rect = self.rect;
        painter.rect_filled(rect, CornerRadius::ZERO, theme::VIEW_BACKGROUND);

        if self.preview.is_some() && self.image_size[0] > 0 {
            // While the "before" rendering is not ready yet (or still has the previous crop),
            // show the edited one.
            let fits = |t: &TiledTexture| {
                let (a, b) = (t.width as f32 / t.height as f32, self.image_size[0] as f32 / self.image_size[1] as f32);
                (a / b - 1.0).abs() < 0.01
            };
            let (before_preview, before_full) = match &self.before_preview {
                Some(before) if fits(before) => (Some(before), self.before_full.as_ref().filter(|f| fits(f))),
                _ => (self.preview.as_ref(), self.full.as_ref()),
            };
            match self.compare {
                CompareMode::Off => {
                    self.draw_photo(painter, self.preview.as_ref(), self.full.as_ref(), rect);
                    if self.mask_editor.is_active()
                        && let Some(overlay) = &self.mask_overlay
                    {
                        overlay.paint(painter, self.image_rect(), rect);
                    }
                }
                CompareMode::Before => {
                    self.draw_photo(painter, before_preview, before_full, rect);
                    draw_label(painter, "Before", rect.left_top() + Vec2::splat(12.0), Align2::LEFT_TOP);
                }
                CompareMode::Split => {
                    let x = self.split_x();
                    let (left, right) = rect.split_left_right_at_x(x);
                    self.draw_photo(painter, before_preview, before_full, left);
                    self.draw_photo(painter, self.preview.as_ref(), self.full.as_ref(), right);
                    painter.vline(x, rect.y_range(), Stroke::new(1.0, Color32::from_white_alpha(200)));
                    draw_label(painter, "Before", Pos2::new(x - 10.0, rect.top() + 12.0), Align2::RIGHT_TOP);
                    draw_label(painter, "After", Pos2::new(x + 10.0, rect.top() + 12.0), Align2::LEFT_TOP);
                }
            }
            if let Some(tool) = &self.crop_tool {
                tool.paint(&painter.with_clip_rect(rect), &self.frame_mapping());
            }
            let brush_hidden = self.mask_editor.tool() == Tool::Brush && space_held;
            self.mask_editor.paint(painter, &self.mapping(), if brush_hidden { None } else { self.cursor_pos });
        } else if !self.message.is_empty() && !self.loading {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                &self.message,
                FontId::proportional(14.0),
                theme::DIM_TEXT,
            );
        }

        if self.loading {
            draw_label(painter, "Loading…", Pos2::new(rect.center().x, rect.bottom() - 24.0), Align2::CENTER_BOTTOM);
        }
    }
}

/// Text on a dark rounded pill.
fn draw_label(painter: &Painter, text: &str, anchor: Pos2, align: Align2) {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(13.0), theme::TEXT);
    let size = galley.size() + Vec2::new(20.0, 8.0);
    let pill = align.anchor_size(anchor, size);
    painter.rect_filled(pill, CornerRadius::same((size.y / 2.0) as u8), Color32::from_black_alpha(165));
    painter.galley(pill.center() - galley.size() / 2.0, galley, theme::TEXT);
}
