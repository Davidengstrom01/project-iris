//! The single rendering pipeline shared by the interactive preview and export:
//!
//!   source (linear Rec.2020, as-shot white balance)
//!     -> resize
//!     -> clone / heal strokes                (scene-linear, before anything is developed)
//!     -> white balance + exposure           (one 3x3 matrix, scene-linear)
//!     -> highlights / shadows                (edge-aware local gain, scene-linear)
//!     -> masks: local white balance, exposure, highlights / shadows, contrast, saturation
//!     -> contrast / whites / blacks          (hue-preserving -> display-linear)
//!     -> RGB tone curve                      (hue-preserving, perceptual space)
//!     -> HSL: hue / saturation / luminance per colour range (Oklab)
//!     -> vibrance / saturation
//!     -> sharpening / noise reduction        (lightness and colour, perceptual)
//!     -> crop / rotation / straighten         (resampled, display-linear)
//!     -> output colour transform (sRGB)
//!
//! Sharpening and noise reduction work on the developed photo before the crop resamples it.

use std::borrow::Cow;

use iris_core::color::{LUMA_B, LUMA_G, LUMA_R, MAX_TEMPERATURE, MIN_TEMPERATURE, inverse, mul, white_balance_matrix};
use iris_core::crop::{Crop, CropGeometry};
use iris_core::{EditState, EncodedImage, ImageF, Mask, Samples, WhiteBalance};
use rayon::prelude::*;

use crate::color_transform::{to_srgb8, to_srgb16};
use crate::hsl_mixer::HslMixer;
use crate::mask_coverage::MaskCoverage;
use crate::resample::{fit_size, resize_area};
use crate::tone::{ToneBaseLayer, ToneLut};
use crate::{detail, retouch};

const MIDDLE_GREY_LOG2: f32 = -2.473_931_2; // log2(0.18)
const MIN_LUMINANCE: f32 = 1.0 / 65536.0;
/// Lift of the darkest regions at Shadows +100.
const MAX_SHADOWS_EV: f32 = 2.0;
/// Change of the brightest regions at Highlights ±100.
const MAX_HIGHLIGHTS_EV: f32 = 1.5;
/// White balance shift at local Temperature ±100.
const MAX_LOCAL_MIRED: f64 = 60.0;
/// Log-luminance slope change at local Contrast ±100.
const MAX_LOCAL_CONTRAST: f32 = 0.35;
/// Local contrast acts within ±8 EV of middle grey.
const MAX_CONTRAST_STOPS: f32 = 8.0;

const LR: f32 = LUMA_R as f32;
const LG: f32 = LUMA_G as f32;
const LB: f32 = LUMA_B as f32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    /// Long edge of the result; 0 = the source's resolution.
    pub max_long_edge: usize,
    /// 8 or 16.
    pub bits_per_channel: u32,
    /// Ignore the crop rectangle and show the whole straightened frame (while cropping).
    pub whole_frame: bool,
    /// Size of `source` relative to the full-resolution photo (e.g. 0.5 for a half-size
    /// preview decode), so sharpening and noise reduction keep their real-world size.
    pub source_scale: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { max_long_edge: 0, bits_per_channel: 8, whole_frame: false, source_scale: 1.0 }
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Highlights / Shadows: exposure change in EV for a region whose base luminance is
/// `ev` stops from middle grey (sensor white is about +2.5).
#[derive(Clone, Copy)]
struct RegionalExposure {
    shadows: f32,
    highlights: f32,
}

impl RegionalExposure {
    fn ev(&self, ev: f32) -> f32 {
        let shadow_weight = 1.0 - smoothstep(-5.0, 0.5, ev);
        let highlight_weight = smoothstep(-1.0, 2.5, ev);
        self.shadows * MAX_SHADOWS_EV * shadow_weight + self.highlights * MAX_HIGHLIGHTS_EV * highlight_weight
    }
}

/// Vibrance / Saturation on display-linear RGB.
struct Presence {
    /// -1..1
    saturation: f32,
    /// -1..1
    vibrance: f32,
}

impl Presence {
    fn active(&self) -> bool {
        self.saturation != 0.0 || self.vibrance != 0.0
    }

    fn hue_degrees(rgb: &[f32], hi: f32, lo: f32) -> f32 {
        let d = hi - lo;
        if hi == rgb[0] {
            60.0 * (((rgb[1] - rgb[2]) / d + 6.0) % 6.0)
        } else if hi == rgb[1] {
            60.0 * ((rgb[2] - rgb[0]) / d + 2.0)
        } else {
            60.0 * ((rgb[0] - rgb[1]) / d + 4.0)
        }
    }

