//! The side panels. Each draws from the current edits and pushes [`Action`]s; none of them
//! changes the edits directly.

use std::path::{Path, PathBuf};

use egui::{Color32, RichText, Ui};
use iris_core::color::{MAX_TEMPERATURE, MAX_TINT, MIN_TEMPERATURE};
use iris_core::curve::{inverse_s_curve, linear_curve, s_curve};
use iris_core::mask::LOCAL_ADJUSTMENT_FIELDS;
use iris_core::{
    BasicAdjustments, BrushMode, HslAdjustments, HslBand, HslColor, Mask, MaskType, PhotoMetadata, ToneCurve,
    WhiteBalance,
};
use iris_persist::{PresetEntry, PresetLibrary, sidecar_path_for};

use crate::action::Action;
use crate::curve_editor::CurveEditor;
use crate::mask_editor::{Brush, Tool};
use crate::theme;
use crate::widgets::{self, MARGIN, Slider, padded, panel_title, section_label, small_button, small_toggle};

// --- Basic ------------------------------------------------------------------------

/// Temperature slider moves evenly in mired (1/K), which matches perceived change.
fn kelvin_to_position(kelvin: f64) -> f64 {
    let (lo, hi) = (1e6 / f64::from(MAX_TEMPERATURE), 1e6 / f64::from(MIN_TEMPERATURE));
    (hi - 1e6 / kelvin) / (hi - lo)
}

fn position_to_kelvin(position: f64) -> f64 {
    let (lo, hi) = (1e6 / f64::from(MAX_TEMPERATURE), 1e6 / f64::from(MIN_TEMPERATURE));
    1e6 / (hi - position * (hi - lo))
}

/// The "Basic" develop controls: white balance, tone and presence.
pub fn develop(
    ui: &mut Ui,
    basic: &BasicAdjustments,
    as_shot: WhiteBalance,
    eyedropper: bool,
    actions: &mut Vec<Action>,
) {
    panel_title(ui, "BASIC", |ui| {
        if small_button(ui, "Reset", "Reset all basic adjustments").clicked() {
            actions.push(Action::ResetBasic);
        }
    });

    ui.horizontal(|ui| {
        ui.add_space(MARGIN);
        ui.label(RichText::new("White Balance").strong().color(theme::SECTION_TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(MARGIN);
            if small_toggle(ui, eyedropper, "Pick", "Click a neutral grey or white area in the photo (Esc to cancel)")
                .clicked()
            {
                actions.push(Action::SetEyedropper(!eyedropper));
            }
            if small_button(ui, "Auto", "Estimate white balance from the photo").clicked() {
                actions.push(Action::AutoWhiteBalance);
            }
            if small_button(ui, "As Shot", "Use the camera's white balance").clicked() {
                actions.push(Action::SetWhiteBalance(as_shot));
            }
        });
    });

    let mut changed = *basic;
    let mut edited = false;
    let response = widgets::Response { to_position: kelvin_to_position, to_value: position_to_kelvin };
    let temperature = Slider::new("Temperature", f64::from(MIN_TEMPERATURE), f64::from(MAX_TEMPERATURE), 0)
        .default_value(f64::from(as_shot.temperature))
        .response(response)
        .gradient(theme::TEMPERATURE_GRADIENT);
    if let Some(v) = temperature.show(ui, f64::from(basic.white_balance.temperature)) {
        changed.white_balance.temperature = v as f32;
        edited = true;
    }
    let tint = Slider::new("Tint", f64::from(-MAX_TINT), f64::from(MAX_TINT), 0)
        .default_value(f64::from(as_shot.tint))
        .gradient(theme::TINT_GRADIENT);
    if let Some(v) = tint.show(ui, f64::from(basic.white_balance.tint)) {
        changed.white_balance.tint = v as f32;
        edited = true;
    }

    let mut slider = |ui: &mut Ui, name: &str, min: f64, max: f64, decimals: usize, value: &mut f32| {
        if let Some(v) = Slider::new(name, min, max, decimals).show(ui, f64::from(*value)) {
            *value = v as f32;
            edited = true;
        }
    };
    ui.add_space(6.0);
    section_label(ui, "Tone");
    slider(ui, "Exposure", -5.0, 5.0, 2, &mut changed.exposure);
    slider(ui, "Contrast", -100.0, 100.0, 0, &mut changed.contrast);
    slider(ui, "Highlights", -100.0, 100.0, 0, &mut changed.highlights);
    slider(ui, "Shadows", -100.0, 100.0, 0, &mut changed.shadows);
    slider(ui, "Whites", -100.0, 100.0, 0, &mut changed.whites);
    slider(ui, "Blacks", -100.0, 100.0, 0, &mut changed.blacks);
    ui.add_space(6.0);
    section_label(ui, "Presence");
    slider(ui, "Vibrance", -100.0, 100.0, 0, &mut changed.vibrance);
    slider(ui, "Saturation", -100.0, 100.0, 0, &mut changed.saturation);
    if edited {
        actions.push(Action::EditBasic(changed));
    }
    ui.add_space(8.0);
}

