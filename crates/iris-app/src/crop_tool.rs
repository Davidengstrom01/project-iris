//! Cropping on the photo: the crop rectangle over the whole straightened frame, with
//! handles to resize it and a drag inside to move it. Used by the image view, which passes
//! in where the frame is on screen.

use egui::{Color32, CursorIcon, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use iris_core::Crop;
use iris_core::crop::frame_size;

/// Screen points from a corner or edge that still grab it.
const GRAB: f32 = 10.0;
/// Smallest crop, in frame pixels.
const MIN_SIZE: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Move,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Handle {
    /// Which sides the handle moves: (left, right, top, bottom).
    fn sides(self) -> (bool, bool, bool, bool) {
        match self {
            Handle::Move => (true, true, true, true),
            Handle::Left => (true, false, false, false),
            Handle::Right => (false, true, false, false),
            Handle::Top => (false, false, true, false),
            Handle::Bottom => (false, false, false, true),
            Handle::TopLeft => (true, false, true, false),
            Handle::TopRight => (false, true, true, false),
            Handle::BottomLeft => (true, false, false, true),
            Handle::BottomRight => (false, true, false, true),
        }
    }

    pub fn cursor(self) -> CursorIcon {
        match self {
            Handle::Move => CursorIcon::Move,
            Handle::Left | Handle::Right => CursorIcon::ResizeHorizontal,
            Handle::Top | Handle::Bottom => CursorIcon::ResizeVertical,
            Handle::TopLeft | Handle::BottomRight => CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNeSw,
        }
    }
}

/// Where the straightened frame is drawn: screen = origin + frame pixel * scale.
#[derive(Clone, Copy, Debug)]
pub struct FrameMapping {
    pub origin: Pos2,
    pub scale: f32,
    pub frame_size: [usize; 2],
}

impl FrameMapping {
    fn frame(&self) -> Vec2 {
        vec2(self.frame_size[0] as f32, self.frame_size[1] as f32)
    }
}

/// A crop rectangle in frame pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Box2 {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Box2 {
    fn of(crop: &Crop, frame: Vec2) -> Self {
        Self {
            left: crop.left * frame.x,
            top: crop.top * frame.y,
            right: crop.right * frame.x,
            bottom: crop.bottom * frame.y,
        }
    }

    fn apply(self, crop: Crop, frame: Vec2) -> Crop {
        Crop {
            left: self.left / frame.x,
            top: self.top / frame.y,
            right: self.right / frame.x,
            bottom: self.bottom / frame.y,
            ..crop
        }
    }

    fn width(&self) -> f32 {
        self.right - self.left
    }

    fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

#[derive(Default)]
pub struct CropTool {
    crop: Crop,
    photo_size: [usize; 2],
    /// The handle being dragged, where the drag started (screen) and the crop then.
    drag: Option<(Handle, Pos2, Crop)>,
}

impl CropTool {
    pub fn new(crop: Crop, photo_size: [usize; 2]) -> Self {
        Self { crop, photo_size, drag: None }
    }

    /// Follows the edits (keeping a drag in progress).
    pub fn set_crop(&mut self, crop: Crop, photo_size: [usize; 2]) {
        self.crop = crop;
        self.photo_size = photo_size;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    fn screen_rect(&self, m: &FrameMapping) -> Rect {
        let b = Box2::of(&self.crop, m.frame());
        Rect::from_min_max(m.origin + vec2(b.left, b.top) * m.scale, m.origin + vec2(b.right, b.bottom) * m.scale)
    }

    pub fn hit(&self, pos: Pos2, m: &FrameMapping) -> Option<Handle> {
        let r = self.screen_rect(m);
        let near = |a: f32, b: f32| (a - b).abs() <= GRAB;
        let within_x = pos.x >= r.left() - GRAB && pos.x <= r.right() + GRAB;
        let within_y = pos.y >= r.top() - GRAB && pos.y <= r.bottom() + GRAB;
        let (l, rt, t, b) =
            (near(pos.x, r.left()), near(pos.x, r.right()), near(pos.y, r.top()), near(pos.y, r.bottom()));
        let handle = match (l, rt, t, b) {
            (true, _, true, _) => Handle::TopLeft,
            (_, true, true, _) => Handle::TopRight,
            (true, _, _, true) => Handle::BottomLeft,
            (_, true, _, true) => Handle::BottomRight,
            (true, ..) if within_y => Handle::Left,
            (_, true, ..) if within_y => Handle::Right,
            (_, _, true, _) if within_x => Handle::Top,
            (.., true) if within_x => Handle::Bottom,
            _ if r.contains(pos) => Handle::Move,
            _ => return None,
        };
        Some(handle)
    }

    pub fn press(&mut self, pos: Pos2, m: &FrameMapping) -> bool {
        match self.hit(pos, m) {
            Some(handle) => {
                self.drag = Some((handle, pos, self.crop));
                true
            }
            None => false,
        }
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    /// Continues a drag; returns the new crop if it changed. The rectangle keeps its aspect
    /// ratio when one is locked and never leaves the straightened photo.
    pub fn drag_to(&mut self, pos: Pos2, m: &FrameMapping) -> Option<Crop> {
        let (handle, press, start) = self.drag?;
        let frame = m.frame();
        let delta = (pos - press) / m.scale;
        let s = Box2::of(&start, frame);
        let (left, right, top, bottom) = handle.sides();
        let mut b = s;

        if handle == Handle::Move {
            let dx = delta.x.clamp(-s.left, frame.x - s.right);
            let dy = delta.y.clamp(-s.top, frame.y - s.bottom);
            b = Box2 { left: s.left + dx, right: s.right + dx, top: s.top + dy, bottom: s.bottom + dy };
        } else {
            if left {
                b.left = (s.left + delta.x).clamp(0.0, s.right - MIN_SIZE);
            }
            if right {
                b.right = (s.right + delta.x).clamp(s.left + MIN_SIZE, frame.x);
            }
            if top {
                b.top = (s.top + delta.y).clamp(0.0, s.bottom - MIN_SIZE);
            }
            if bottom {
                b.bottom = (s.bottom + delta.y).clamp(s.top + MIN_SIZE, frame.y);
            }
            if start.aspect > 0.0 {
                b = locked(b, s, handle, start.aspect, frame);
            }
        }

        let target = b.apply(start, frame);
        let [w, h] = self.photo_size;
        let next = self.crop.toward(target, w, h);
        (next != self.crop).then(|| {
            self.crop = next;
            next
        })
    }

    /// Shades the frame outside the crop and draws the rectangle, a rule-of-thirds grid and
    /// the handles.
    pub fn paint(&self, painter: &Painter, m: &FrameMapping) {
        let frame = Rect::from_min_size(m.origin, m.frame() * m.scale);
        let r = self.screen_rect(m);
        let shade = Color32::from_black_alpha(150);
        for outside in [
            Rect::from_min_max(frame.min, pos2(frame.right(), r.top())),
            Rect::from_min_max(pos2(frame.left(), r.bottom()), frame.max),
            Rect::from_min_max(pos2(frame.left(), r.top()), pos2(r.left(), r.bottom())),
            Rect::from_min_max(pos2(r.right(), r.top()), pos2(frame.right(), r.bottom())),
        ] {
            if outside.is_positive() {
                painter.rect_filled(outside, 0.0, shade);
            }
        }
        let thirds = Stroke::new(1.0, Color32::from_white_alpha(if self.drag.is_some() { 120 } else { 60 }));
        for i in 1..3 {
            let f = i as f32 / 3.0;
            painter.vline(r.left() + r.width() * f, r.y_range(), thirds);
            painter.hline(r.x_range(), r.top() + r.height() * f, thirds);
        }
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_white_alpha(230)), egui::StrokeKind::Middle);

        // Corner brackets and edge bars.
        let handle = Stroke::new(3.0, Color32::WHITE);
        let arm = 14.0f32.min(r.width() / 3.0).min(r.height() / 3.0);
        for (corner, dx, dy) in [
            (r.left_top(), 1.0, 1.0),
            (r.right_top(), -1.0, 1.0),
            (r.left_bottom(), 1.0, -1.0),
            (r.right_bottom(), -1.0, -1.0),
        ] {
            painter.line_segment([corner, corner + vec2(arm * dx, 0.0)], handle);
            painter.line_segment([corner, corner + vec2(0.0, arm * dy)], handle);
        }
        let bar = arm / 2.0;
        let c = r.center();
        painter.line_segment([pos2(c.x - bar, r.top()), pos2(c.x + bar, r.top())], handle);
        painter.line_segment([pos2(c.x - bar, r.bottom()), pos2(c.x + bar, r.bottom())], handle);
        painter.line_segment([pos2(r.left(), c.y - bar), pos2(r.left(), c.y + bar)], handle);
        painter.line_segment([pos2(r.right(), c.y - bar), pos2(r.right(), c.y + bar)], handle);
    }
}

/// Resizes with a locked aspect ratio (width / height in pixels): a corner keeps the
/// opposite corner in place, an edge keeps the rectangle centred across it.
fn locked(b: Box2, s: Box2, handle: Handle, aspect: f32, frame: Vec2) -> Box2 {
    let (cx, cy) = ((s.left + s.right) / 2.0, (s.top + s.bottom) / 2.0);
    match handle {
        Handle::Left | Handle::Right => {
            // Height follows width, limited by the room above and below the centre.
            let room = 2.0 * cy.min(frame.y - cy);
            let w = b.width().min(room * aspect).max(MIN_SIZE);
            let h = w / aspect;
            let (left, right) = if handle == Handle::Left { (s.right - w, s.right) } else { (s.left, s.left + w) };
            Box2 { left, right, top: cy - h / 2.0, bottom: cy + h / 2.0 }
        }
        Handle::Top | Handle::Bottom => {
            let room = 2.0 * cx.min(frame.x - cx);
            let h = b.height().min(room / aspect).max(MIN_SIZE);
            let w = h * aspect;
            let (top, bottom) = if handle == Handle::Top { (s.bottom - h, s.bottom) } else { (s.top, s.top + h) };
            Box2 { left: cx - w / 2.0, right: cx + w / 2.0, top, bottom }
        }
        _ => {
            // Corners: the larger of the two proposed sizes wins, within the frame.
            let (left, _, top, _) = handle.sides();
            let anchor_x = if left { s.right } else { s.left };
            let anchor_y = if top { s.bottom } else { s.top };
            let room_x = if left { anchor_x } else { frame.x - anchor_x };
            let room_y = if top { anchor_y } else { frame.y - anchor_y };
            let w = b.width().max(b.height() * aspect).min(room_x).min(room_y * aspect).max(MIN_SIZE);
            let h = w / aspect;
            let (l, r) = if left { (anchor_x - w, anchor_x) } else { (anchor_x, anchor_x + w) };
            let (t, bt) = if top { (anchor_y - h, anchor_y) } else { (anchor_y, anchor_y + h) };
            Box2 { left: l, right: r, top: t, bottom: bt }
        }
    }
}

/// Aspect ratio choices for the crop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Aspect {
    Free,
    /// The photo's own.
    Original,
    /// Long side : short side; the crop's orientation decides which way round.
    Ratio(u16, u16),
}

