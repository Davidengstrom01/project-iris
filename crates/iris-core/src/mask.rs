//! Local adjustments: a mask selects part of the photo and a few develop adjustments are
//! applied there. Geometry is resolution-independent so the preview and the export agree:
//!   positions are fractions of the image width (x) and height (y), 0..1;
//!   lengths (brush radius, gradient feather, ellipse size) are fractions of the long edge.
//! Masks live in the coordinates of the uncropped, unrotated photo.

use crate::clean;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskType {
    #[default]
    Brush,
    Linear,
    Radial,
}

impl MaskType {
    /// "brush" / "linear" / "radial" (file format keys)
    pub fn key(self) -> &'static str {
        match self {
            MaskType::Brush => "brush",
            MaskType::Linear => "linear",
            MaskType::Radial => "radial",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [MaskType::Brush, MaskType::Linear, MaskType::Radial].into_iter().find(|t| t.key() == key)
    }

    /// "Brush", "Linear Gradient", "Radial Gradient"
    pub fn name(self) -> &'static str {
        match self {
            MaskType::Brush => "Brush",
            MaskType::Linear => "Linear Gradient",
            MaskType::Radial => "Radial Gradient",
        }
    }
}

/// How a brush stroke changes the mask:
///   Add       paints the mask in
///   Subtract  removes the mask where painted, including any gradient underneath
///   Erase     removes earlier brush strokes (both Add and Subtract) where painted
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrushMode {
    #[default]
    Add,
    Subtract,
    Erase,
}

impl BrushMode {
    /// "add" / "subtract" / "erase" (file format keys)
    pub fn key(self) -> &'static str {
        match self {
            BrushMode::Add => "add",
            BrushMode::Subtract => "subtract",
            BrushMode::Erase => "erase",
        }
    }

    /// Unknown keys are Add.
    pub fn from_key(key: &str) -> Self {
        match key {
            "subtract" => BrushMode::Subtract,
            "erase" => BrushMode::Erase,
            _ => BrushMode::Add,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MaskPoint {
    pub x: f32,
    pub y: f32,
}

impl MaskPoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrushStroke {
    pub mode: BrushMode,
    /// Fraction of the long edge.
    pub radius: f32,
    /// Soft part of the radius, 0 (hard) .. 1 (soft from the centre).
    pub feather: f32,
    /// 0..1
    pub opacity: f32,
    pub points: Vec<MaskPoint>,
}

impl Default for BrushStroke {
    fn default() -> Self {
        Self { mode: BrushMode::Add, radius: 0.05, feather: 0.5, opacity: 1.0, points: Vec::new() }
    }
}

/// Full effect on one side of a line, fading out across `feather`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearGradient {
    /// Centre of the transition.
    pub x: f32,
    pub y: f32,
    /// Degrees; 0 = horizontal with the effect above, counter-clockwise.
    pub angle: f32,
    /// Width of the transition, fraction of the long edge.
    pub feather: f32,
}

impl Default for LinearGradient {
    fn default() -> Self {
        Self { x: 0.5, y: 0.4, angle: 0.0, feather: 0.3 }
    }
}

/// Full effect inside an ellipse, fading out towards its edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadialGradient {
    pub x: f32,
    pub y: f32,
    /// Diameters, fractions of the long edge.
    pub width: f32,
    pub height: f32,
    /// Degrees, counter-clockwise.
    pub rotation: f32,
    /// Soft part of the radius, 0..1.
    pub feather: f32,
}

impl Default for RadialGradient {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5, width: 0.4, height: 0.3, rotation: 0.0, feather: 0.5 }
    }
}

/// The adjustments a mask applies. They act like the global sliders, relative to them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocalAdjustments {
    /// EV, -4..+4
    pub exposure: f32,
    /// -100..+100 (and the same for the rest)
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub saturation: f32,
    /// Relative: + warmer.
    pub temperature: f32,
}

impl LocalAdjustments {
    pub fn is_neutral(&self) -> bool {
        *self == Self::default()
    }
}

/// One mask: a shape (none for a brush mask), brush strokes that refine it, and the
/// adjustments applied through it. Invert flips the shape and Add / Erase strokes; Subtract
/// strokes always remove the effect.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mask {
    pub mask_type: MaskType,
    pub name: String,
    pub invert: bool,
    /// Used when the type is Linear.
    pub linear: LinearGradient,
    /// Used when the type is Radial.
    pub radial: RadialGradient,
    pub strokes: Vec<BrushStroke>,
    pub adjustments: LocalAdjustments,
}

pub const MAX_MASKS: usize = 32;
pub const MAX_STROKES_PER_MASK: usize = 2000;
pub const MAX_POINTS_PER_STROKE: usize = 20000;
pub const MIN_BRUSH_RADIUS: f32 = 0.001;
pub const MAX_BRUSH_RADIUS: f32 = 0.5;

/// Describes one local adjustment (shared by persistence and the UI).
pub struct LocalAdjustmentField {
    pub key: &'static str,
    pub label: &'static str,
    pub minimum: f32,
    pub maximum: f32,
    pub value: fn(&mut LocalAdjustments) -> &mut f32,
}