// --- Tone curve -------------------------------------------------------------------

pub fn tone_curve(
    ui: &mut Ui,
    curve: &ToneCurve,
    editor: &mut CurveEditor,
    histogram: Option<&[f32; 256]>,
    actions: &mut Vec<Action>,
) {
    panel_title(ui, "TONE CURVE", |ui| {
        if small_button(ui, "Reset", "Reset to a straight line").clicked() {
            actions.push(Action::ChooseCurve(ToneCurve::default()));
        }
    });

    let presets = [
        ("Linear", linear_curve()),
        ("S-Curve (more contrast)", s_curve()),
        ("Inverse S (less contrast)", inverse_s_curve()),
    ];
    let current = presets.iter().find(|(_, p)| *p == curve.rgb).map_or("Custom", |(name, _)| *name);
    padded(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Curve").color(theme::LABEL_TEXT));
            egui::ComboBox::from_id_salt("curve-preset").width(ui.available_width()).selected_text(current).show_ui(
                ui,
                |ui| {
                    for (name, points) in &presets {
                        if ui.selectable_label(*name == current, *name).clicked() {
                            actions.push(Action::ChooseCurve(ToneCurve { rgb: points.clone() }));
                        }
                    }
                },
            );
        });
    });
    if let Some(points) = editor.ui(ui, &curve.rgb, histogram) {
        actions.push(Action::EditCurve(ToneCurve { rgb: points }));
    }
    ui.add_space(8.0);
}

// --- Colour -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HslProperty {
    #[default]
    Hue,
    Saturation,
    Luminance,
}

impl HslProperty {
    fn name(self) -> &'static str {
        match self {
            HslProperty::Hue => "Hue",
            HslProperty::Saturation => "Saturation",
            HslProperty::Luminance => "Luminance",
        }
    }

    fn value(self, band: &mut HslBand) -> &mut f32 {
        match self {
            HslProperty::Hue => &mut band.hue,
            HslProperty::Saturation => &mut band.saturation,
            HslProperty::Luminance => &mut band.luminance,
        }
    }
}

/// Approximate display hue (HSV degrees) of each colour range, for the slider grooves.
const DISPLAY_HUE: [i32; 8] = [0, 30, 55, 120, 180, 220, 270, 310];

fn range_color(index: usize, hue_offset: i32, saturation: u8, value: u8) -> Color32 {
    let hue = (DISPLAY_HUE[index] + hue_offset).rem_euclid(360) as f32 / 360.0;
    egui::ecolor::Hsva::new(hue, f32::from(saturation) / 255.0, f32::from(value) / 255.0, 1.0).into()
}

