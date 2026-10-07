//! Editing a mask directly on the photo: painting brush strokes, and dragging the handles
//! of linear and radial gradients (or dragging out a new one). Used by the image view,
//! which passes in where the photo is on screen.

use std::f64::consts::PI;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use iris_core::crop::Affine;
use iris_core::mask::{MAX_POINTS_PER_STROKE, MAX_STROKES_PER_MASK};
use iris_core::{BrushMode, BrushStroke, LinearGradient, Mask, MaskPoint, MaskType, RadialGradient};

/// Screen points.
const HANDLE_RADIUS: f32 = 5.0;
const HIT_DISTANCE: f32 = 10.0;
/// Beyond the ellipse.
const ROTATION_ARM: f64 = 28.0;
/// Before a drag creates a gradient.
const MIN_CREATE_DRAG: f32 = 4.0;
/// Of the brush radius, between recorded points (segments are exact).
const STROKE_SPACING: f64 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Move / reshape the gradient.
    Shape,
    /// Paint strokes with the current brush.
    Brush,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub mode: BrushMode,
    /// Fraction of the long edge.
    pub radius: f32,
    pub feather: f32,
    pub opacity: f32,
}

impl Default for Brush {
    fn default() -> Self {
        Self { mode: BrushMode::Add, radius: 0.05, feather: 0.5, opacity: 1.0 }
    }
}

/// Where the photo is drawn: photo pixels -> screen points (a scale, plus the crop's
/// rotation and offset).
#[derive(Clone, Copy, Debug)]
pub struct Mapping {
    pub photo_to_screen: Affine,
    /// Screen points per photo pixel.
    pub scale: f32,
    /// Photo size in pixels.
    pub image_size: [usize; 2],
    /// Where the rendering is on screen; gradient lines are clipped to it.
    pub clip: Rect,
}

impl Mapping {
    fn size(&self) -> (f64, f64) {
        (self.image_size[0] as f64, self.image_size[1] as f64)
    }

    fn is_empty(&self) -> bool {
        self.image_size[0] == 0 || self.image_size[1] == 0
    }

    /// Long edge in screen points.
    pub fn long_edge(&self) -> f64 {
        self.image_size[0].max(self.image_size[1]) as f64 * f64::from(self.scale)
    }

    /// Screen point -> image pixels.
    fn to_image(self, pos: Pos2) -> Pt {
        let (x, y) = self.photo_to_screen.inverted().map(f64::from(pos.x), f64::from(pos.y));
        Pt::new(x, y)
    }

    /// Image pixels -> screen point.
    fn to_screen(self, p: Pt) -> Pos2 {
        let (x, y) = self.photo_to_screen.map(p.x, p.y);
        Pos2::new(x as f32, y as f32)
    }

    fn to_mask(self, pos: Pos2) -> MaskPoint {
        let p = self.to_image(pos);
        let (w, h) = self.size();
        MaskPoint::new((p.x / w.max(1.0)) as f32, (p.y / h.max(1.0)) as f32)
    }
}

/// A point or vector in image pixels (double precision, like the Qt version).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pt {
    x: f64,
    y: f64,
}

impl Pt {
    const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
}

impl std::ops::Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for Pt {
    type Output = Pt;
    fn mul(self, k: f64) -> Pt {
        Pt::new(self.x * k, self.y * k)
    }
}

impl std::ops::Div<f64> for Pt {
    type Output = Pt;
    fn div(self, k: f64) -> Pt {
        Pt::new(self.x / k, self.y / k)
    }
}

/// Direction perpendicular to a gradient, pointing to its full-effect side (image pixels,
/// y down). Angles are counter-clockwise on screen; 0 points up.
fn normal_for(degrees: f64) -> Pt {
    let a = degrees.to_radians();
    Pt::new(-a.sin(), -a.cos())
}

fn angle_of(normal: Pt) -> f64 {
    (-normal.x).atan2(-normal.y).to_degrees()
}

struct Frame {
    w: f64,
    h: f64,
    long_edge: f64,
}

impl Frame {
    fn new(m: &Mapping) -> Self {
        let (w, h) = m.size();
        Self { w, h, long_edge: w.max(h) }
    }
    fn to_pixels(&self, x: f32, y: f32) -> Pt {
        Pt::new(f64::from(x) * self.w, f64::from(y) * self.h)
    }
}

struct LinearHandles {
    center: Pt,
    /// Full effect.
    start: Pt,
    /// No effect.
    end: Pt,
    /// Unit direction of the lines.
    along: Pt,
}