impl LocalAdjustmentField {
    pub fn get(&self, a: &LocalAdjustments) -> f32 {
        let mut copy = *a;
        *(self.value)(&mut copy)
    }
}

pub static LOCAL_ADJUSTMENT_FIELDS: [LocalAdjustmentField; 6] = [
    LocalAdjustmentField {
        key: "exposure",
        label: "Exposure",
        minimum: -4.0,
        maximum: 4.0,
        value: |a| &mut a.exposure,
    },
    LocalAdjustmentField {
        key: "contrast",
        label: "Contrast",
        minimum: -100.0,
        maximum: 100.0,
        value: |a| &mut a.contrast,
    },
    LocalAdjustmentField {
        key: "highlights",
        label: "Highlights",
        minimum: -100.0,
        maximum: 100.0,
        value: |a| &mut a.highlights,
    },
    LocalAdjustmentField {
        key: "shadows",
        label: "Shadows",
        minimum: -100.0,
        maximum: 100.0,
        value: |a| &mut a.shadows,
    },
    LocalAdjustmentField {
        key: "saturation",
        label: "Saturation",
        minimum: -100.0,
        maximum: 100.0,
        value: |a| &mut a.saturation,
    },
    LocalAdjustmentField {
        key: "temperature",
        label: "Temperature",
        minimum: -100.0,
        maximum: 100.0,
        value: |a| &mut a.temperature,
    },
];

/// Angles are kept in (-180, 180].
fn normalized_angle(degrees: f32) -> f32 {
    if !degrees.is_finite() {
        return 0.0;
    }
    let d = degrees % 360.0; // same sign as the input, like fmod
    if d > 180.0 {
        d - 360.0
    } else if d <= -180.0 {
        d + 360.0
    } else {
        d
    }
}

impl Mask {
    /// A new mask with a default shape and a name such as "Radial 2" that is not yet used.
    pub fn new(mask_type: MaskType, existing: &[Mask]) -> Self {
        let base = match mask_type {
            MaskType::Brush => "Brush",
            MaskType::Linear => "Linear",
            MaskType::Radial => "Radial",
        };
        let name = (1..)
            .map(|n| format!("{base} {n}"))
            .find(|name| existing.iter().all(|m| &m.name != name))
            .expect("unbounded range");
        Self { mask_type, name, ..Default::default() }
    }

    /// Clamps every value into its valid range and drops unusable strokes (e.g. after
    /// reading a file).
    pub fn sanitized(mut self) -> Self {
        let l = &mut self.linear;
        l.x = clean(l.x, -1.0, 2.0, 0.5);
        l.y = clean(l.y, -1.0, 2.0, 0.5);
        l.angle = normalized_angle(l.angle);
        l.feather = clean(l.feather, 0.0, 2.0, 0.3);

        let r = &mut self.radial;
        r.x = clean(r.x, -1.0, 2.0, 0.5);
        r.y = clean(r.y, -1.0, 2.0, 0.5);
        r.width = clean(r.width, 0.002, 4.0, 0.4);
        r.height = clean(r.height, 0.002, 4.0, 0.3);
        r.rotation = normalized_angle(r.rotation);
        r.feather = clean(r.feather, 0.0, 1.0, 0.5);

        self.strokes.truncate(MAX_STROKES_PER_MASK);
        self.strokes.retain(|s| !s.points.is_empty());
        for s in &mut self.strokes {
            s.radius = clean(s.radius, MIN_BRUSH_RADIUS, MAX_BRUSH_RADIUS, 0.05);
            s.feather = clean(s.feather, 0.0, 1.0, 0.5);
            s.opacity = clean(s.opacity, 0.0, 1.0, 1.0);
            s.points.truncate(MAX_POINTS_PER_STROKE);
            for p in &mut s.points {
                p.x = clean(p.x, -1.0, 2.0, 0.0);
                p.y = clean(p.y, -1.0, 2.0, 0.0);
            }
        }

        for field in &LOCAL_ADJUSTMENT_FIELDS {
            let v = (field.value)(&mut self.adjustments);
            *v = clean(*v, field.minimum, field.maximum, 0.0);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_masks_get_unique_names() {
        let mut masks = vec![Mask::new(MaskType::Brush, &[])];
        masks.push(Mask::new(MaskType::Brush, &masks));
        masks.push(Mask::new(MaskType::Linear, &masks));
        assert_eq!(masks[1].name, "Brush 2");
        assert_eq!(masks[2].name, "Linear 1");
        masks.remove(0);
        assert_eq!(Mask::new(MaskType::Brush, &masks).name, "Brush 1");
    }

    #[test]
    fn angles_are_normalised() {
        assert_eq!(normalized_angle(450.0), 90.0);
        assert_eq!(normalized_angle(-180.0), 180.0);
        assert_eq!(normalized_angle(190.0), -170.0);
        assert_eq!(normalized_angle(f32::NAN), 0.0);
    }
}
