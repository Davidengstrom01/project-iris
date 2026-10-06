//! Area-averaging (box) downscaling in linear light.

use iris_core::ImageF;
use rayon::prelude::*;

struct Taps {
    first: usize,
    weights: Vec<f32>,
}

/// For each destination sample, the overlapping source samples and their coverage weights.
fn box_taps(source_length: usize, dest_length: usize) -> Vec<Taps> {
    let scale = source_length as f64 / dest_length as f64;
    (0..dest_length)
        .map(|d| {
            let start = d as f64 * scale;
            let end = (d + 1) as f64 * scale;
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(source_length) - 1;
            let coverage: Vec<f64> = (first..=last).map(|s| end.min(s as f64 + 1.0) - start.max(s as f64)).collect();
            let total: f64 = coverage.iter().sum();
            Taps { first, weights: coverage.iter().map(|&c| (c as f32 as f64 / total) as f32).collect() }
        })
        .collect()
}

/// Size that fits within `max_long_edge` while keeping the aspect ratio (never upscales;
/// 0 = no limit).
pub fn fit_size(width: usize, height: usize, max_long_edge: usize) -> (usize, usize) {
    let long_edge = width.max(height);
    if max_long_edge == 0 || long_edge <= max_long_edge {
        return (width, height);
    }
    let factor = max_long_edge as f64 / long_edge as f64;
    (((width as f64 * factor).round() as usize).max(1), ((height as f64 * factor).round() as usize).max(1))
}

/// Area-averaging downscale. Only shrinks; requested sizes larger than the source are
/// clamped to the source size.
pub fn resize_area(source: &ImageF, width: usize, height: usize) -> ImageF {
    let width = width.clamp(1, source.width);
    let height = height.clamp(1, source.height);
    if width == source.width && height == source.height {
        return source.clone();
    }

    let x_taps = box_taps(source.width, width);
    let y_taps = box_taps(source.height, height);

    // Horizontal pass: source.height rows of `width` pixels.
    let mut horizontal = ImageF::new(width, source.height);
    horizontal.pixels.par_chunks_mut(width * 3).enumerate().for_each(|(y, out)| {
        let input = source.row(y);
        for (x, t) in x_taps.iter().enumerate() {
            let (mut r, mut g, mut b) = (0.0f32, 0.0f32, 0.0f32);
            for (i, &w) in t.weights.iter().enumerate() {
                let px = &input[(t.first + i) * 3..];
                r += px[0] * w;
                g += px[1] * w;
                b += px[2] * w;
            }
            out[x * 3] = r;
            out[x * 3 + 1] = g;
            out[x * 3 + 2] = b;
        }
    });

    // Vertical pass.
    let mut result = ImageF::new(width, height);
    result.pixels.par_chunks_mut(width * 3).zip(&y_taps).for_each(|(out, t)| {
        for (i, &w) in t.weights.iter().enumerate() {
            for (o, &s) in out.iter_mut().zip(horizontal.row(t.first + i)) {
                *o += s * w;
            }
        }
    });
    result
}

/// Downscales so the long edge is at most `max_long_edge`.
pub fn downscale_to_fit(source: &ImageF, max_long_edge: usize) -> ImageF {
    let (w, h) = fit_size(source.width, source.height, max_long_edge);
    resize_area(source, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::solid;

    #[test]
    fn fit_size_never_upscales() {
        assert_eq!(fit_size(6000, 4000, 1500), (1500, 1000));
        assert_eq!(fit_size(4000, 6000, 1500), (1000, 1500));
        assert_eq!(fit_size(800, 600, 1500), (800, 600));
        assert_eq!(fit_size(800, 600, 0), (800, 600));
    }

    #[test]
    fn area_resize_averages_blocks() {
        let mut image = ImageF::new(4, 2);
        for y in 0..2 {
            for x in 0..4 {
                for c in 0..3 {
                    image.row_mut(y)[x * 3 + c] = if x < 2 { 0.0 } else { 1.0 };
                }
            }
        }
        let small = resize_area(&image, 2, 1);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.row(0)[0], 0.0);
        assert_eq!(small.row(0)[3], 1.0);

        let resized = resize_area(&solid(7, 5, 0.25, 0.5, 0.75), 3, 2);
        for px in resized.pixels.chunks(3) {
            assert!((px[0] - 0.25).abs() < 1e-5);
            assert!((px[2] - 0.75).abs() < 1e-5);
        }
    }
}
