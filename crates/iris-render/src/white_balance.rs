//! Auto white balance and the white-balance eyedropper.

use iris_core::color::white_balance_for_neutral;
use iris_core::{ImageF, WhiteBalance};

const CLIP_LEVEL: f32 = 0.97;
const MIN_LEVEL: f32 = 0.002;

fn usable(px: &[f32]) -> bool {
    let hi = px[0].max(px[1]).max(px[2]);
    let lo = px[0].min(px[1]).min(px[2]);
    hi < CLIP_LEVEL && lo > MIN_LEVEL
}

/// Auto white balance: grey-world estimate over well-exposed pixels of a source image
/// decoded with the as-shot white balance.
pub fn estimate_white_balance(source: &ImageF, as_shot: &WhiteBalance) -> WhiteBalance {
    let mut sum = [0.0f64; 3];
    let mut count = 0usize;
    for px in source.pixels.as_chunks::<3>().0.iter().filter(|px| usable(px.as_slice())) {
        for c in 0..3 {
            sum[c] += f64::from(px[c]);
        }
        count += 1;
    }
    if count == 0 {
        return *as_shot;
    }
    white_balance_for_neutral(&sum.map(|s| s / count as f64), as_shot)
}

/// Eyedropper: the white balance that makes the area around (x, y) neutral. x and y are
/// normalised [0, 1] image coordinates. Returns `None` if the area is clipped or black.
pub fn sample_white_balance(source: &ImageF, x: f64, y: f64, as_shot: &WhiteBalance) -> Option<WhiteBalance> {
    if source.is_empty() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return None;
    }
    // Average a small area (about 0.3% of the long edge) to reduce noise.
    let radius = (source.width.max(source.height) / 300).max(1);
    let cx = ((x * source.width as f64) as usize).min(source.width - 1);
    let cy = ((y * source.height as f64) as usize).min(source.height - 1);
    let mut sum = [0.0f64; 3];
    let mut count = 0usize;
    for py in cy.saturating_sub(radius)..=(cy + radius).min(source.height - 1) {
        let row = source.row(py);
        for px in cx.saturating_sub(radius)..=(cx + radius).min(source.width - 1) {
            let p = &row[px * 3..px * 3 + 3];
            if !usable(p) {
                return None;
            }
            for c in 0..3 {
                sum[c] += f64::from(p[c]);
            }
            count += 1;
        }
    }
    Some(white_balance_for_neutral(&sum.map(|s| s / count as f64), as_shot))
}
