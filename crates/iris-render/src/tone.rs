use iris_core::{BasicAdjustments, CurveSpline, ImageF, ToneCurve};
use rayon::prelude::*;

use crate::resample::downscale_to_fit;

const GAMMA: f64 = 2.2;
/// 2 EV above sensor white; brighter values clip anyway.
const MAX_INPUT: f32 = 4.0;
const TABLE_SIZE: usize = 1 << 16;

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The global tone mapping as one lookup table from scene-linear values to display-linear
/// [0, 1]: Contrast, Whites and Blacks, followed by the user's RGB tone curve. Both are
/// shaped in a gamma 2.2 perceptual space. With everything neutral it is the identity,
/// clipped at 1.
pub struct ToneLut {
    table: Vec<f32>,
    scale: f32,
}

impl ToneLut {
    pub fn new(adjustments: &BasicAdjustments, curve: &ToneCurve) -> Self {
        let scale = TABLE_SIZE as f32 / MAX_INPUT;
        let basic_identity = adjustments.whites == 0.0 && adjustments.blacks == 0.0 && adjustments.contrast == 0.0;
        let curve_identity = curve.is_identity();
        let spline = CurveSpline::new(&curve.rgb);
        let table = (0..=TABLE_SIZE)
            .into_par_iter()
            .map(|i| {
                let v = i as f64 / f64::from(scale);
                let mut out = if basic_identity { v.min(1.0) } else { Self::evaluate_basic(v, adjustments) };
                if !curve_identity {
                    out = f64::from(spline.eval(out.powf(1.0 / GAMMA) as f32)).powf(GAMMA);
                }
                out as f32
            })
            .collect();
        Self { table, scale }
    }

    pub fn eval(&self, v: f32) -> f32 {
        let position = v.max(0.0) * self.scale;
        if position >= (self.table.len() - 1) as f32 {
            return self.table[self.table.len() - 1];
        }
        let i = position as usize;
        let f = position - i as f32;
        self.table[i] + (self.table[i + 1] - self.table[i]) * f
    }

    /// Applies the curve to the largest and smallest channel and interpolates the middle
    /// one, which keeps hue stable (the technique used by Adobe's DNG reference renderer).
    pub fn apply_hue_preserving(&self, rgb: &mut [f32]) {
        let hi = rgb[0].max(rgb[1]).max(rgb[2]);
        let lo = rgb[0].min(rgb[1]).min(rgb[2]);
        let hi_out = self.eval(hi);
        if hi - lo < 1e-9 {
            rgb[..3].fill(hi_out);
            return;
        }
        let lo_out = self.eval(lo);
        let k = (hi_out - lo_out) / (hi - lo);
        for c in &mut rgb[..3] {
            *c = lo_out + (*c - lo) * k;
        }
    }

    /// Contrast / Whites / Blacks evaluated directly (no table, no tone curve).
    pub fn evaluate_basic(v: f64, a: &BasicAdjustments) -> f64 {
        let mut p = v.max(0.0).powf(1.0 / GAMMA);

        // Whites: move the white point; only the upper part of the range is affected.
        let white_point = (-f64::from(a.whites) / 100.0).exp2().powf(1.0 / GAMMA);
        let t = smoothstep(0.25, 1.0, p);
        p *= 1.0 - t + t / white_point;

        // Blacks: lift or crush the black point; only the lower part is affected.
        if p < 0.5 {
            p += f64::from(a.blacks) / 100.0 * 0.1 * (1.0 - p / 0.5).powf(2.0);
        }

        // Contrast: a power curve on each side of middle grey, meeting with equal slope.
        p = p.clamp(0.0, 1.0);
        let exponent = 1.0 + f64::from(a.contrast) / 100.0 * 0.6;
        let pivot = 0.18f64.powf(1.0 / GAMMA);
        p = if p < pivot {
            pivot * (p / pivot).powf(exponent)
        } else {
            1.0 - (1.0 - pivot) * ((1.0 - p) / (1.0 - pivot)).powf(exponent)
        };

        p.clamp(0.0, 1.0).powf(GAMMA)
    }
}

/// Resolution the base layer is computed at.
const BASE_LONG_EDGE: usize = 1024;
/// Filter radius as a fraction of the long edge.
const BASE_RADIUS: f64 = 0.025;
/// Edge threshold, in (log2 units)^2.
const BASE_EPSILON: f32 = 0.25;
const MIN_LUMINANCE: f32 = 1.0 / 65536.0;

/// Mean over a (2r+1)^2 window, shrinking the window at the borders.
fn box_mean(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0.0f32; src.len()];
    tmp.par_chunks_mut(w).zip(src.par_chunks(w)).for_each(|(o, input)| {
        let mut sum: f64 = input[..=r.min(w - 1)].iter().map(|&v| f64::from(v)).sum();
        for x in 0..w {
            let lo = x.saturating_sub(r);
            let hi = (x + r).min(w - 1);
            o[x] = (sum / (hi - lo + 1) as f64) as f32;
            if x + r + 1 < w {
                sum += f64::from(input[x + r + 1]);
            }
            if x >= r {
                sum -= f64::from(input[x - r]);
            }
        }
    });

    // Columns, processed in parallel blocks of columns to keep memory access row-wise.
    let mut out = vec![0.0f32; src.len()];
    let columns: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let mut column = vec![0.0f32; h];
            let mut sum: f64 = (0..=r.min(h - 1)).map(|y| f64::from(tmp[y * w + x])).sum();
            for y in 0..h {
                let lo = y.saturating_sub(r);
                let hi = (y + r).min(h - 1);
                column[y] = (sum / (hi - lo + 1) as f64) as f32;
                if y + r + 1 < h {
                    sum += f64::from(tmp[(y + r + 1) * w + x]);
                }
                if y >= r {
                    sum -= f64::from(tmp[(y - r) * w + x]);
                }
            }
            column
        })
        .collect();
    for (x, column) in columns.iter().enumerate() {
        for (y, &v) in column.iter().enumerate() {
            out[y * w + x] = v;
        }
    }
    out
}

