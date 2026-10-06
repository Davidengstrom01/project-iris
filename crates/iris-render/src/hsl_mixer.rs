//! HSL adjustments on display-linear Rec.2020 pixels.
//!
//! Works in Oklab (Ottosson 2020), a perceptual colour space in which hue angles match
//! how colours look and lightness is separate from chroma. Each pixel is affected by the
//! two colour ranges its hue falls between, blended smoothly; near-neutral pixels (whose
//! hue is meaningless) are left alone.

use std::f32::consts::PI;
use std::sync::OnceLock;

use iris_core::HslAdjustments;
use iris_core::color::{Mat3, REC2020_TO_XYZ, inverse, mul, mul_vec, to_f32, xyz_to_rec2020};
use iris_core::hsl::HSL_COLOR_COUNT;

/// Degrees at +-100.
const MAX_HUE_SHIFT: f32 = 30.0;
/// Relative Oklab lightness at +-100.
const MAX_LUMINANCE_CHANGE: f32 = 0.35;
/// Chroma below which a colour counts as neutral (Oklab units; vivid colours are 0.1-0.3).
const NEUTRAL_CHROMA: f32 = 0.005;
const FULL_CHROMA: f32 = 0.04;

// Oklab matrices, from CIE XYZ (D65).
const XYZ_TO_LMS: Mat3 = [
    0.8189330101,
    0.3618667424,
    -0.1288597137, //
    0.0329845436,
    0.9293118715,
    0.0361456387, //
    0.0482003018,
    0.2643662691,
    0.6338517070,
];
const LMS_TO_LAB: Mat3 = [
    0.2104542553,
    0.7936177850,
    -0.0040720468, //
    1.9779984951,
    -2.4285922050,
    0.4505937099, //
    0.0259040371,
    0.7827717662,
    -0.8086757660,
];

struct Matrices {
    rgb_to_lms: [f32; 9],
    lms_to_lab: [f32; 9],
    lab_to_lms: [f32; 9],
    lms_to_rgb: [f32; 9],
}

fn matrices() -> &'static Matrices {
    static M: OnceLock<Matrices> = OnceLock::new();
    M.get_or_init(|| {
        let rgb_to_lms = mul(&XYZ_TO_LMS, &REC2020_TO_XYZ);
        Matrices {
            rgb_to_lms: to_f32(&rgb_to_lms),
            lms_to_lab: to_f32(&LMS_TO_LAB),
            lab_to_lms: to_f32(&inverse(&LMS_TO_LAB)),
            lms_to_rgb: to_f32(&inverse(&rgb_to_lms)),
        }
    })
}

#[inline]
fn multiply(m: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

fn rgb_to_oklab(rgb: [f32; 3]) -> [f32; 3] {
    let m = matrices();
    let lms = multiply(&m.rgb_to_lms, rgb).map(f32::cbrt);
    multiply(&m.lms_to_lab, lms)
}

fn oklab_to_rgb(lab: [f32; 3]) -> [f32; 3] {
    let m = matrices();
    let lms = multiply(&m.lab_to_lms, lab).map(|v| v * v * v);
    multiply(&m.lms_to_rgb, lms)
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Oklab hue angle (degrees) at the centre of each colour range.
pub fn band_centers() -> &'static [f32; HSL_COLOR_COUNT] {
    static CENTERS: OnceLock<[f32; HSL_COLOR_COUNT]> = OnceLock::new();
    CENTERS.get_or_init(|| {
        // Reference colours (sRGB): red, orange, yellow, green, aqua, blue, purple, magenta.
        const SRGB: [[f64; 3]; HSL_COLOR_COUNT] = [
            [1.0, 0.0, 0.0],
            [1.0, 0.5, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.5, 0.0, 1.0],
            [1.0, 0.0, 1.0],
        ];
        const SRGB_TO_XYZ: Mat3 =
            [0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522, 0.0721750, 0.0193339, 0.1191920, 0.9503041];
        SRGB.map(|srgb| {
            let linear = srgb.map(|c| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) });
            let rgb2020 = mul_vec(&xyz_to_rec2020(), &mul_vec(&SRGB_TO_XYZ, &linear));
            let lab = rgb_to_oklab(rgb2020.map(|v| v as f32));
            let h = lab[2].atan2(lab[1]) * 180.0 / PI;
            if h < 0.0 { h + 360.0 } else { h }
        })
    })
}

