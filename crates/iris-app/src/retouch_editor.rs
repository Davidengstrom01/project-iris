//! Clone and heal on the photo: Alt-click chooses where to copy from, then painting copies
//! (or heals) from there. With *Aligned* the source keeps its distance from the brush
//! across strokes; otherwise every stroke starts copying from the chosen source point again.

use egui::{Color32, Painter, Pos2, Stroke, Vec2};
use iris_core::retouch::MAX_RETOUCH_POINTS;
use iris_core::{MaskPoint, RetouchMode, RetouchStroke};

use crate::mask_editor::{Mapping, draw_outlined, ellipse_points};

/// Of the brush radius, between recorded points.
const STROKE_SPACING: f64 = 0.25;

/// The brush settings (from the Retouch panel).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RetouchBrush {
    pub mode: RetouchMode,
    /// Fraction of the long edge.
    pub radius: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub flow: f32,
    pub aligned: bool,
}

impl Default for RetouchBrush {
    fn default() -> Self {
        Self { mode: RetouchMode::Clone, radius: 0.02, hardness: 0.5, opacity: 1.0, flow: 1.0, aligned: true }
    }
}

/// What a press did.
#[derive(Debug, PartialEq)]
pub enum Press {
    /// Alt-click: the source point was set.
    SourceSet,
    /// Painting needs a source first.
    NeedsSource,
    /// A stroke started (the first dab).
    Started(RetouchStroke),
}

#[derive(Default)]
pub struct RetouchEditor {
    brush: RetouchBrush,
    /// Where copying starts from (photo fractions).
    source: Option<MaskPoint>,
    /// With Aligned: the offset all strokes share once the first one has started.
    aligned_offset: Option<MaskPoint>,
    /// The stroke being painted.
    stroke: Option<RetouchStroke>,
}

impl RetouchEditor {
    pub fn set_brush(&mut self, brush: RetouchBrush) {
        if brush.aligned != self.brush.aligned {
            self.aligned_offset = None;
        }
        self.brush = brush;
    }

    pub fn has_source(&self) -> bool {
        self.source.is_some()
    }

    pub fn is_painting(&self) -> bool {
        self.stroke.is_some()
    }

    /// Forgets the source (e.g. for another photo).
    pub fn reset(&mut self) {
        self.source = None;
        self.aligned_offset = None;
        self.stroke = None;
    }

    /// The offset a stroke starting at `dest` would use.
    fn offset_for(&self, dest: MaskPoint) -> Option<MaskPoint> {
        let source = self.source?;
        match (self.brush.aligned, self.aligned_offset) {
            (true, Some(offset)) => Some(offset),
            _ => Some(MaskPoint::new(source.x - dest.x, source.y - dest.y)),
        }
    }

    pub fn press(&mut self, pos: Pos2, m: &Mapping, alt: bool) -> Press {
        let at = m.to_mask(pos);
        if alt {
            self.source = Some(at);
            self.aligned_offset = None;
            return Press::SourceSet;
        }
        let Some(offset) = self.offset_for(at) else { return Press::NeedsSource };
        if self.brush.aligned {
            self.aligned_offset = Some(offset);
        }
        let b = self.brush;
        let stroke = RetouchStroke {
            mode: b.mode,
            offset,
            radius: b.radius,
            hardness: b.hardness,
            opacity: b.opacity,
            flow: b.flow,
            points: vec![at],
        };
        self.stroke = Some(stroke.clone());
        Press::Started(stroke)
    }

    /// Continues the stroke; returns it when a point was added.
    pub fn drag_to(&mut self, pos: Pos2, m: &Mapping) -> Option<RetouchStroke> {
        let stroke = self.stroke.as_mut()?;
        let p = m.to_mask(pos);
        let last = *stroke.points.last()?;
        let [w, h] = m.image_size.map(|v| v as f64);
        let distance = (f64::from(p.x - last.x) * w).hypot(f64::from(p.y - last.y) * h) / w.max(h);
        let minimum = (f64::from(stroke.radius) * STROKE_SPACING).max(1.5 / m.long_edge());
        if distance < minimum || stroke.points.len() >= MAX_RETOUCH_POINTS {
            return None;
        }
        stroke.points.push(p);
        Some(stroke.clone())
    }

    pub fn release(&mut self) {
        self.stroke = None;
    }

