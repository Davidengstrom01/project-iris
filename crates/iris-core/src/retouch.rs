//! Retouching: clone and heal strokes. Each stroke is stored as an operation (where it
//! copies from, the path painted, the brush) and applied when rendering, so it can be
//! undone and the RAW is never changed. Geometry is resolution-independent like masks:
//! positions are fractions of the photo's width and height, the radius a fraction of the
//! long edge. Strokes live in the coordinates of the uncropped, unrotated photo.

use crate::{MaskPoint, clean};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RetouchMode {
    /// Copies the source pixels.
    #[default]
    Clone,
    /// Copies the source's texture, keeping the destination's colour and light.
    Heal,
}

impl RetouchMode {
    /// "clone" / "heal" (file format keys)
    pub fn key(self) -> &'static str {
        match self {
            RetouchMode::Clone => "clone",
            RetouchMode::Heal => "heal",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [RetouchMode::Clone, RetouchMode::Heal].into_iter().find(|m| m.key() == key)
    }

    pub fn name(self) -> &'static str {
        match self {
            RetouchMode::Clone => "Clone",
            RetouchMode::Heal => "Heal",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetouchStroke {
    pub mode: RetouchMode,
    /// Where the source is relative to the destination, as fractions of the photo's width
    /// and height (source = destination + offset).
    pub offset: MaskPoint,
    /// Fraction of the long edge.
    pub radius: f32,
    /// 0 (soft from the centre) .. 1 (hard edge).
    pub hardness: f32,
    /// 0..1: how much of the result replaces the photo at most.
    pub opacity: f32,
    /// 0..1: how much each dab adds; below 1 the effect builds up along the stroke.
    pub flow: f32,
    /// The destination path.
    pub points: Vec<MaskPoint>,
}

impl Default for RetouchStroke {
    fn default() -> Self {
        Self {
            mode: RetouchMode::Clone,
            offset: MaskPoint::new(0.05, 0.0),
            radius: 0.02,
            hardness: 0.5,
            opacity: 1.0,
            flow: 1.0,
            points: Vec::new(),
        }
    }
}

pub const MAX_RETOUCH_STROKES: usize = 2000;
pub const MAX_RETOUCH_POINTS: usize = 20000;

impl RetouchStroke {
    /// Clamps values into range (e.g. after reading a file).
    pub fn sanitized(mut self) -> Self {
        self.offset.x = clean(self.offset.x, -2.0, 2.0, 0.0);
        self.offset.y = clean(self.offset.y, -2.0, 2.0, 0.0);
        self.radius = clean(self.radius, crate::mask::MIN_BRUSH_RADIUS, crate::mask::MAX_BRUSH_RADIUS, 0.02);
        self.hardness = clean(self.hardness, 0.0, 1.0, 0.5);
        self.opacity = clean(self.opacity, 0.0, 1.0, 1.0);
        self.flow = clean(self.flow, 0.01, 1.0, 1.0);
        self.points.truncate(MAX_RETOUCH_POINTS);
        for p in &mut self.points {
            p.x = clean(p.x, -1.0, 2.0, 0.0);
            p.y = clean(p.y, -1.0, 2.0, 0.0);
        }
        self
    }
}

/// Sanitizes a list of strokes, dropping empty ones.
pub fn sanitized_strokes(mut strokes: Vec<RetouchStroke>) -> Vec<RetouchStroke> {
    strokes.truncate(MAX_RETOUCH_STROKES);
    strokes.into_iter().filter(|s| !s.points.is_empty()).map(RetouchStroke::sanitized).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strokes_are_sanitized() {
        let strokes = vec![
            RetouchStroke { points: vec![], ..Default::default() },
            RetouchStroke {
                radius: 9.0,
                flow: 0.0,
                hardness: f32::NAN,
                points: vec![MaskPoint::new(0.5, 0.5)],
                ..Default::default()
            },
        ];
        let s = sanitized_strokes(strokes);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].radius, 0.5);
        assert_eq!(s[0].flow, 0.01);
        assert_eq!(s[0].hardness, 0.5);
        assert_eq!(RetouchMode::from_key("heal"), Some(RetouchMode::Heal));
    }
}