    fn apply(&self, rgb: &mut [f32]) {
        let hi = rgb[0].max(rgb[1]).max(rgb[2]);
        let lo = rgb[0].min(rgb[1]).min(rgb[2]);
        if hi <= 0.0 || hi - lo < 1e-9 {
            return;
        }
        let mut factor = 1.0 + self.saturation;
        if self.vibrance != 0.0 {
            // Vibrance favours muted colours and, when boosting, spares skin tones.
            let current_saturation = (hi - lo) / hi;
            let mut amount = self.vibrance * (1.0 - current_saturation);
            if self.vibrance > 0.0 {
                let skin = (1.0 - (Self::hue_degrees(rgb, hi, lo) - 25.0).abs() / 30.0).clamp(0.0, 1.0);
                amount *= 1.0 - 0.6 * skin;
            }
            factor *= 1.0 + amount;
        }
        let y = LR * rgb[0] + LG * rgb[1] + LB * rgb[2];
        for c in &mut rgb[..3] {
            *c = (y + (*c - y) * factor).max(0.0);
        }
    }
}

/// One mask's local adjustments, applied in scene-linear light right after the global
/// white balance and exposure, so they behave like the global sliders (local exposure can
/// recover highlights, for example). Each is scaled by the mask's coverage at the pixel.
struct LocalStage {
    coverage: MaskCoverage,
    /// EV.
    exposure: f32,
    regional: RegionalExposure,
    /// Log-luminance slope change.
    contrast: f32,
    /// -1..1
    saturation: f32,
    /// Relative to the global white balance.
    white_balance: Option<[f32; 9]>,
}

impl LocalStage {
    fn new(mask: &Mask, width: usize, height: usize, as_shot: &WhiteBalance, global: &WhiteBalance) -> Self {
        let a = &mask.adjustments;
        let white_balance = (a.temperature != 0.0).then(|| {
            // Shift in mired; fewer mired is a warmer setting.
            let mired = 1e6 / f64::from(global.temperature) - f64::from(a.temperature) / 100.0 * MAX_LOCAL_MIRED;
            let shifted = WhiteBalance {
                temperature: (1e6 / mired.max(1.0)).clamp(f64::from(MIN_TEMPERATURE), f64::from(MAX_TEMPERATURE))
                    as f32,
                ..*global
            };
            let m = mul(&white_balance_matrix(as_shot, &shifted), &inverse(&white_balance_matrix(as_shot, global)));
            m.map(|v| v as f32)
        });
        Self {
            coverage: MaskCoverage::new(mask, width, height),
            exposure: a.exposure,
            regional: RegionalExposure { shadows: a.shadows / 100.0, highlights: a.highlights / 100.0 },
            contrast: a.contrast / 100.0 * MAX_LOCAL_CONTRAST,
            saturation: a.saturation / 100.0,
            white_balance,
        }
    }

    fn needs_base_layer(&self) -> bool {
        self.regional.shadows != 0.0 || self.regional.highlights != 0.0
    }

    /// `base_ev`: the region's brightness in EV from middle grey (if known).
    fn apply(&self, p: &mut [f32], w: f32, base_ev: f32) {
        if let Some(m) = &self.white_balance {
            let q = [
                m[0] * p[0] + m[1] * p[1] + m[2] * p[2],
                m[3] * p[0] + m[4] * p[1] + m[5] * p[2],
                m[6] * p[0] + m[7] * p[1] + m[8] * p[2],
            ];
            for c in 0..3 {
                p[c] = (p[c] + w * (q[c] - p[c])).max(0.0);
            }
        }
        let mut ev = w * self.exposure;
        if self.needs_base_layer() {
            ev += w * self.regional.ev(base_ev + ev);
        }
        let mut y = LR * p[0] + LG * p[1] + LB * p[2];
        if self.contrast != 0.0 && y > 0.0 {
            let stops = (y.log2() + ev - MIDDLE_GREY_LOG2).clamp(-MAX_CONTRAST_STOPS, MAX_CONTRAST_STOPS);
            ev += w * self.contrast * stops;
        }
        if ev != 0.0 {
            let k = ev.exp2();
            for c in &mut p[..3] {
                *c *= k;
            }
            y *= k;
        }
        if self.saturation != 0.0 {
            let factor = 1.0 + w * self.saturation;
            for c in &mut p[..3] {
                *c = (y + (*c - y) * factor).max(0.0);
            }
        }
    }
}