#[derive(Clone, Copy)]
struct Tap {
    i0: usize,
    i1: usize,
    f: f32,
}

/// Bilinear lookup from source pixels into a smaller grid (pixel-centre aligned).
fn taps(source_length: usize, grid_length: usize) -> Vec<Tap> {
    let scale = grid_length as f64 / source_length as f64;
    (0..source_length)
        .map(|i| {
            let pos = ((i as f64 + 0.5) * scale - 0.5).clamp(0.0, (grid_length - 1) as f64);
            let i0 = pos as usize;
            Tap { i0, i1: (i0 + 1).min(grid_length - 1), f: (pos - i0 as f64) as f32 }
        })
        .collect()
}

/// Edge-aware, low-frequency log-luminance of an image, used to steer Highlights and
/// Shadows so that they act on regions rather than on individual pixels (preserving
/// local detail without halos). It is computed by a guided filter at a fixed small
/// resolution, so the preview and the full-resolution export get the same result.
pub struct ToneBaseLayer {
    width: usize,
    /// Guided filter coefficients (smoothed).
    a: Vec<f32>,
    b: Vec<f32>,
    x_taps: Vec<Tap>,
    y_taps: Vec<Tap>,
}

impl ToneBaseLayer {
    /// `luminance_weights`: weights that turn a source pixel into scene luminance after
    /// white balance and exposure.
    pub fn new(source: &ImageF, luminance_weights: [f32; 3]) -> Self {
        let small = downscale_to_fit(source, BASE_LONG_EDGE);
        let (width, height) = (small.width, small.height);
        let count = width * height;

        let log_y: Vec<f32> = small
            .pixels
            .par_chunks(3)
            .map(|px| {
                let y = luminance_weights[0] * px[0] + luminance_weights[1] * px[1] + luminance_weights[2] * px[2];
                y.max(MIN_LUMINANCE).log2()
            })
            .collect();

        // Self-guided filter (He et al.): flat regions are smoothed, strong edges kept.
        let r = ((width.max(height) as f64 * BASE_RADIUS).round() as usize).max(1);
        let sq: Vec<f32> = log_y.iter().map(|v| v * v).collect();
        let mean = box_mean(&log_y, width, height, r);
        let mean_sq = box_mean(&sq, width, height, r);
        let mut a = vec![0.0f32; count];
        let mut b = vec![0.0f32; count];
        for i in 0..count {
            let variance = (mean_sq[i] - mean[i] * mean[i]).max(0.0);
            a[i] = variance / (variance + BASE_EPSILON);
            b[i] = mean[i] - a[i] * mean[i];
        }
        Self {
            width,
            a: box_mean(&a, width, height, r),
            b: box_mean(&b, width, height, r),
            x_taps: taps(source.width, width),
            y_taps: taps(source.height, height),
        }
    }

    /// Base log2 luminance at pixel (x, y) of the source, given that pixel's own log2
    /// luminance.
    pub fn at(&self, x: usize, y: usize, log2_luminance: f32) -> f32 {
        let tx = self.x_taps[x];
        let ty = self.y_taps[y];
        let (r0, r1) = (ty.i0 * self.width, ty.i1 * self.width);
        let sample = |g: &[f32]| {
            let top = g[r0 + tx.i0] + (g[r0 + tx.i1] - g[r0 + tx.i0]) * tx.f;
            let bottom = g[r1 + tx.i0] + (g[r1 + tx.i1] - g[r1 + tx.i0]) * tx.f;
            top + (bottom - top) * ty.f
        };
        sample(&self.a) * log2_luminance + sample(&self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_curve_is_monotonic_and_bounded() {
        for contrast in [-100.0, 0.0, 100.0] {
            for whites in [-100.0, 0.0, 100.0] {
                for blacks in [-100.0, 0.0, 100.0] {
                    let a = BasicAdjustments { contrast, whites, blacks, ..Default::default() };
                    let curve = ToneLut::new(&a, &ToneCurve::default());
                    let mut previous = -1.0;
                    for i in 0..=4000 {
                        let v = curve.eval(i as f32 / 1000.0);
                        assert!(v >= previous - 1e-6);
                        assert!((0.0..=1.0).contains(&v));
                        previous = v;
                    }
                }
            }
        }
        // Neutral curve is the identity below white.
        let identity = ToneLut::new(&BasicAdjustments::default(), &ToneCurve::default());
        assert!((identity.eval(0.18) - 0.18).abs() < 1e-5);
        assert_eq!(identity.eval(3.0), 1.0);
    }

    #[test]
    fn box_mean_of_constant_is_constant() {
        let src = vec![2.5f32; 7 * 5];
        for v in box_mean(&src, 7, 5, 2) {
            assert!((v - 2.5).abs() < 1e-6);
        }
    }
}