pub fn hsl(ui: &mut Ui, hsl: &HslAdjustments, property: &mut HslProperty, actions: &mut Vec<Action>) {
    panel_title(ui, "COLOR", |ui| {
        if small_button(ui, "Reset", "Reset all HSL adjustments").clicked() {
            actions.push(Action::ResetHsl);
        }
    });
    padded(ui, |ui| {
        ui.columns(3, |columns| {
            for (column, p) in
                columns.iter_mut().zip([HslProperty::Hue, HslProperty::Saturation, HslProperty::Luminance])
            {
                let button = egui::Button::selectable(*property == p, RichText::new(p.name()).size(11.0));
                if column.add_sized([column.available_width(), 20.0], button).clicked() {
                    *property = p;
                }
            }
        });
    });
    ui.add_space(4.0);
    for color in HslColor::ALL {
        let i = color as usize;
        // The groove previews the effect: towards the neighbouring hues, from grey to
        // vivid, or from dark to light.
        let gradient = match property {
            HslProperty::Hue => (range_color(i, -30, 200, 210), range_color(i, 30, 200, 210)),
            HslProperty::Saturation => (range_color(i, 0, 0, 150), range_color(i, 0, 230, 210)),
            HslProperty::Luminance => (range_color(i, 0, 220, 70), range_color(i, 0, 110, 245)),
        };
        let mut band = hsl[color];
        let value = f64::from(*property.value(&mut band));
        if let Some(v) = Slider::new(color.name(), -100.0, 100.0, 0).gradient(gradient).show(ui, value) {
            let mut edited = *hsl;
            *property.value(&mut edited[color]) = v as f32;
            actions.push(Action::EditHsl(edited, format!("{} {}", color.name(), property.name())));
        }
    }
    ui.add_space(8.0);
}

// --- Masks ------------------------------------------------------------------------

/// Brush Size 1..100 -> radius 0.2%..20% of the long edge.
const RADIUS_PER_SIZE: f32 = 0.002;

/// Mask panel state that is not part of the edits: the tool and brush.
pub struct MaskPanel {
    pub tool: Tool,
    pub brush: Brush,
    pub overlay: bool,
    /// The mask the panel last showed, to notice a new selection.
    shown: Option<(usize, MaskType, String)>,
}

impl Default for MaskPanel {
    fn default() -> Self {
        Self { tool: Tool::Brush, brush: Brush::default(), overlay: true, shown: None }
    }
}

impl MaskPanel {
    pub fn new(overlay: bool) -> Self {
        Self { overlay, ..Default::default() }
    }

    /// Changes the brush size by a number of steps ([ and ] keys).
    pub fn step_brush_size(&mut self, steps: i32) {
        let size = f64::from((self.brush.radius / RADIUS_PER_SIZE).round());
        let mut next = (size * 1.15f64.powi(steps)).round();
        if next == size {
            next += f64::from(steps.signum());
        }
        self.brush.radius = next.clamp(1.0, 100.0) as f32 * RADIUS_PER_SIZE;
    }

