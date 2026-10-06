use crate::WhiteBalance;

/// Shooting information extracted from a RAW file. Zero / empty means "unknown".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhotoMetadata {
    pub make: String,
    pub model: String,
    pub lens: String,
    pub iso: f32,
    pub shutter_seconds: f32,
    /// f-number
    pub aperture: f32,
    pub focal_length_mm: f32,
    /// Seconds since the epoch.
    pub timestamp: i64,
    /// EXIF orientation (1 = normal, 3 = 180°, 6 = 90° CW, 8 = 90° CCW).
    pub orientation: i32,
    /// Full-resolution size after orientation is applied.
    pub width: usize,
    pub height: usize,
    /// White balance the decoded image is rendered with.
    pub as_shot: WhiteBalance,
}