/// The per-pixel develop stages (everything before the crop), set up for one image.
struct Develop<'a> {
    input: &'a ImageF,
    /// White balance and exposure combined into one matrix.
    m: [f32; 9],
    /// Scene luminance of a source pixel after `m`.
    luma: [f32; 3],
    regional: RegionalExposure,
    global_regional: bool,
    locals: Vec<LocalStage>,
    base: Option<ToneBaseLayer>,
    /// Hue-preserving steps compose, so the basic tone and the RGB curve share one table.
    curve: ToneLut,
    hsl: HslMixer,
    presence: Presence,
}

impl<'a> Develop<'a> {
    fn new(input: &'a ImageF, as_shot: &WhiteBalance, edits: &EditState) -> Self {
        let a = &edits.basic;
        let wb = white_balance_matrix(as_shot, &a.white_balance);
        let gain = a.exposure.exp2();
        let m: [f32; 9] = wb.map(|v| v as f32 * gain);
        let luma = [
            (LUMA_R * f64::from(m[0]) + LUMA_G * f64::from(m[3]) + LUMA_B * f64::from(m[6])) as f32,
            (LUMA_R * f64::from(m[1]) + LUMA_G * f64::from(m[4]) + LUMA_B * f64::from(m[7])) as f32,
            (LUMA_R * f64::from(m[2]) + LUMA_G * f64::from(m[5]) + LUMA_B * f64::from(m[8])) as f32,
        ];
        let global_regional = a.shadows != 0.0 || a.highlights != 0.0;
        // Masks that change nothing are skipped.
        let locals: Vec<LocalStage> = edits
            .masks
            .iter()
            .filter(|mask| !mask.adjustments.is_neutral())
            .map(|mask| LocalStage::new(mask, input.width, input.height, as_shot, &a.white_balance))
            .collect();
        let local_regional = locals.iter().any(LocalStage::needs_base_layer);
        Self {
            input,
            m,
            luma,
            regional: RegionalExposure { shadows: a.shadows / 100.0, highlights: a.highlights / 100.0 },
            global_regional,
            base: (global_regional || local_regional).then(|| ToneBaseLayer::new(input, luma)),
            locals,
            curve: ToneLut::new(a, &edits.tone_curve),
            hsl: HslMixer::new(&edits.hsl),
            presence: Presence { saturation: a.saturation / 100.0, vibrance: a.vibrance / 100.0 },
        }
    }

    fn scratch(&self) -> Vec<Vec<f32>> {
        vec![vec![0.0; self.input.width]; self.locals.len()]
    }

    /// Develops pixels `x0..x1` of row `y` into `out` (display-linear RGB).
    fn row(&self, coverage: &mut [Vec<f32>], y: usize, x0: usize, x1: usize, out: &mut [f32]) {
        let m = &self.m;
        let luma = &self.luma;
        let source_row = self.input.row(y);
        for (local, coverage) in self.locals.iter().zip(coverage.iter_mut()) {
            local.coverage.row(y, coverage);
        }
        for x in x0..x1 {
            let s = &source_row[x * 3..x * 3 + 3];
            let p = &mut out[(x - x0) * 3..(x - x0) * 3 + 3];
            p[0] = (m[0] * s[0] + m[1] * s[1] + m[2] * s[2]).max(0.0);
            p[1] = (m[3] * s[0] + m[4] * s[1] + m[5] * s[2]).max(0.0);
            p[2] = (m[6] * s[0] + m[7] * s[1] + m[8] * s[2]).max(0.0);

            let mut base_ev = 0.0;
            if let Some(base) = &self.base {
                let lum = luma[0] * s[0] + luma[1] * s[1] + luma[2] * s[2];
                let log_y = lum.max(MIN_LUMINANCE).log2();
                base_ev = base.at(x, y, log_y) - MIDDLE_GREY_LOG2;
            }
            if self.global_regional {
                let k = self.regional.ev(base_ev).exp2();
                for c in p.iter_mut() {
                    *c *= k;
                }
            }
            for (local, coverage) in self.locals.iter().zip(coverage.iter()) {
                let cw = coverage[x];
                if cw > 0.0 {
                    local.apply(p, cw, base_ev);
                }
            }

            self.curve.apply_hue_preserving(p);
            if self.hsl.active() {
                self.hsl.apply(p);
            }
            if self.presence.active() {
                self.presence.apply(p);
            }
        }
    }
}