    /// A newly selected gradient starts with its handles; a brush mask always paints.
    pub fn follow_selection(&mut self, masks: &[Mask], selected: Option<usize>) {
        let current = selected.and_then(|i| masks.get(i).map(|m| (i, m.mask_type, m.name.clone())));
        if current != self.shown {
            if let Some((_, mask_type, _)) = &current {
                self.tool = if *mask_type == MaskType::Brush { Tool::Brush } else { Tool::Shape };
            }
            self.shown = current;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, masks: &[Mask], selected: Option<usize>, actions: &mut Vec<Action>) {
        self.follow_selection(masks, selected);
        panel_title(ui, "MASKS", |ui| {
            for (mask_type, name) in
                [(MaskType::Radial, "Radial"), (MaskType::Linear, "Linear"), (MaskType::Brush, "Brush")]
            {
                let tip = format!("Add a {} mask", mask_type.name().to_lowercase());
                if small_button(ui, &format!("+ {name}"), &tip).clicked() {
                    actions.push(Action::AddMask(mask_type));
                }
            }
        });

        padded(ui, |ui| {
            if masks.is_empty() {
                ui.label(RichText::new("Add a mask to adjust part of the photo (M).").color(theme::DIM_TEXT));
            } else {
                egui::Frame::NONE
                    .fill(Color32::from_rgb(0x1b, 0x1c, 0x1f))
                    .stroke(egui::Stroke::new(1.0, Color32::from_rgb(0x2f, 0x31, 0x36)))
                    .corner_radius(3)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("mask-list")
                            .max_height(96.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                for (i, mask) in masks.iter().enumerate() {
                                    let response = ui
                                        .add_sized(
                                            [ui.available_width(), 20.0],
                                            egui::Button::selectable(selected == Some(i), mask.name.as_str())
                                                .frame_when_inactive(false),
                                        )
                                        .on_hover_text(mask.mask_type.name());
                                    if response.clicked() && selected != Some(i) {
                                        actions.push(Action::SelectMask(Some(i)));
                                    }
                                }
                            });
                    });
            }
            ui.horizontal(|ui| {
                let mut overlay = self.overlay;
                if ui
                    .checkbox(&mut overlay, "Show overlay")
                    .on_hover_text("Tint the selected mask's area (O)")
                    .changed()
                {
                    self.overlay = overlay;
                    actions.push(Action::MaskToolChanged);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let delete =
                        ui.add_enabled(selected.is_some(), egui::Button::new(RichText::new("Delete").size(11.0)));
                    if delete.on_hover_text("Delete the selected mask").clicked()
                        && let Some(i) = selected
                    {
                        actions.push(Action::DeleteMask(i));
                    }
                });
            });
        });

        let Some(mask) = selected.and_then(|i| masks.get(i)) else {
            ui.add_space(8.0);
            return;
        };

        // Tool row.
        ui.horizontal(|ui| {
            ui.add_space(MARGIN);
            ui.label(RichText::new("Edit").strong().color(theme::SECTION_TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(MARGIN);
                let tools = [
                    (BrushMode::Erase, "Erase", "Paint to remove earlier brush strokes"),
                    (BrushMode::Subtract, "Subtract", "Paint to remove the effect, gradient included"),
                    (BrushMode::Add, "Add", "Paint the mask in (Alt+drag erases)"),
                ];
                let mut changed = false;
                for (mode, name, tip) in tools {
                    let on = self.tool == Tool::Brush && self.brush.mode == mode;
                    if small_toggle(ui, on, name, tip).clicked() {
                        self.brush.mode = mode;
                        self.tool = Tool::Brush;
                        changed = true;
                    }
                }
                if mask.mask_type != MaskType::Brush
                    && small_toggle(
                        ui,
                        self.tool == Tool::Shape,
                        "Shape",
                        "Drag the handles, or drag on the photo to draw the gradient again",
                    )
                    .clicked()
                {
                    self.tool = Tool::Shape;
                    changed = true;
                }
                if changed {
                    actions.push(Action::MaskToolChanged);
                }
            });
        });

        // Brush.
        if self.tool == Tool::Brush {
            let mut changed = false;
            let size = Slider::new("Brush Size", 1.0, 100.0, 0).default_value(25.0).tooltip("[ and ] change the size");
            if let Some(v) = size.show(ui, f64::from((self.brush.radius / RADIUS_PER_SIZE).round())) {
                self.brush.radius = v as f32 * RADIUS_PER_SIZE;
                changed = true;
            }
            if let Some(v) = Slider::new("Brush Feather", 0.0, 100.0, 0)
                .default_value(50.0)
                .show(ui, f64::from(self.brush.feather * 100.0).round())
            {
                self.brush.feather = (v / 100.0) as f32;
                changed = true;
            }
            if let Some(v) = Slider::new("Brush Opacity", 1.0, 100.0, 0)
                .default_value(100.0)
                .show(ui, f64::from(self.brush.opacity * 100.0).round())
            {
                self.brush.opacity = (v / 100.0) as f32;
                changed = true;
            }
            if changed {
                actions.push(Action::MaskToolChanged);
            }
        }

        // Shape sliders: lengths are percentages of the photo's long edge.
        let mut shape =
            |ui: &mut Ui, name: &str, min: f64, max: f64, default: f64, value: f64, apply: fn(&mut Mask, f32)| {
                if let Some(v) = Slider::new(name, min, max, 0).default_value(default).show(ui, value.round()) {
                    let mut m = mask.clone();
                    apply(&mut m, v as f32);
                    actions.push(Action::EditMask(m.sanitized(), format!("{} {name}", mask.name)));
                }
            };
        match mask.mask_type {
            MaskType::Linear => {
                let l = mask.linear;
                shape(ui, "Angle", -180.0, 180.0, 0.0, f64::from(l.angle), |m, v| m.linear.angle = v);
                shape(ui, "Feather", 0.0, 100.0, 30.0, f64::from(l.feather * 100.0), |m, v| {
                    m.linear.feather = v / 100.0
                });
            }
            MaskType::Radial => {
                let r = mask.radial;
                shape(ui, "Width", 1.0, 200.0, 40.0, f64::from(r.width * 100.0), |m, v| m.radial.width = v / 100.0);
                shape(ui, "Height", 1.0, 200.0, 30.0, f64::from(r.height * 100.0), |m, v| m.radial.height = v / 100.0);
                shape(ui, "Rotation", -180.0, 180.0, 0.0, f64::from(r.rotation), |m, v| m.radial.rotation = v);
                shape(ui, "Feather", 0.0, 100.0, 50.0, f64::from(r.feather * 100.0), |m, v| {
                    m.radial.feather = v / 100.0
                });
            }
            MaskType::Brush => {}
        }

        padded(ui, |ui| {
            let mut invert = mask.invert;
            if ui
                .checkbox(&mut invert, "Invert")
                .on_hover_text("Apply the adjustments outside the mask instead")
                .changed()
            {
                actions.push(Action::EditMask(Mask { invert, ..mask.clone() }, format!("{} Invert", mask.name)));
            }
        });

        section_label(ui, "Adjustments");
        for field in &LOCAL_ADJUSTMENT_FIELDS {
            let decimals = if field.key == "exposure" { 2 } else { 0 };
            let mut slider = Slider::new(field.label, f64::from(field.minimum), f64::from(field.maximum), decimals);
            if field.key == "temperature" {
                slider = slider.gradient(theme::TEMPERATURE_GRADIENT);
            }
            if let Some(v) = slider.show(ui, f64::from(field.get(&mask.adjustments))) {
                let mut m = mask.clone();
                *(field.value)(&mut m.adjustments) = v as f32;
                actions.push(Action::EditMask(m, format!("{} {}", mask.name, field.label)));
            }
        }
        ui.add_space(8.0);
    }
}