    /// Where the brush would copy from with the cursor at `cursor` (photo fractions).
    pub fn source_for(&self, cursor: Pos2, m: &Mapping) -> Option<MaskPoint> {
        let dest = match &self.stroke {
            Some(stroke) => *stroke.points.last()?,
            None => m.to_mask(cursor),
        };
        let offset = match &self.stroke {
            Some(stroke) => stroke.offset,
            None => self.offset_for(dest)?,
        };
        Some(MaskPoint::new(dest.x + offset.x, dest.y + offset.y))
    }

    /// Screen radius of the brush.
    pub fn screen_radius(&self, m: &Mapping) -> f32 {
        (f64::from(self.brush.radius) * m.long_edge()) as f32
    }

    /// The brush outline at the cursor, and a crosshair where it copies from.
    pub fn paint(&self, painter: &Painter, m: &Mapping, cursor: Option<Pos2>) {
        let r = self.screen_radius(m);
        if let Some(cursor) = cursor {
            draw_outlined(painter, ellipse_points(cursor, r, r), false);
            let inner = r * self.brush.hardness;
            if self.brush.hardness < 0.98 && inner > 2.0 {
                draw_outlined(painter, ellipse_points(cursor, inner, inner), true);
            }
        }
        let source = match cursor {
            Some(c) => self.source_for(c, m),
            None => self.source,
        };
        if let Some(source) = source {
            let at = m.mask_to_screen(source);
            draw_outlined(painter, ellipse_points(at, r, r), true);
            let arm = (r * 0.5).clamp(5.0, 12.0);
            for (a, b) in [(Vec2::new(-arm, 0.0), Vec2::new(arm, 0.0)), (Vec2::new(0.0, -arm), Vec2::new(0.0, arm))] {
                painter.line_segment([at + a, at + b], Stroke::new(3.0, Color32::from_black_alpha(140)));
                painter.line_segment([at + a, at + b], Stroke::new(1.2, Color32::WHITE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::crop::Affine;

    fn mapping() -> Mapping {
        // A 1000 x 500 photo drawn 1:1 at the origin.
        Mapping { photo_to_screen: Affine::IDENTITY, scale: 1.0, image_size: [1000, 500], clip: egui::Rect::EVERYTHING }
    }

    fn at(x: f32, y: f32) -> Pos2 {
        Pos2::new(x, y)
    }

    #[test]
    fn needs_a_source_first() {
        let m = mapping();
        let mut editor = RetouchEditor::default();
        assert_eq!(editor.press(at(500.0, 250.0), &m, false), Press::NeedsSource);
        assert_eq!(editor.press(at(100.0, 250.0), &m, true), Press::SourceSet);
        let Press::Started(stroke) = editor.press(at(500.0, 250.0), &m, false) else { panic!() };
        assert!((stroke.offset.x + 0.4).abs() < 1e-6 && stroke.offset.y.abs() < 1e-6);
        assert!(editor.drag_to(at(501.0, 250.0), &m).is_none()); // too close
        let stroke = editor.drag_to(at(600.0, 250.0), &m).unwrap();
        assert_eq!(stroke.points.len(), 2);
        // While painting, the source follows the brush.
        let source = editor.source_for(at(600.0, 250.0), &m).unwrap();
        assert!((source.x - 0.2).abs() < 1e-6);
    }

    #[test]
    fn aligned_keeps_the_offset_fixed_restarts_from_the_source() {
        let m = mapping();
        let mut editor = RetouchEditor::default();
        editor.press(at(100.0, 100.0), &m, true);
        editor.press(at(300.0, 100.0), &m, false); // offset -0.2
        editor.release();
        let Press::Started(second) = editor.press(at(700.0, 300.0), &m, false) else { panic!() };
        assert!((second.offset.x + 0.2).abs() < 1e-6 && second.offset.y.abs() < 1e-6); // aligned
        editor.release();

        editor.set_brush(RetouchBrush { aligned: false, ..Default::default() });
        let Press::Started(fixed) = editor.press(at(700.0, 300.0), &m, false) else { panic!() };
        // Fixed: copies from the source point again.
        assert!((fixed.offset.x - (0.1 - 0.7)).abs() < 1e-6 && (fixed.offset.y - (0.2 - 0.6)).abs() < 1e-6);
    }
}
