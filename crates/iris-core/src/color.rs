//! White balance and colour-space maths in the working space (linear Rec.2020, D65).

use crate::WhiteBalance;

pub type Vec3 = [f64; 3];
/// Row-major 3x3 matrix.
pub type Mat3 = [f64; 9];

pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut r = [0.0; 9];
    for i in 0..3 {
        for j in 0..3 {
            for k in 0..3 {
                r[i * 3 + j] += a[i * 3 + k] * b[k * 3 + j];
            }
        }
    }
    r
}

pub fn mul_vec(m: &Mat3, v: &Vec3) -> Vec3 {
    [
        m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
        m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
        m[6] * v[0] + m[7] * v[1] + m[8] * v[2],
    ]
}

pub fn inverse(m: &Mat3) -> Mat3 {
    let c00 = m[4] * m[8] - m[5] * m[7];
    let c01 = m[5] * m[6] - m[3] * m[8];
    let c02 = m[3] * m[7] - m[4] * m[6];
    let det = m[0] * c00 + m[1] * c01 + m[2] * c02;
    let s = 1.0 / det;
    [
        c00 * s,
        (m[2] * m[7] - m[1] * m[8]) * s,
        (m[1] * m[5] - m[2] * m[4]) * s,
        c01 * s,
        (m[0] * m[8] - m[2] * m[6]) * s,
        (m[2] * m[3] - m[0] * m[5]) * s,
        c02 * s,
        (m[1] * m[6] - m[0] * m[7]) * s,
        (m[0] * m[4] - m[1] * m[3]) * s,
    ]
}

