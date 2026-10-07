//! Sharpening and noise reduction on a developed (display-linear) image.
//!
//! Both work in a perceptual space split into lightness and colour, like a photographer
//! thinks of them: sharpening and luminance noise reduction change only the lightness (so
//! sharpening adds no colour fringes), colour noise reduction only the colour.
//!
//!   lightness L = luma of the gamma-2.2 encoded RGB
//!   colour    C = gamma-2.2 RGB minus L (three planes)
//!
//! Lengths are given in full-resolution pixels and scaled to the image being rendered, so a
//! smaller preview shows a correspondingly subtler effect.

use iris_core::Detail;
use iris_core::color::{LUMA_B, LUMA_G, LUMA_R};
use rayon::prelude::*;

use crate::tone::box_mean;

const GAMMA: f32 = 2.2;
const LR: f32 = LUMA_R as f32;
const LG: f32 = LUMA_G as f32;
const LB: f32 = LUMA_B as f32;

/// Below this many pixels an effect would be invisible; it is skipped.
const MIN_SIGMA: f32 = 0.25;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What the detail stage will do at a given scale (output pixels per full-resolution pixel).
struct Plan {
    /// Unsharp-mask strength and blur sigma (pixels), and the edge threshold for masking.
    sharpen: Option<(f32, f32, f32)>,
    /// Guided-filter radius, edge threshold (squared) and blend.
    luminance: Option<(usize, f32, f32)>,
    /// Gaussian sigma for the colour planes.
    color: Option<f32>,
}

impl Plan {
    fn new(detail: &Detail, scale: f32) -> Self {
        let s = &detail.sharpening;
        let n = &detail.noise_reduction;
        let sharpen = (s.amount > 0.0)
            .then(|| (s.amount / 100.0, s.radius * scale, s.masking / 100.0 * 0.08))
            .filter(|&(_, sigma, _)| sigma >= MIN_SIGMA);
        let luminance = (n.luminance > 0.0)
            .then(|| {
                let t = n.luminance / 100.0;
                let radius = (1.0 + 2.0 * t) * scale;
                let eps = (0.008 + 0.06 * t).powi(2);
                (radius, eps, (t * 2.0).min(1.0))
            })
            .filter(|&(radius, ..)| radius >= 0.5)
            .map(|(radius, eps, blend)| (radius.round().max(1.0) as usize, eps, blend));
        let color = (n.color > 0.0).then(|| (0.5 + 5.5 * n.color / 100.0) * scale).filter(|&sigma| sigma >= MIN_SIGMA);
        Self { sharpen, luminance, color }
    }

    fn is_empty(&self) -> bool {
        self.sharpen.is_none() && self.luminance.is_none() && self.color.is_none()
    }
}

/// How far around a pixel the detail stage looks, in output pixels (so a caller that
/// develops only part of an image can include enough margin).
pub fn reach(detail: &Detail, scale: f32) -> usize {
    let plan = Plan::new(detail, scale);
    let sharpen = plan.sharpen.map_or(0.0, |(_, sigma, _)| 3.0 * sigma + 1.0);
    let luminance = plan.luminance.map_or(0.0, |(r, ..)| 2.0 * r as f32);
    let color = plan.color.map_or(0.0, |sigma| 3.0 * sigma);
    sharpen.max(luminance).max(color).ceil() as usize
}

/// Whether [`apply`] would change anything at this scale.
pub fn is_active(detail: &Detail, scale: f32) -> bool {
    !Plan::new(detail, scale).is_empty()
}

/// Separable Gaussian blur with clamped edges.
fn gaussian(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil() as isize;
    let weights: Vec<f32> = (-radius..=radius).map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp()).collect();
    let total: f32 = weights.iter().sum();
    let weights: Vec<f32> = weights.iter().map(|v| v / total).collect();
    let (wi, hi) = (w as isize, h as isize);

    let mut horizontal = vec![0.0f32; src.len()];
    horizontal.par_chunks_mut(w).zip(src.par_chunks(w)).for_each(|(out, row)| {
        for (x, o) in out.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (k, wt) in weights.iter().enumerate() {
                let sx = (x as isize + k as isize - radius).clamp(0, wi - 1) as usize;
                sum += row[sx] * wt;
            }
            *o = sum;
        }
    });
    let mut out = vec![0.0f32; src.len()];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (k, wt) in weights.iter().enumerate() {
            let sy = (y as isize + k as isize - radius).clamp(0, hi - 1) as usize;
            for (o, s) in row.iter_mut().zip(&horizontal[sy * w..(sy + 1) * w]) {
                *o += s * wt;
            }
        }
    });
    out
}