/// Writes display-linear rows (made by `row` with per-thread state from `init`) through the
/// output colour transform.
fn encode_rows<S>(
    output: &mut EncodedImage,
    init: impl Fn() -> S + Sync + Send,
    row: impl Fn(&mut S, usize, &mut [f32]) + Sync,
) {
    let w = output.width;
    let init = || (init(), vec![0.0f32; w * 3]);
    match &mut output.samples {
        Samples::Eight(data) => {
            data.par_chunks_mut(w * 3).enumerate().for_each_init(init, |(state, buffer), (y, out)| {
                row(state, y, buffer);
                to_srgb8(buffer, out);
            })
        }
        Samples::Sixteen(data) => {
            data.par_chunks_mut(w * 3).enumerate().for_each_init(init, |(state, buffer), (y, out)| {
                row(state, y, buffer);
                to_srgb16(buffer, out);
            })
        }
    }
}

/// Display-linear grey shown where a straightened photo leaves the frame empty (only
/// visible while cropping; a fitted crop never shows it).
const EMPTY: f32 = 0.006;

/// Renders `source` with `edits`. `as_shot` is the white balance the source was decoded
/// with.
pub fn render(source: &ImageF, as_shot: &WhiteBalance, edits: &EditState, options: &RenderOptions) -> EncodedImage {
    let bits = if options.bits_per_channel == 16 { 16 } else { 8 };
    let crop = &edits.crop;
    let whole_frame = options.whole_frame;
    let transformed =
        crop.quarter_turns.rem_euclid(4) != 0 || crop.angle != 0.0 || (!whole_frame && crop.has_rectangle());
    if !transformed {
        // Resize first so every later stage runs at the output resolution.
        let (width, height) = fit_size(source.width, source.height, options.max_long_edge);
        let input = retouched(resized(source, width, height), edits);
        let develop = Develop::new(&input, as_shot, edits);
        let mut output = EncodedImage::new(width, height, bits);
        let scale = options.source_scale * width as f32 / source.width as f32;
        if detail::is_active(&edits.detail, scale) {
            // Sharpening and noise reduction look at neighbours: develop everything first.
            let mut developed = vec![0.0f32; width * height * 3];
            developed
                .par_chunks_mut(width * 3)
                .enumerate()
                .for_each_init(|| develop.scratch(), |coverage, (y, out)| develop.row(coverage, y, 0, width, out));
            detail::apply(&mut developed, width, height, &edits.detail, scale);
            encode_rows(
                &mut output,
                || (),
                |_, y, out| out.copy_from_slice(&developed[y * width * 3..(y + 1) * width * 3]),
            );
        } else {
            encode_rows(&mut output, || develop.scratch(), |coverage, y, out| develop.row(coverage, y, 0, width, out));
        }
        return output;
    }

    // Scale the photo so the cropped result fits max_long_edge, develop the part of it the
    // crop needs (masks and local tone live in photo coordinates), then resample through
    // the crop's rotation.
    let long_edge = |g: &CropGeometry| g.width.max(g.height);
    let mut scale = match options.max_long_edge {
        0 => 1.0,
        max => (max as f64 / long_edge(&crop.geometry(source.width, source.height, whole_frame)) as f64).min(1.0),
    };
    let (mut pw, mut ph, mut geometry);
    loop {
        (pw, ph) = if scale < 1.0 {
            (
                ((source.width as f64 * scale).round() as usize).max(1),
                ((source.height as f64 * scale).round() as usize).max(1),
            )
        } else {
            (source.width, source.height)
        };
        geometry = crop.geometry(pw, ph, whole_frame);
        // Rounding can make the result a pixel too large; shrink a little and try again.
        if options.max_long_edge == 0 || long_edge(&geometry) <= options.max_long_edge || scale < 1e-3 {
            break;
        }
        scale *= options.max_long_edge as f64 / long_edge(&geometry) as f64 * 0.9995;
    }
    let input = retouched(resized(source, pw, ph), edits);
    let develop = Develop::new(&input, as_shot, edits);

    // The photo pixels the result needs (with a pixel of margin for interpolation, and room
    // for sharpening and noise reduction to see their neighbours).
    let scale = options.source_scale * pw as f32 / source.width as f32;
    let margin = 1.0 + detail::reach(&edits.detail, scale) as f64;
    let to_photo = geometry.photo_to_result.inverted();
    let (gw, gh) = (geometry.width as f64, geometry.height as f64);
    let corners = [(0.0, 0.0), (gw, 0.0), (0.0, gh), (gw, gh)].map(|(x, y)| to_photo.map(x, y));
    let span = |values: [f64; 4], limit: usize| {
        let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (
            (lo - margin).floor().clamp(0.0, limit as f64) as usize,
            (hi + margin).ceil().clamp(0.0, limit as f64) as usize,
        )
    };
    let (x0, x1) = span(corners.map(|c| c.0), pw);
    let (y0, y1) = span(corners.map(|c| c.1), ph);
    let (bw, bh) = (x1 - x0, y1 - y0);

    let mut developed = vec![0.0f32; bw * bh * 3];
    if bw > 0 {
        developed
            .par_chunks_mut(bw * 3)
            .enumerate()
            .for_each_init(|| develop.scratch(), |coverage, (i, out)| develop.row(coverage, y0 + i, x0, x1, out));
        detail::apply(&mut developed, bw, bh, &edits.detail, scale);
    }

    let mut output = EncodedImage::new(geometry.width, geometry.height, bits);
    let (pwf, phf) = (pw as f64, ph as f64);
    encode_rows(
        &mut output,
        || (),
        |_, oy, out| {
            for (ox, p) in out.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let (px, py) = to_photo.map(ox as f64 + 0.5, oy as f64 + 0.5);
                if bw == 0 || bh == 0 || px < 0.0 || py < 0.0 || px > pwf || py > phf {
                    p.fill(EMPTY);
                    continue;
                }
                // Bilinear between the four nearest pixel centres.
                let fx = (px - 0.5 - x0 as f64).clamp(0.0, (bw - 1) as f64);
                let fy = (py - 0.5 - y0 as f64).clamp(0.0, (bh - 1) as f64);
                let (ix, iy) = (fx as usize, fy as usize);
                let (ix1, iy1) = ((ix + 1).min(bw - 1), (iy + 1).min(bh - 1));
                let (tx, ty) = ((fx - ix as f64) as f32, (fy - iy as f64) as f32);
                let at = |x: usize, y: usize, c: usize| developed[(y * bw + x) * 3 + c];
                for (c, v) in p.iter_mut().enumerate() {
                    let top = at(ix, iy, c) + (at(ix1, iy, c) - at(ix, iy, c)) * tx;
                    let bottom = at(ix, iy1, c) + (at(ix1, iy1, c) - at(ix, iy1, c)) * tx;
                    *v = top + (bottom - top) * ty;
                }
            }
        },
    );
    output
}