pub const ASPECTS: [Aspect; 8] = [
    Aspect::Free,
    Aspect::Original,
    Aspect::Ratio(1, 1),
    Aspect::Ratio(5, 4),
    Aspect::Ratio(4, 3),
    Aspect::Ratio(3, 2),
    Aspect::Ratio(7, 5),
    Aspect::Ratio(16, 9),
];

impl Aspect {
    pub fn name(self) -> String {
        match self {
            Aspect::Free => "Free".into(),
            Aspect::Original => "Original".into(),
            Aspect::Ratio(a, b) => format!("{a} : {b}"),
        }
    }

    /// The locked width / height for this choice (0 = free), oriented like the current crop.
    pub fn value(self, crop: &Crop, photo_size: [usize; 2]) -> f32 {
        let [fw, fh] = frame_of(crop, photo_size);
        let portrait = crop.pixel_aspect(photo_size[0], photo_size[1]) < 1.0;
        let landscape = match self {
            Aspect::Free => return 0.0,
            Aspect::Original => fw.max(fh) as f32 / fw.min(fh).max(1) as f32,
            Aspect::Ratio(a, b) => f32::from(a) / f32::from(b),
        };
        if portrait { 1.0 / landscape } else { landscape }
    }

    /// Which choice a crop's locked aspect corresponds to (None = some other ratio).
    pub fn of(crop: &Crop, photo_size: [usize; 2]) -> Option<Aspect> {
        if crop.aspect <= 0.0 {
            return Some(Aspect::Free);
        }
        let landscape = crop.aspect.max(1.0 / crop.aspect);
        ASPECTS.into_iter().skip(1).find(|a| {
            let v = a.value(&Crop { aspect: 0.0, left: 0.0, top: 0.0, right: 1.0, bottom: 1.0, ..*crop }, photo_size);
            (v.max(1.0 / v) - landscape).abs() < 1e-3
        })
    }
}

