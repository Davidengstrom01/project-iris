//! Detail: sharpening and noise reduction. Lengths are in full-resolution pixels, so a
//! preview at a lower resolution shows them scaled down (judge them at 100%).

use crate::clean;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sharpening {
    /// 0..150
    pub amount: f32,
    /// Size of the details sharpened, in pixels, 0.5..3.
    pub radius: f32,
    /// 0..100: higher values sharpen only edges and leave smooth areas (and their noise)
    /// alone.
    pub masking: f32,
}

impl Default for Sharpening {
    fn default() -> Self {
        Self { amount: 0.0, radius: 1.0, masking: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NoiseReduction {
    /// 0..100: smooths grain in brightness, keeping edges.
    pub luminance: f32,
    /// 0..100: removes colour blotches.
    pub color: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Detail {
    pub sharpening: Sharpening,
    pub noise_reduction: NoiseReduction,
}

pub const MAX_SHARPEN_AMOUNT: f32 = 150.0;
pub const MIN_SHARPEN_RADIUS: f32 = 0.5;
pub const MAX_SHARPEN_RADIUS: f32 = 3.0;

impl Detail {
    /// True when the detail stage changes nothing.
    pub fn is_neutral(&self) -> bool {
        self.sharpening.amount == 0.0 && self.noise_reduction.luminance == 0.0 && self.noise_reduction.color == 0.0
    }

    /// Clamps every value into its range (e.g. after reading a file).
    pub fn sanitized(mut self) -> Self {
        let s = &mut self.sharpening;
        s.amount = clean(s.amount, 0.0, MAX_SHARPEN_AMOUNT, 0.0);
        s.radius = clean(s.radius, MIN_SHARPEN_RADIUS, MAX_SHARPEN_RADIUS, 1.0);
        s.masking = clean(s.masking, 0.0, 100.0, 0.0);
        let n = &mut self.noise_reduction;
        n.luminance = clean(n.luminance, 0.0, 100.0, 0.0);
        n.color = clean(n.color, 0.0, 100.0, 0.0);
        self
    }

    /// Undo label for a change from `before` to `self`.
    pub fn describe_change(&self, before: &Detail) -> &'static str {
        if self.sharpening != before.sharpening && self.noise_reduction == before.noise_reduction {
            "Sharpening"
        } else if self.noise_reduction != before.noise_reduction && self.sharpening == before.sharpening {
            "Noise Reduction"
        } else {
            "Detail"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_neutral_and_values_are_clamped() {
        assert!(Detail::default().is_neutral());
        let d = Detail {
            sharpening: Sharpening { amount: 400.0, radius: 0.0, masking: f32::NAN },
            noise_reduction: NoiseReduction { luminance: -5.0, color: 30.0 },
        }
        .sanitized();
        assert_eq!(d.sharpening.amount, 150.0);
        assert_eq!(d.sharpening.radius, 0.5);
        assert_eq!(d.sharpening.masking, 0.0);
        assert_eq!(d.noise_reduction.luminance, 0.0);
        assert!(!d.is_neutral());
        let mut sharper = Detail::default();
        sharper.sharpening.amount = 40.0;
        assert_eq!(sharper.describe_change(&Detail::default()), "Sharpening");
    }
}