/// Applies the clone and heal strokes to the (resized) source, before anything is developed.
fn retouched<'a>(input: Cow<'a, ImageF>, edits: &EditState) -> Cow<'a, ImageF> {
    if edits.retouch.is_empty() {
        return input;
    }
    let mut image = input.into_owned();
    retouch::apply(&mut image, &edits.retouch);
    Cow::Owned(image)
}

fn resized(source: &ImageF, width: usize, height: usize) -> Cow<'_, ImageF> {
    if (width, height) != (source.width, source.height) {
        Cow::Owned(resize_area(source, width, height))
    } else {
        Cow::Borrowed(source)
    }
}

/// Puts a single-channel image of the photo (e.g. a mask's coverage) through the crop, as
/// [`render`] does with the photo itself. Returns the result's width, height and values.
pub fn crop_gray(
    values: &[u8],
    width: usize,
    height: usize,
    crop: &Crop,
    whole_frame: bool,
) -> (usize, usize, Vec<u8>) {
    let transformed =
        crop.quarter_turns.rem_euclid(4) != 0 || crop.angle != 0.0 || (!whole_frame && crop.has_rectangle());
    if !transformed {
        return (width, height, values.to_vec());
    }
    let geometry = crop.geometry(width, height, whole_frame);
    let to_photo = geometry.photo_to_result.inverted();
    let mut out = vec![0u8; geometry.width * geometry.height];
    out.par_chunks_mut(geometry.width.max(1)).enumerate().for_each(|(oy, row)| {
        for (ox, v) in row.iter_mut().enumerate() {
            let (px, py) = to_photo.map(ox as f64 + 0.5, oy as f64 + 0.5);
            if px >= 0.0 && py >= 0.0 && px < width as f64 && py < height as f64 {
                *v = values[py as usize * width + px as usize];
            }
        }
    });
    (geometry.width, geometry.height, out)
}
