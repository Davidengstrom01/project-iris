//! Edit state, masks, crop geometry, images and colour science. No I/O and no
//! dependencies, so every other crate (and every front end) can build on it.

pub mod color;
pub mod crop;
pub mod curve;
pub mod detail;
pub mod edit_state;
pub mod history;
pub mod hsl;
pub mod image;
pub mod mask;
pub mod metadata;

pub use crop::Crop;
pub use curve::{CurvePoint, CurvePoints, CurveSpline, ToneCurve};
pub use detail::{Detail, NoiseReduction, Sharpening};
pub use edit_state::{AdjustmentField, BasicAdjustments, EditState, WhiteBalance};
pub use history::EditHistory;
pub use hsl::{HslAdjustments, HslBand, HslColor};
pub use image::{EncodedImage, ImageF, Samples};
pub use mask::{BrushMode, BrushStroke, LinearGradient, LocalAdjustments, Mask, MaskPoint, MaskType, RadialGradient};
pub use metadata::PhotoMetadata;

/// Clamps `v` into `[lo, hi]`, or returns `fallback` if it is NaN or infinite.
pub(crate) fn clean(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { fallback }
}