/// The frame size (after quarter turns) of a photo.
pub fn frame_of(crop: &Crop, photo_size: [usize; 2]) -> [usize; 2] {
    let (w, h) = frame_size(photo_size[0], photo_size[1], crop.quarter_turns);
    [w, h]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 600 x 400 photo drawn at half size at (100, 50).
    fn mapping() -> FrameMapping {
        FrameMapping { origin: pos2(100.0, 50.0), scale: 0.5, frame_size: [600, 400] }
    }

    fn screen(x: f32, y: f32) -> Pos2 {
        pos2(100.0 + x * 0.5, 50.0 + y * 0.5)
    }

    fn tool(crop: Crop) -> CropTool {
        CropTool::new(crop, [600, 400])
    }

    #[test]
    fn aspect_choices() {
        let photo = [600, 400];
        let portrait = Crop { right: 0.5, ..Default::default() }; // 300 x 400
        assert!((Aspect::Ratio(3, 2).value(&portrait, photo) - 2.0 / 3.0).abs() < 1e-6);
        assert!((Aspect::Ratio(3, 2).value(&Crop::default(), photo) - 1.5).abs() < 1e-6);
        assert_eq!(Aspect::Free.value(&portrait, photo), 0.0);
        let locked = Crop { aspect: 0.8, ..Default::default() };
        assert_eq!(Aspect::of(&locked, photo), Some(Aspect::Ratio(5, 4)));
        assert_eq!(Aspect::of(&Crop { aspect: 1.5, ..Default::default() }, photo), Some(Aspect::Original));
        assert_eq!(Aspect::of(&Crop { aspect: 2.2, ..Default::default() }, photo), None);
    }

    #[test]
    fn handles_are_found() {
        let t = tool(Crop { left: 0.25, top: 0.25, right: 0.75, bottom: 0.75, ..Default::default() });
        let m = mapping();
        assert_eq!(t.hit(screen(150.0, 100.0), &m), Some(Handle::TopLeft));
        assert_eq!(t.hit(screen(450.0, 300.0), &m), Some(Handle::BottomRight));
        assert_eq!(t.hit(screen(150.0, 200.0), &m), Some(Handle::Left));
        assert_eq!(t.hit(screen(300.0, 300.0), &m), Some(Handle::Bottom));
        assert_eq!(t.hit(screen(300.0, 200.0), &m), Some(Handle::Move));
        assert_eq!(t.hit(screen(20.0, 20.0), &m), None);
    }

    #[test]
    fn dragging_an_edge_and_moving() {
        let m = mapping();
        let mut t = tool(Crop::default());
        assert!(t.press(screen(600.0, 200.0), &m)); // right edge
        let crop = t.drag_to(screen(450.0, 200.0), &m).unwrap();
        assert!((crop.right - 0.75).abs() < 1e-4 && crop.left == 0.0);
        // Dragging past the edge stops there.
        let crop = t.drag_to(screen(700.0, 200.0), &m).unwrap();
        assert!((crop.right - 1.0).abs() < 1e-4);
        t.drag_to(screen(300.0, 200.0), &m);
        t.release();

        // Move by the inside; it cannot leave the frame.
        assert!(t.press(screen(150.0, 200.0), &m));
        let crop = t.drag_to(screen(-500.0, 200.0), &m);
        assert!(crop.is_none() || crop.unwrap().left.abs() < 1e-4);
        let crop = t.drag_to(screen(250.0, 200.0), &m).unwrap();
        assert!((crop.left - 100.0 / 600.0).abs() < 1e-4 && (crop.right - 400.0 / 600.0).abs() < 1e-4);
    }

    #[test]
    fn locked_aspect_is_kept() {
        let m = mapping();
        let start = Crop::default().with_aspect(1.0, 600, 400); // 400 x 400, centred
        let mut t = tool(start);
        assert!(t.press(screen(500.0, 400.0), &m)); // bottom-right corner
        let crop = t.drag_to(screen(400.0, 250.0), &m).unwrap();
        let pixels = |c: &Crop| ((c.right - c.left) * 600.0, (c.bottom - c.top) * 400.0);
        let (w, h) = pixels(&crop);
        assert!((w - h).abs() < 0.5, "{w} x {h}");
        assert!((crop.left - start.left).abs() < 1e-5 && crop.top.abs() < 1e-5); // anchored
        t.release();

        // An edge keeps the rectangle centred across it.
        let mut t = tool(crop);
        let r = Box2::of(&crop, vec2(600.0, 400.0));
        assert!(t.press(screen(r.right, (r.top + r.bottom) / 2.0), &m));
        let edged = t.drag_to(screen(r.right - 50.0, (r.top + r.bottom) / 2.0), &m).unwrap();
        let (w, h) = pixels(&edged);
        assert!((w - h).abs() < 0.5 && (w - (r.width() - 50.0)).abs() < 0.5);
        assert!(((edged.top + edged.bottom) / 2.0 - (crop.top + crop.bottom) / 2.0).abs() < 1e-4);
    }

    #[test]
    fn straightened_crops_stay_inside_the_photo() {
        let m = mapping();
        let start = Crop { angle: 10.0, ..Default::default() }.constrained(600, 400);
        let mut t = tool(start);
        let r = Box2::of(&start, vec2(600.0, 400.0));
        assert!(t.press(screen(r.right, r.bottom), &m));
        // Pulling the corner outwards cannot reach the empty corner of the frame.
        let crop = t.drag_to(screen(600.0, 400.0), &m);
        let crop = crop.unwrap_or(start);
        assert!(crop.fits_photo(600, 400));
    }
}