// --- Presets ----------------------------------------------------------------------

pub fn presets(ui: &mut Ui, library: &PresetLibrary, actions: &mut Vec<Action>) {
    panel_title(ui, "PRESETS", |ui| {
        if small_button(ui, "Save Preset…", "Save the current settings as a preset").clicked() {
            actions.push(Action::ShowSavePreset);
        }
    });
    padded(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("presets").max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            let entries = library.presets();
            let mut start = 0;
            while start < entries.len() {
                let folder = &entries[start].folder;
                let end =
                    entries[start..].iter().position(|e| &e.folder != folder).map_or(entries.len(), |n| start + n);
                egui::CollapsingHeader::new(RichText::new(folder).strong())
                    .id_salt(("preset-folder", folder))
                    .default_open(true)
                    .show(ui, |ui| {
                        for entry in &entries[start..end] {
                            preset_item(ui, entry, actions);
                        }
                    });
                start = end;
            }
        });
    });
    ui.add_space(6.0);
}

fn preset_item(ui: &mut Ui, entry: &PresetEntry, actions: &mut Vec<Action>) {
    let tooltip = if entry.built_in { "Built-in preset".to_owned() } else { entry.file_path.display().to_string() };
    let response =
        ui.add(egui::Button::new(entry.preset.name.as_str()).frame_when_inactive(false)).on_hover_text(tooltip);
    if response.clicked() {
        actions.push(Action::ApplyPreset(entry.preset.clone()));
    }
    response.context_menu(|ui| {
        if ui.button("Apply").clicked() {
            actions.push(Action::ApplyPreset(entry.preset.clone()));
        }
        ui.separator();
        ui.add_enabled_ui(!entry.built_in, |ui| {
            if ui.button("Rename…").clicked() {
                actions.push(Action::RenamePreset(entry.clone()));
            }
            if ui.button("Move to Folder…").clicked() {
                actions.push(Action::MovePreset(entry.clone()));
            }
            if ui.button("Delete").clicked() {
                actions.push(Action::DeletePreset(entry.clone()));
            }
        });
    });
}