fn linear_handles(g: &LinearGradient, f: &Frame) -> LinearHandles {
    let c = f.to_pixels(g.x, g.y);
    let n = normal_for(f64::from(g.angle));
    let half = f64::from(g.feather) * f.long_edge / 2.0;
    LinearHandles { center: c, start: c + n * half, end: c - n * half, along: Pt::new(-n.y, n.x) }
}

struct RadialHandles {
    center: Pt,
    /// Unit axes (width, height).
    u: Pt,
    v: Pt,
    rx: f64,
    ry: f64,
}

fn radial_handles(g: &RadialGradient, f: &Frame) -> RadialHandles {
    let a = f64::from(g.rotation).to_radians();
    RadialHandles {
        center: f.to_pixels(g.x, g.y),
        u: Pt::new(a.cos(), -a.sin()),
        v: Pt::new(a.sin(), a.cos()),
        rx: f64::from(g.width) * f.long_edge / 2.0,
        ry: f64::from(g.height) * f.long_edge / 2.0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    None,
    Paint,
    Create,
    Center,
    LinearStart,
    LinearEnd,
    RadialWidth,
    RadialHeight,
    RadialRotation,
}

pub struct MaskEditor {
    mask: Option<Mask>,
    tool: Tool,
    brush: Brush,
    drag: Drag,
    /// Screen position where the gesture started.
    press_pos: Pos2,
    /// The mask when the gesture started.
    press_mask: Mask,
}

impl Default for MaskEditor {
    fn default() -> Self {
        Self {
            mask: None,
            tool: Tool::Brush,
            brush: Brush::default(),
            drag: Drag::None,
            press_pos: Pos2::ZERO,
            press_mask: Mask::default(),
        }
    }
}

impl MaskEditor {
    pub fn set_mask(&mut self, mask: Option<&Mask>) {
        // Keep an in-progress gesture going if the mask is merely updated.
        if mask.is_none() || self.mask.as_ref().map(|m| m.mask_type) != mask.map(|m| m.mask_type) {
            self.drag = Drag::None;
        }
        self.mask = mask.cloned();
    }

    pub fn mask(&self) -> Option<&Mask> {
        self.mask.as_ref()
    }

    pub fn is_active(&self) -> bool {
        self.mask.is_some()
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn set_brush(&mut self, brush: Brush) {
        self.brush = brush;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag != Drag::None
    }

    fn hit_test(&self, pos: Pos2, m: &Mapping) -> Drag {
        let Some(mask) = &self.mask else { return Drag::None };
        if self.tool != Tool::Shape {
            return Drag::None;
        }
        let f = Frame::new(m);
        let near = |image: Pt| m.to_screen(image).distance(pos) <= HIT_DISTANCE;
        match mask.mask_type {
            MaskType::Linear => {
                let h = linear_handles(&mask.linear, &f);
                if near(h.center) {
                    return Drag::Center;
                }
                if near(h.start) {
                    return Drag::LinearStart;
                }
                if near(h.end) {
                    return Drag::LinearEnd;
                }
            }
            MaskType::Radial => {
                let h = radial_handles(&mask.radial, &f);
                if near(h.center) {
                    return Drag::Center;
                }
                if near(h.center - h.v * (h.ry + ROTATION_ARM / f64::from(m.scale))) {
                    return Drag::RadialRotation;
                }
                if near(h.center + h.u * h.rx) || near(h.center - h.u * h.rx) {
                    return Drag::RadialWidth;
                }
                if near(h.center + h.v * h.ry) || near(h.center - h.v * h.ry) {
                    return Drag::RadialHeight;
                }
            }
            MaskType::Brush => {}
        }
        Drag::None
    }

    pub fn is_over_handle(&self, pos: Pos2, m: &Mapping) -> bool {
        self.hit_test(pos, m) != Drag::None
    }

    /// Starts a gesture; returns true if one started. `erase_modifier` temporarily
    /// switches the brush to Erase.
    pub fn press(&mut self, pos: Pos2, m: &Mapping, erase_modifier: bool) -> bool {
        let Some(mask) = &mut self.mask else { return false };
        if m.is_empty() {
            return false;
        }
        self.press_pos = pos;
        self.press_mask = mask.clone();

        if self.tool == Tool::Brush {
            if mask.strokes.len() >= MAX_STROKES_PER_MASK {
                return false;
            }
            mask.strokes.push(BrushStroke {
                mode: if erase_modifier { BrushMode::Erase } else { self.brush.mode },
                radius: self.brush.radius,
                feather: self.brush.feather,
                opacity: self.brush.opacity,
                points: vec![m.to_mask(pos)],
            });
            self.drag = Drag::Paint;
            return true;
        }
        if mask.mask_type == MaskType::Brush {
            return false;
        }
        self.drag = match self.hit_test(pos, m) {
            Drag::None => Drag::Create, // drag out a new gradient
            hit => hit,
        };
        true
    }

    /// Continues the gesture; returns true if the mask changed.
    pub fn drag_to(&mut self, pos: Pos2, m: &Mapping) -> bool {
        if self.mask.is_none() || self.drag == Drag::None {
            return false;
        }
        if self.drag == Drag::Paint {
            let f = Frame::new(m);
            let min_long_edge = m.long_edge();
            let stroke = self.mask.as_mut().and_then(|mask| mask.strokes.last_mut()).expect("painting a stroke");
            let p = m.to_mask(pos);
            let last = *stroke.points.last().expect("a stroke has points");
            let distance = (f64::from(p.x - last.x) * f.w).hypot(f64::from(p.y - last.y) * f.h) / f.long_edge;
            let minimum = (f64::from(stroke.radius) * STROKE_SPACING).max(1.5 / min_long_edge);
            if distance < minimum || stroke.points.len() >= MAX_POINTS_PER_STROKE {
                return false;
            }
            stroke.points.push(p);
            return true;
        }
        if self.drag == Drag::Create && pos.distance(self.press_pos) < MIN_CREATE_DRAG {
            return false;
        }
        let before = self.mask.clone();
        self.reshape(pos, m);
        self.mask != before
    }

    fn reshape(&mut self, pos: Pos2, m: &Mapping) {
        let f = Frame::new(m);
        let p = m.to_image(pos);
        let p0 = m.to_image(self.press_pos); // where the drag started
        let to_normalized = |image: Pt| ((image.x / f.w) as f32, (image.y / f.h) as f32);
        let Some(mask) = &mut self.mask else { return };

        match mask.mask_type {
            MaskType::Linear => {
                let g = &mut mask.linear;
                let h = linear_handles(&self.press_mask.linear, &f);
                let (mut start, mut end) = (h.start, h.end);
                match self.drag {
                    Drag::Center => {
                        (g.x, g.y) = to_normalized(h.center + (p - p0));
                        return;
                    }
                    Drag::LinearStart => start = p,
                    Drag::LinearEnd => end = p,
                    Drag::Create => (start, end) = (p0, p),
                    _ => return,
                }
                // The gradient runs from start (full effect) to end (no effect).
                let d = start - end;
                if d.length() < 1.0 {
                    return;
                }
                (g.x, g.y) = to_normalized((start + end) / 2.0);
                g.angle = angle_of(d / d.length()) as f32;
                g.feather = (d.length() / f.long_edge) as f32;
            }
            MaskType::Radial => {
                let g = &mut mask.radial;
                let h = radial_handles(&self.press_mask.radial, &f);
                const MINIMUM: f32 = 0.005;
                let d = p - h.center;
                match self.drag {
                    Drag::Center => (g.x, g.y) = to_normalized(h.center + (p - p0)),
                    Drag::RadialWidth => {
                        g.width = MINIMUM.max((2.0 * (d.x * h.u.x + d.y * h.u.y).abs() / f.long_edge) as f32);
                    }
                    Drag::RadialHeight => {
                        g.height = MINIMUM.max((2.0 * (d.x * h.v.x + d.y * h.v.y).abs() / f.long_edge) as f32);
                    }
                    Drag::RadialRotation => {
                        if d.length() > 1.0 {
                            g.rotation = angle_of(d / d.length()) as f32;
                        }
                    }
                    Drag::Create => {
                        // Drag from the centre outwards.
                        (g.x, g.y) = to_normalized(p0);
                        g.width = MINIMUM.max((2.0 * (p.x - p0.x).abs() / f.long_edge) as f32);
                        g.height = MINIMUM.max((2.0 * (p.y - p0.y).abs() / f.long_edge) as f32);
                        g.rotation = 0.0;
                    }
                    _ => {}
                }
            }
            MaskType::Brush => {}
        }
    }

    pub fn release(&mut self) {
        self.drag = Drag::None;
    }

    /// Undo label of the current gesture, e.g. "Brush Stroke".
    pub fn gesture_label(&self) -> &'static str {
        match self.drag {
            Drag::Paint => "Brush Stroke",
            Drag::Create => "Draw Gradient",
            Drag::Center => "Move Mask",
            _ => "Reshape Mask",
        }
    }

    /// Draws the gradient guides and handles, or the brush outline at the cursor.
    pub fn paint(&self, painter: &Painter, m: &Mapping, cursor: Option<Pos2>) {
        let Some(mask) = &self.mask else { return };
        if m.is_empty() {
            return;
        }
        let f = Frame::new(m);
        let w = |image: Pt| m.to_screen(image);

        if self.tool == Tool::Brush {
            if let Some(cursor) = cursor {
                let r = (f64::from(self.brush.radius) * m.long_edge()) as f32;
                draw_outlined(painter, ellipse_points(cursor, r, r), false);
                let inner = r * (1.0 - self.brush.feather);
                if self.brush.feather > 0.02 && inner > 2.0 {
                    draw_outlined(painter, ellipse_points(cursor, inner, inner), true);
                }
            }
            return;
        }

        match mask.mask_type {
            MaskType::Linear => {
                let h = linear_handles(&mask.linear, &f);
                let reach = (f.w + f.h) * 2.0; // long enough to cross the photo
                let line = |through: Pt| vec![w(through - h.along * reach), w(through + h.along * reach)];
                let clipped = painter.with_clip_rect(m.clip.intersect(painter.clip_rect()));
                draw_outlined(&clipped, line(h.start), false);
                draw_outlined(&clipped, line(h.center), true);
                draw_outlined(&clipped, line(h.end), false);
                draw_handle(painter, w(h.center), true);
                draw_handle(painter, w(h.start), false);
                draw_handle(painter, w(h.end), false);
            }
            MaskType::Radial => {
                let h = radial_handles(&mask.radial, &f);
                let ellipse = |scale: f64| {
                    (0..=96)
                        .map(|i| {
                            let t = 2.0 * PI * f64::from(i) / 96.0;
                            w(h.center + h.u * (h.rx * scale * t.cos()) + h.v * (h.ry * scale * t.sin()))
                        })
                        .collect::<Vec<_>>()
                };
                draw_outlined(painter, ellipse(1.0), false);
                if mask.radial.feather > 0.02 {
                    draw_outlined(painter, ellipse(1.0 - f64::from(mask.radial.feather)), true);
                }
                let top = w(h.center - h.v * h.ry);
                let rotation = w(h.center - h.v * (h.ry + ROTATION_ARM / f64::from(m.scale)));
                draw_outlined(painter, vec![top, rotation], false);
                draw_handle(painter, w(h.center), true);
                for handle in
                    [h.center + h.u * h.rx, h.center - h.u * h.rx, h.center + h.v * h.ry, h.center - h.v * h.ry]
                {
                    draw_handle(painter, w(handle), false);
                }
                draw_handle(painter, rotation, true);
            }
            MaskType::Brush => {}
        }
    }
}

fn ellipse_points(center: Pos2, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..=96)
        .map(|i| {
            let t = std::f32::consts::TAU * i as f32 / 96.0;
            center + Vec2::new(rx * t.cos(), ry * t.sin())
        })
        .collect()
}