/// Edge-preserving smoothing (self-guided filter, He et al.).
fn guided(src: &[f32], w: usize, h: usize, radius: usize, eps: f32) -> Vec<f32> {
    let sq: Vec<f32> = src.par_iter().map(|v| v * v).collect();
    let mean = box_mean(src, w, h, radius);
    let mean_sq = box_mean(&sq, w, h, radius);
    let (a, b): (Vec<f32>, Vec<f32>) = mean
        .par_iter()
        .zip(&mean_sq)
        .map(|(&m, &m2)| {
            let variance = (m2 - m * m).max(0.0);
            let a = variance / (variance + eps);
            (a, m - a * m)
        })
        .unzip();
    let a = box_mean(&a, w, h, radius);
    let b = box_mean(&b, w, h, radius);
    src.par_iter().zip(a.par_iter().zip(&b)).map(|(&v, (&a, &b))| a * v + b).collect()
}

/// Applies `detail` to `rgb` (interleaved display-linear RGB, `w` x `h`). `scale` is the
/// image's size relative to the full-resolution photo.
pub fn apply(rgb: &mut [f32], w: usize, h: usize, detail: &Detail, scale: f32) {
    let plan = Plan::new(detail, scale);
    if plan.is_empty() || w == 0 || h == 0 {
        return;
    }

    // Split into lightness and colour.
    let n = w * h;
    let mut lightness = vec![0.0f32; n];
    let mut color = vec![0.0f32; n * 3];
    lightness.par_iter_mut().zip(color.par_chunks_mut(3)).zip(rgb.par_chunks(3)).for_each(|((l, c), p)| {
        let e = [p[0], p[1], p[2]].map(|v| v.max(0.0).powf(1.0 / GAMMA));
        *l = LR * e[0] + LG * e[1] + LB * e[2];
        for k in 0..3 {
            c[k] = e[k] - *l;
        }
    });

    if let Some(sigma) = plan.color {
        for k in 0..3 {
            let plane: Vec<f32> = color.iter().skip(k).step_by(3).copied().collect();
            let smooth = gaussian(&plane, w, h, sigma);
            for (i, v) in smooth.into_iter().enumerate() {
                color[i * 3 + k] = v;
            }
        }
    }

    if let Some((radius, eps, blend)) = plan.luminance {
        let smooth = guided(&lightness, w, h, radius, eps);
        lightness.par_iter_mut().zip(smooth).for_each(|(l, s)| *l += (s - *l) * blend);
    }

    if let Some((amount, sigma, threshold)) = plan.sharpen {
        let blurred = gaussian(&lightness, w, h, sigma);
        let sharpened: Vec<f32> = (0..h)
            .into_par_iter()
            .flat_map_iter(|y| {
                let (lightness, blurred) = (&lightness, &blurred);
                (0..w).map(move |x| {
                    let i = y * w + x;
                    // Masking: only where the (blurred) image has edges.
                    let mask = if threshold > 0.0 {
                        let gx = blurred[y * w + (x + 1).min(w - 1)] - blurred[y * w + x.saturating_sub(1)];
                        let gy = blurred[(y + 1).min(h - 1) * w + x] - blurred[y.saturating_sub(1) * w + x];
                        smoothstep(0.25 * threshold, threshold, gx.hypot(gy))
                    } else {
                        1.0
                    };
                    lightness[i] + amount * (lightness[i] - blurred[i]) * mask
                })
            })
            .collect();
        lightness = sharpened;
    }

    // Back to display-linear RGB.
    rgb.par_chunks_mut(3).zip(lightness.par_iter().zip(color.par_chunks(3))).for_each(|(p, (&l, c))| {
        for k in 0..3 {
            p[k] = (l + c[k]).max(0.0).powf(GAMMA);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::{NoiseReduction, Sharpening};

    /// Deterministic pseudo-random noise in [-1, 1].
    fn noise(i: usize) -> f32 {
        let x = (i as u32).wrapping_mul(2654435761).rotate_left(13).wrapping_mul(2246822519);
        (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }

    fn sharpen(amount: f32, masking: f32) -> Detail {
        Detail { sharpening: Sharpening { amount, radius: 1.0, masking }, ..Default::default() }
    }

    fn std_dev(values: impl Iterator<Item = f32> + Clone) -> f32 {
        let n = values.clone().count() as f32;
        let mean = values.clone().sum::<f32>() / n;
        (values.map(|v| (v - mean).powi(2)).sum::<f32>() / n).sqrt()
    }

    #[test]
    fn neutral_detail_changes_nothing() {
        let mut rgb: Vec<f32> = (0..30).map(|i| i as f32 / 30.0).collect();
        let before = rgb.clone();
        apply(&mut rgb, 5, 2, &Detail::default(), 1.0);
        assert_eq!(rgb, before);
        assert!(!is_active(&Detail::default(), 1.0));
    }

    #[test]
    fn sharpening_raises_edge_contrast_without_colour_fringes() {
        // A vertical edge between dark and light grey.
        let (w, h) = (20, 8);
        let mut rgb: Vec<f32> = (0..w * h).flat_map(|i| [if i % w < 10 { 0.05 } else { 0.4 }; 3]).collect();
        apply(&mut rgb, w, h, &sharpen(100.0, 0.0), 1.0);
        let at = |x: usize| rgb[(4 * w + x) * 3];
        assert!(at(9) < 0.05 && at(10) > 0.4, "{} {}", at(9), at(10));
        assert!((at(0) - 0.05).abs() < 1e-4 && (at(19) - 0.4).abs() < 1e-4); // flat areas unchanged
        let p = &rgb[(4 * w + 10) * 3..(4 * w + 10) * 3 + 3];
        assert!((p[0] - p[1]).abs() < 1e-5 && (p[1] - p[2]).abs() < 1e-5); // still grey
    }

    #[test]
    fn masking_spares_smooth_areas() {
        let (w, h) = (64, 64);
        let mut flat: Vec<f32> = (0..w * h).flat_map(|i| [0.2 + 0.01 * noise(i); 3]).collect();
        let before = std_dev(flat.iter().step_by(3).copied());
        let mut masked = flat.clone();
        apply(&mut flat, w, h, &sharpen(150.0, 0.0), 1.0);
        apply(&mut masked, w, h, &sharpen(150.0, 100.0), 1.0);
        let unmasked_noise = std_dev(flat.iter().step_by(3).copied());
        let masked_noise = std_dev(masked.iter().step_by(3).copied());
        assert!(unmasked_noise > before * 1.5); // sharpening amplifies grain ...
        assert!(masked_noise < before * 1.1); // ... unless masked out
    }

    #[test]
    fn luminance_noise_reduction_smooths_grain_and_keeps_edges() {
        let (w, h) = (64, 64);
        let base = |i: usize| if i % w < 32 { 0.05 } else { 0.5 };
        let mut rgb: Vec<f32> = (0..w * h).flat_map(|i| [base(i) * (1.0 + 0.15 * noise(i)); 3]).collect();
        let region =
            |rgb: &[f32]| (0..w * h).filter(|i| i % w > 40 && i % w < 60).map(|i| rgb[i * 3]).collect::<Vec<_>>();
        let before = std_dev(region(&rgb).into_iter());
        let detail = Detail { noise_reduction: NoiseReduction { luminance: 60.0, color: 0.0 }, ..Default::default() };
        apply(&mut rgb, w, h, &detail, 1.0);
        assert!(std_dev(region(&rgb).into_iter()) < before * 0.6);
        // The edge stays sharp: one pixel either side keeps its level.
        let row = 32 * w;
        assert!(rgb[(row + 30) * 3] < 0.1 && rgb[(row + 34) * 3] > 0.4);
    }

    #[test]
    fn colour_noise_reduction_removes_blotches_but_keeps_the_colour() {
        let (w, h) = (64, 64);
        let mut rgb: Vec<f32> =
            (0..w * h).flat_map(|i| [0.3 * (1.0 + 0.3 * noise(i)), 0.2, 0.1 * (1.0 + 0.3 * noise(i + 7919))]).collect();
        let chroma = |rgb: &[f32]| rgb.chunks(3).map(|p| p[0] - p[2]).collect::<Vec<_>>();
        let before = chroma(&rgb);
        let detail = Detail { noise_reduction: NoiseReduction { luminance: 0.0, color: 50.0 }, ..Default::default() };
        apply(&mut rgb, w, h, &detail, 1.0);
        let after = chroma(&rgb);
        assert!(std_dev(after.iter().copied()) < std_dev(before.iter().copied()) * 0.4);
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!((mean(&after) - mean(&before)).abs() < 0.01);
    }

    #[test]
    fn small_previews_skip_invisible_effects() {
        // Radius 1 px at a quarter of full resolution is a quarter of a pixel: skipped.
        assert!(!is_active(&sharpen(100.0, 0.0), 0.2));
        assert!(is_active(&sharpen(100.0, 0.0), 0.5));
        assert_eq!(reach(&sharpen(100.0, 0.0), 1.0), 4);
    }
}