// --- Info -------------------------------------------------------------------------

fn format_shutter(seconds: f32) -> String {
    if seconds >= 0.3 {
        format!("{} s", format_significant(f64::from(seconds), 2))
    } else {
        format!("1/{} s", (1.0 / f64::from(seconds)).round())
    }
}

/// Like printf("%g") with a given number of significant digits.
fn format_significant(v: f64, digits: i32) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let magnitude = v.abs().log10().floor() as i32;
    let decimals = (digits - 1 - magnitude).max(0) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

fn format_orientation(orientation: i32) -> &'static str {
    match orientation {
        3 => "Rotated 180°",
        6 => "Rotated 90° CW",
        8 => "Rotated 90° CCW",
        _ => "Normal",
    }
}

fn format_date(timestamp: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(timestamp, 0).single() {
        Some(t) => t.format("%Y-%m-%d %H:%M").to_string(),
        None => String::new(),
    }
}

pub fn info(ui: &mut Ui, metadata: Option<&PhotoMetadata>, file_name: &str) {
    panel_title(ui, "INFO", |_| {});
    const UNKNOWN: &str = "—";
    let value = |s: String| if s.is_empty() { UNKNOWN.to_owned() } else { s };
    let rows: Vec<(&str, String)> = match metadata {
        None => ["File", "Camera", "Lens", "Exposure", "ISO", "Focal length", "Date", "Size", "Orientation"]
            .into_iter()
            .map(|k| (k, UNKNOWN.to_owned()))
            .collect(),
        Some(m) => {
            let mut make = m.make.trim().to_owned();
            let model = m.model.trim();
            if model.to_lowercase().starts_with(&make.to_lowercase()) {
                make.clear();
            }
            let mut exposure = Vec::new();
            if m.shutter_seconds > 0.0 {
                exposure.push(format_shutter(m.shutter_seconds));
            }
            if m.aperture > 0.0 {
                exposure.push(format!("f/{}", format_significant(f64::from(m.aperture), 2)));
            }
            vec![
                ("File", value(file_name.to_owned())),
                ("Camera", value(format!("{make} {model}").trim().to_owned())),
                ("Lens", value(m.lens.trim().to_owned())),
                ("Exposure", value(exposure.join("  ·  "))),
                ("ISO", value(if m.iso > 0.0 { format!("{}", m.iso.round()) } else { String::new() })),
                (
                    "Focal length",
                    value(if m.focal_length_mm > 0.0 {
                        format!("{} mm", format_significant(f64::from(m.focal_length_mm), 4))
                    } else {
                        String::new()
                    }),
                ),
                ("Date", value(if m.timestamp > 0 { format_date(m.timestamp) } else { String::new() })),
                (
                    "Size",
                    value(if m.width > 0 {
                        format!("{} × {}  ({:.1} MP)", m.width, m.height, m.width as f64 * m.height as f64 / 1e6)
                    } else {
                        String::new()
                    }),
                ),
                ("Orientation", format_orientation(m.orientation).to_owned()),
            ]
        }
    };
    for (key, v) in rows {
        widgets::info_row(ui, key, &v);
    }
    ui.add_space(12.0);
}

// --- Library ----------------------------------------------------------------------

/// The RAW photos in the open photo's folder.
#[derive(Default)]
pub struct Library {
    folder: Option<PathBuf>,
    /// (path, file name, has saved edits)
    photos: Vec<(PathBuf, String, bool)>,
    scroll_to_current: bool,
}