pub struct HslMixer {
    active: bool,
    /// Degrees.
    hue_shift: [f32; HSL_COLOR_COUNT],
    /// Chroma factor - 1.
    saturation: [f32; HSL_COLOR_COUNT],
    /// Lightness factor - 1.
    luminance: [f32; HSL_COLOR_COUNT],
}

impl HslMixer {
    pub fn new(hsl: &HslAdjustments) -> Self {
        Self {
            active: !hsl.is_neutral(),
            hue_shift: hsl.bands.map(|b| b.hue / 100.0 * MAX_HUE_SHIFT),
            saturation: hsl.bands.map(|b| b.saturation / 100.0),
            luminance: hsl.bands.map(|b| b.luminance / 100.0 * MAX_LUMINANCE_CHANGE),
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn apply(&self, rgb: &mut [f32]) {
        let mut lab = rgb_to_oklab([rgb[0], rgb[1], rgb[2]]);
        let chroma = lab[1].hypot(lab[2]);
        if chroma < NEUTRAL_CHROMA {
            return;
        }
        let mut hue = lab[2].atan2(lab[1]) * 180.0 / PI;
        if hue < 0.0 {
            hue += 360.0;
        }

        // Find the two colour ranges around this hue and blend between them.
        let centers = band_centers();
        let lower = centers.iter().rposition(|&c| c <= hue).unwrap_or(HSL_COLOR_COUNT - 1);
        let upper = (lower + 1) % HSL_COLOR_COUNT;
        let mut gap = centers[upper] - centers[lower];
        let mut offset = hue - centers[lower];
        if gap <= 0.0 {
            gap += 360.0;
        }
        if offset < 0.0 {
            offset += 360.0;
        }
        let w_upper = smoothstep(offset / gap);
        let w_lower = 1.0 - w_upper;
        let strength = smoothstep((chroma - NEUTRAL_CHROMA) / (FULL_CHROMA - NEUTRAL_CHROMA));

        let blend = |v: &[f32; HSL_COLOR_COUNT]| (w_lower * v[lower] + w_upper * v[upper]) * strength;
        let hue_shift = blend(&self.hue_shift);
        let saturation = blend(&self.saturation);
        let luminance = blend(&self.luminance);
        if hue_shift == 0.0 && saturation == 0.0 && luminance == 0.0 {
            return;
        }

        let new_hue = (hue + hue_shift) * PI / 180.0;
        let new_chroma = chroma * (1.0 + saturation).max(0.0);
        lab[0] = (lab[0] * (1.0 + luminance)).max(0.0);
        lab[1] = new_chroma * new_hue.cos();
        lab[2] = new_chroma * new_hue.sin();
        let out = oklab_to_rgb(lab);
        for c in 0..3 {
            rgb[c] = out[c].max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::HslColor;

    #[test]
    fn bands_are_in_hue_order() {
        let c = band_centers();
        for i in 0..HSL_COLOR_COUNT - 1 {
            assert!(c[i] < c[i + 1], "band {i}: {} >= {}", c[i], c[i + 1]);
        }
        assert!(c[0] > 15.0 && c[0] < 45.0); // red sits around 29 degrees in Oklab
    }

    #[test]
    fn blends_smoothly_around_the_hue_circle() {
        // Walk around the hue circle with one range desaturated: no sudden jumps.
        let mut hsl = HslAdjustments::default();
        hsl[HslColor::Green].saturation = -100.0;
        hsl[HslColor::Yellow].luminance = 80.0;
        let mixer = HslMixer::new(&hsl);
        let mut previous: Option<[f32; 3]> = None;
        for deg in 0..=360 {
            let h = deg as f32 * PI / 180.0;
            let mut rgb = [0.3 + 0.2 * h.cos(), 0.3 + 0.2 * (h - 2.094).cos(), 0.3 + 0.2 * (h + 2.094).cos()];
            mixer.apply(&mut rgb);
            if let Some(p) = previous {
                for c in 0..3 {
                    assert!((rgb[c] - p[c]).abs() < 0.03, "jump at {deg} deg");
                }
            }
            previous = Some(rgb);
        }
    }
}