pub fn to_f32(m: &Mat3) -> [f32; 9] {
    m.map(|v| v as f32)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Chromaticity {
    pub x: f64,
    pub y: f64,
}

pub const D65: Chromaticity = Chromaticity { x: 0.3127, y: 0.3290 };

/// Linear Rec.2020 (D65) -> CIE XYZ.
pub const REC2020_TO_XYZ: Mat3 = [
    0.6369580, 0.1446169, 0.1688810, //
    0.2627002, 0.6779981, 0.0593017, //
    0.0000000, 0.0280727, 1.0609851,
];

/// CIE XYZ -> linear Rec.2020 (D65).
pub fn xyz_to_rec2020() -> Mat3 {
    inverse(&REC2020_TO_XYZ)
}

/// Luminance weights of linear Rec.2020.
pub const LUMA_R: f64 = 0.2627;
pub const LUMA_G: f64 = 0.6780;
pub const LUMA_B: f64 = 0.0593;

pub const MIN_TEMPERATURE: f32 = 2000.0;
pub const MAX_TEMPERATURE: f32 = 15000.0;
pub const MAX_TINT: f32 = 150.0;

const BRADFORD: Mat3 = [
    0.8951, 0.2664, -0.1614, //
    -0.7502, 1.7135, 0.0367, //
    0.0389, -0.0685, 1.0296,
];

const DUV_PER_TINT: f64 = 1.0 / 3000.0;

#[derive(Clone, Copy)]
struct Uv {
    u: f64,
    v: f64,
}

/// Planckian locus in CIE 1960 uv (Krystek 1985, valid 1000-15000 K).
fn planckian(t: f64) -> Uv {
    let u =
        (0.860117757 + 1.54118254e-4 * t + 1.28641212e-7 * t * t) / (1.0 + 8.42420235e-4 * t + 7.08145163e-7 * t * t);
    let v =
        (0.317398726 + 4.22806245e-5 * t + 4.20481691e-8 * t * t) / (1.0 - 2.89741816e-5 * t + 1.61456053e-7 * t * t);
    Uv { u, v }
}

/// Unit normal to the locus at temperature t, pointing towards green (+v).
fn locus_normal(t: f64) -> Uv {
    let a = planckian(t - 1.0);
    let b = planckian(t + 1.0);
    let mut nu = -(b.v - a.v);
    let mut nv = b.u - a.u;
    let length = nu.hypot(nv);
    nu /= length;
    nv /= length;
    if nv < 0.0 {
        nu = -nu;
        nv = -nv;
    }
    Uv { u: nu, v: nv }
}

fn to_uv(c: Chromaticity) -> Uv {
    let d = -2.0 * c.x + 12.0 * c.y + 3.0;
    Uv { u: 4.0 * c.x / d, v: 6.0 * c.y / d }
}

fn from_uv(p: Uv) -> Chromaticity {
    let d = 2.0 * p.u - 8.0 * p.v + 4.0;
    Chromaticity { x: 3.0 * p.u / d, y: 2.0 * p.v / d }
}

fn to_xyz(c: Chromaticity) -> Vec3 {
    [c.x / c.y, 1.0, (1.0 - c.x - c.y) / c.y]
}

fn distance_to_locus(p: Uv, t: f64) -> f64 {
    let l = planckian(t);
    (p.u - l.u).hypot(p.v - l.v)
}

pub fn chromaticity(xyz: &Vec3) -> Chromaticity {
    let sum = xyz[0] + xyz[1] + xyz[2];
    Chromaticity { x: xyz[0] / sum, y: xyz[1] / sum }
}

/// White point of a temperature/tint pair: a point on the Planckian locus, offset
/// perpendicular to it by the tint (1 tint unit = 1/3000 Duv).
pub fn white_point(wb: &WhiteBalance) -> Chromaticity {
    let t = f64::from(wb.temperature).clamp(1000.0, 15000.0);
    let l = planckian(t);
    let n = locus_normal(t);
    let duv = f64::from(wb.tint) * DUV_PER_TINT;
    from_uv(Uv { u: l.u + n.u * duv, v: l.v + n.v * duv })
}

/// Inverse of [`white_point`], clamped to the supported range.
pub fn white_balance_from_white_point(white: Chromaticity) -> WhiteBalance {
    let p = to_uv(white);
    // Search in mired (perceptually even) for the closest locus point, then refine.
    const MIN_MIRED: f64 = 1e6 / 15000.0;
    const MAX_MIRED: f64 = 1e6 / 1000.0;
    const STEPS: i32 = 2000;
    let mut best = MIN_MIRED;
    let mut best_distance = 1e9;
    for i in 0..=STEPS {
        let mired = MIN_MIRED + (MAX_MIRED - MIN_MIRED) * f64::from(i) / f64::from(STEPS);
        let d = distance_to_locus(p, 1e6 / mired);
        if d < best_distance {
            best_distance = d;
            best = mired;
        }
    }
    let step = (MAX_MIRED - MIN_MIRED) / f64::from(STEPS);
    let mut lo = MIN_MIRED.max(best - step);
    let mut hi = MAX_MIRED.min(best + step);
    for _ in 0..60 {
        let m1 = lo + (hi - lo) / 3.0;
        let m2 = hi - (hi - lo) / 3.0;
        if distance_to_locus(p, 1e6 / m1) < distance_to_locus(p, 1e6 / m2) {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let t = 1e6 / ((lo + hi) / 2.0);
    let l = planckian(t);
    let n = locus_normal(t);
    let duv = (p.u - l.u) * n.u + (p.v - l.v) * n.v;

    WhiteBalance {
        temperature: (t as f32).clamp(MIN_TEMPERATURE, MAX_TEMPERATURE),
        tint: ((duv / DUV_PER_TINT) as f32).clamp(-MAX_TINT, MAX_TINT),
    }
}

/// Bradford chromatic adaptation in XYZ.
pub fn bradford_adaptation(from: Chromaticity, to: Chromaticity) -> Mat3 {
    let src = mul_vec(&BRADFORD, &to_xyz(from));
    let dst = mul_vec(&BRADFORD, &to_xyz(to));
    let scale = [dst[0] / src[0], 0.0, 0.0, 0.0, dst[1] / src[1], 0.0, 0.0, 0.0, dst[2] / src[2]];
    mul(&mul(&inverse(&BRADFORD), &scale), &BRADFORD)
}

/// Working-space (linear Rec.2020) matrix that re-balances an image decoded with the
/// as-shot white balance so that it renders with the target white balance.
pub fn white_balance_matrix(as_shot: &WhiteBalance, target: &WhiteBalance) -> Mat3 {
    // The decoder maps the as-shot illuminant to the working white (D65). Undo that,
    // then adapt from the target illuminant instead.
    let undo_as_shot = bradford_adaptation(D65, white_point(as_shot));
    let apply_target = bradford_adaptation(white_point(target), D65);
    mul(&mul(&mul(&xyz_to_rec2020(), &apply_target), &undo_as_shot), &REC2020_TO_XYZ)
}

/// White balance that makes the given working-space colour (from an image decoded with
/// the as-shot white balance) render as neutral grey.
pub fn white_balance_for_neutral(working_rgb: &Vec3, as_shot: &WhiteBalance) -> WhiteBalance {
    // In scene terms the sample was lit by this illuminant; choosing it as the target
    // white balance maps the sample to the working white.
    let scene = mul_vec(&bradford_adaptation(D65, white_point(as_shot)), &mul_vec(&REC2020_TO_XYZ, working_rgb));
    white_balance_from_white_point(chromaticity(&scene))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_balance_round_trips() {
        for t in [2500.0, 3200.0, 5000.0, 6500.0, 10000.0] {
            for tint in [-60.0, 0.0, 35.0] {
                let wb = white_balance_from_white_point(white_point(&WhiteBalance { temperature: t, tint }));
                assert!((wb.temperature - t).abs() < 0.5, "{}", wb.temperature);
                assert!((wb.tint - tint).abs() < 0.05);
            }
        }
        let d65 = white_balance_from_white_point(D65);
        assert!((d65.temperature - 6504.0).abs() < 10.0);
    }

    #[test]
    fn as_shot_white_balance_is_identity() {
        let as_shot = WhiteBalance { temperature: 5500.0, tint: 10.0 };
        let m = white_balance_matrix(&as_shot, &as_shot);
        for (i, v) in m.iter().enumerate() {
            let expected = if i % 4 == 0 { 1.0 } else { 0.0 };
            assert!((v - expected).abs() < 1e-9);
        }
    }
}