impl Library {
    pub fn show_folder_of(&mut self, path: &Path) {
        let folder = path.parent().map(Path::to_owned);
        self.scroll_to_current = true;
        if folder == self.folder {
            return;
        }
        self.folder = folder;
        self.photos.clear();
        let Some(dir) = &self.folder else { return };
        let mut photos: Vec<(PathBuf, String, bool)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && iris_raw::is_raw_file(p))
            .map(|p| {
                let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let edited = sidecar_path_for(&p).exists();
                (p, name, edited)
            })
            .collect();
        photos.sort_by_key(|(_, name, _)| name.to_lowercase());
        self.photos = photos;
    }

    pub fn set_edited(&mut self, path: &Path, edited: bool) {
        for photo in self.photos.iter_mut().filter(|p| p.0 == path) {
            photo.2 = edited;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, current: Option<&Path>, actions: &mut Vec<Action>) {
        panel_title(ui, "LIBRARY", |_| {});
        let folder_name = self
            .folder
            .as_ref()
            .and_then(|f| f.file_name())
            .map_or("No folder".to_owned(), |n| n.to_string_lossy().into_owned());
        padded(ui, |ui| {
            let label = ui.label(RichText::new(folder_name).color(theme::BODY_TEXT));
            if let Some(folder) = &self.folder {
                label.on_hover_text(folder.display().to_string());
            }
        });
        ui.add_space(4.0);
        let scroll = std::mem::take(&mut self.scroll_to_current);
        egui::ScrollArea::vertical().id_salt("library").auto_shrink([false, false]).show(ui, |ui| {
            for (path, name, edited) in &self.photos {
                let selected = current == Some(path.as_path());
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, name)
                });
                let fill = if selected {
                    theme::SELECTION
                } else if response.hovered() {
                    Color32::from_rgb(0x2c, 0x2e, 0x32)
                } else {
                    Color32::TRANSPARENT
                };
                let painter = ui.painter();
                painter.rect_filled(rect, 0.0, fill);
                // A small dot marks photos that have saved edits.
                if *edited {
                    painter.circle_filled(egui::pos2(rect.left() + MARGIN + 3.0, rect.center().y), 3.0, theme::ACCENT);
                }
                let color = if selected { Color32::WHITE } else { theme::BODY_TEXT };
                painter.text(
                    egui::pos2(rect.left() + MARGIN + 12.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(13.0),
                    color,
                );
                let response = if *edited { response.on_hover_text("Has saved edits") } else { response };
                if response.clicked() && !selected {
                    actions.push(Action::OpenPhoto(path.clone()));
                }
                if selected && scroll {
                    response.scroll_to_me(None);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_response_round_trips() {
        for k in [2000.0, 3200.0, 5500.0, 15000.0] {
            assert!((position_to_kelvin(kelvin_to_position(k)) - k).abs() < 1e-6);
        }
        assert!(kelvin_to_position(2000.0).abs() < 1e-9);
        assert!((kelvin_to_position(15000.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn brush_size_steps() {
        let mut panel = MaskPanel::default(); // size 25
        panel.step_brush_size(1);
        assert!((panel.brush.radius / RADIUS_PER_SIZE - 29.0).abs() < 1e-3);
        panel.brush.radius = RADIUS_PER_SIZE; // size 1
        panel.step_brush_size(1);
        assert!((panel.brush.radius / RADIUS_PER_SIZE - 2.0).abs() < 1e-3); // always moves
        panel.step_brush_size(-5);
        assert!((panel.brush.radius / RADIUS_PER_SIZE - 1.0).abs() < 1e-3); // clamped
    }

    #[test]
    fn selecting_a_gradient_switches_to_its_handles() {
        let mut panel = MaskPanel::default();
        let masks = vec![Mask::new(MaskType::Brush, &[]), Mask::new(MaskType::Radial, &[])];
        panel.follow_selection(&masks, Some(1));
        assert_eq!(panel.tool, Tool::Shape);
        panel.tool = Tool::Brush; // the user switches to painting
        panel.follow_selection(&masks, Some(1));
        assert_eq!(panel.tool, Tool::Brush); // same mask: keep the tool
        panel.follow_selection(&masks, Some(0));
        assert_eq!(panel.tool, Tool::Brush);
    }

    #[test]
    fn formats_metadata() {
        assert_eq!(format_shutter(0.00625), "1/160 s");
        assert_eq!(format_shutter(2.5), "2.5 s");
        assert_eq!(format_significant(2.8, 2), "2.8");
        assert_eq!(format_significant(11.0, 2), "11");
        assert_eq!(format_significant(27.0, 4), "27");
    }
}