fn draw_handle(painter: &Painter, at: Pos2, filled: bool) {
    painter.circle_stroke(at, HANDLE_RADIUS, Stroke::new(3.0, Color32::from_black_alpha(160)));
    let fill = if filled { Color32::from_rgb(0x4c, 0x8d, 0xf6) } else { Color32::from_rgb(0x1e, 0x1f, 0x22) };
    painter.circle(at, HANDLE_RADIUS, fill, Stroke::new(1.5, Color32::WHITE));
}

/// A line or shape in white with a dark outline, visible on any photo.
fn draw_outlined(painter: &Painter, points: Vec<Pos2>, dashed: bool) {
    painter.add(Shape::line(points.clone(), Stroke::new(3.0, Color32::from_black_alpha(140))));
    let stroke = Stroke::new(1.2, Color32::from_white_alpha(230));
    if dashed {
        painter.extend(Shape::dashed_line(&points, stroke, 5.0, 4.0));
    } else {
        painter.add(Shape::line(points, stroke));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping() -> Mapping {
        // A 1000 x 500 photo drawn at half size, offset by (10, 20).
        let photo_to_screen = Affine::translate(10.0, 20.0) * Affine::scale(0.5, 0.5);
        Mapping { photo_to_screen, scale: 0.5, image_size: [1000, 500], clip: Rect::EVERYTHING }
    }

    fn screen(m: &Mapping, x: f64, y: f64) -> Pos2 {
        m.to_screen(Pt::new(x, y))
    }

    #[test]
    fn brush_strokes_record_spaced_points() {
        let m = mapping();
        let mut editor = MaskEditor::default();
        editor.set_mask(Some(&Mask::new(MaskType::Brush, &[])));
        editor.set_brush(Brush { radius: 0.02, ..Default::default() }); // 20 px
        assert!(editor.press(screen(&m, 100.0, 100.0), &m, false));
        assert_eq!(editor.gesture_label(), "Brush Stroke");
        assert!(!editor.drag_to(screen(&m, 102.0, 100.0), &m)); // closer than the spacing
        assert!(editor.drag_to(screen(&m, 200.0, 100.0), &m));
        editor.release();
        let stroke = &editor.mask().unwrap().strokes[0];
        assert_eq!(stroke.points.len(), 2);
        assert!((stroke.points[1].x - 0.2).abs() < 1e-6 && (stroke.points[1].y - 0.2).abs() < 1e-6);

        // Alt erases.
        assert!(editor.press(screen(&m, 100.0, 100.0), &m, true));
        assert_eq!(editor.mask().unwrap().strokes[1].mode, BrushMode::Erase);
    }

    #[test]
    fn dragging_out_a_linear_gradient() {
        let m = mapping();
        let mut editor = MaskEditor::default();
        editor.set_tool(Tool::Shape);
        let mut mask = Mask::new(MaskType::Linear, &[]);
        mask.linear = LinearGradient { x: 0.9, y: 0.9, angle: 0.0, feather: 0.01 };
        editor.set_mask(Some(&mask));
        // From (500, 100) (full effect) down to (500, 300) (no effect): effect above.
        assert!(editor.press(screen(&m, 500.0, 100.0), &m, false));
        assert_eq!(editor.gesture_label(), "Draw Gradient");
        assert!(editor.drag_to(screen(&m, 500.0, 300.0), &m));
        let g = editor.mask().unwrap().linear;
        assert!((g.x - 0.5).abs() < 1e-4 && (g.y - 0.4).abs() < 1e-4);
        assert!(g.angle.abs() < 1e-3);
        assert!((g.feather - 0.2).abs() < 1e-4);
    }

    #[test]
    fn radial_handles_reshape() {
        let m = mapping();
        let mut editor = MaskEditor::default();
        editor.set_tool(Tool::Shape);
        let mut mask = Mask::new(MaskType::Radial, &[]);
        mask.radial = RadialGradient { x: 0.5, y: 0.5, width: 0.4, height: 0.2, rotation: 0.0, feather: 0.5 };
        editor.set_mask(Some(&mask));
        // The right width handle sits at (500 + 200, 250).
        let handle = screen(&m, 700.0, 250.0);
        assert!(editor.is_over_handle(handle, &m));
        assert!(editor.press(handle, &m, false));
        assert_eq!(editor.gesture_label(), "Reshape Mask");
        assert!(editor.drag_to(screen(&m, 800.0, 250.0), &m));
        assert!((editor.mask().unwrap().radial.width - 0.6).abs() < 1e-4);
        editor.release();

        // Moving by the centre keeps the size.
        assert!(editor.press(screen(&m, 500.0, 250.0), &m, false));
        assert_eq!(editor.gesture_label(), "Move Mask");
        assert!(editor.drag_to(screen(&m, 600.0, 250.0), &m));
        let r = editor.mask().unwrap().radial;
        assert!((r.x - 0.6).abs() < 1e-4 && (r.width - 0.6).abs() < 1e-4);
    }

    #[test]
    fn painting_on_a_rotated_photo_lands_on_the_photo() {
        // A 1000 x 500 photo turned clockwise (a 500 x 1000 rendering), drawn 1:1 at (0, 0).
        let turned = iris_core::Crop { quarter_turns: 1, ..Default::default() }.geometry(1000, 500, false);
        let m = Mapping {
            photo_to_screen: turned.photo_to_result,
            scale: 1.0,
            image_size: [1000, 500],
            clip: Rect::EVERYTHING,
        };
        let mut editor = MaskEditor::default();
        editor.set_mask(Some(&Mask::new(MaskType::Brush, &[])));
        // The rendering's top-right corner is the photo's top-left corner.
        assert!(editor.press(Pos2::new(499.0, 1.0), &m, false));
        let p = editor.mask().unwrap().strokes[0].points[0];
        assert!(p.x < 0.01 && p.y < 0.01, "{p:?}");
        // And a screen point maps back where it came from.
        let image = m.to_image(Pos2::new(100.0, 700.0));
        let back = m.to_screen(image);
        assert!((back.x - 100.0).abs() < 1e-3 && (back.y - 700.0).abs() < 1e-3);
    }

    #[test]
    fn brush_masks_have_no_shape_to_drag() {
        let m = mapping();
        let mut editor = MaskEditor::default();
        editor.set_tool(Tool::Shape);
        editor.set_mask(Some(&Mask::new(MaskType::Brush, &[])));
        assert!(!editor.press(screen(&m, 100.0, 100.0), &m, false));
    }
}
