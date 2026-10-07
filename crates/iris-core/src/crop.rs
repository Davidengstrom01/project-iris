//! Crop and rotation of a photo, applied after all tonal and colour edits:
//!
//!   photo -> quarter turns -> straighten (rotate about the centre) -> crop rectangle
//!
//! The crop rectangle is axis-aligned in the straightened frame: the frame has the size of
//! the photo after its quarter turns, and the photo is rotated by `angle` inside it.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    /// Clockwise 90° rotations, 0..3.
    pub quarter_turns: i32,
    /// Straighten, degrees, -45..45; positive turns the photo clockwise.
    pub angle: f32,
    /// Crop rectangle as fractions of the straightened frame.
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    /// Locked aspect ratio (width / height in pixels); 0 = free.
    pub aspect: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self { quarter_turns: 0, angle: 0.0, left: 0.0, top: 0.0, right: 1.0, bottom: 1.0, aspect: 0.0 }
    }
}

pub const MAX_STRAIGHTEN_ANGLE: f32 = 45.0;
/// Pixels a crop corner may lie outside the photo.
const TOLERANCE: f64 = 0.02;
const MIN_CROP_FRACTION: f32 = 0.01;

/// x' = m11 x + m12 y + dx,  y' = m21 x + m22 y + dy
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub m11: f64,
    pub m12: f64,
    pub m21: f64,
    pub m22: f64,
    pub dx: f64,
    pub dy: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { m11: 1.0, m12: 0.0, m21: 0.0, m22: 1.0, dx: 0.0, dy: 0.0 };

    const fn new(m11: f64, m12: f64, m21: f64, m22: f64, dx: f64, dy: f64) -> Self {
        Self { m11, m12, m21, m22, dx, dy }
    }

    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        (self.m11 * x + self.m12 * y + self.dx, self.m21 * x + self.m22 * y + self.dy)
    }

    pub fn inverted(&self) -> Affine {
        let det = self.m11 * self.m22 - self.m12 * self.m21;
        let m11 = self.m22 / det;
        let m12 = -self.m12 / det;
        let m21 = -self.m21 / det;
        let m22 = self.m11 / det;
        Affine { m11, m12, m21, m22, dx: -(m11 * self.dx + m12 * self.dy), dy: -(m21 * self.dx + m22 * self.dy) }
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Self::new(sx, 0.0, 0.0, sy, 0.0, 0.0)
    }

    pub fn translate(tx: f64, ty: f64) -> Self {
        Self::new(1.0, 0.0, 0.0, 1.0, tx, ty)
    }
}

/// `a * b` maps a point through b, then a.
impl std::ops::Mul for Affine {
    type Output = Affine;
    fn mul(self, b: Affine) -> Affine {
        let a = self;
        Affine {
            m11: a.m11 * b.m11 + a.m12 * b.m21,
            m12: a.m11 * b.m12 + a.m12 * b.m22,
            m21: a.m21 * b.m11 + a.m22 * b.m21,
            m22: a.m21 * b.m12 + a.m22 * b.m22,
            dx: a.m11 * b.dx + a.m12 * b.dy + a.dx,
            dy: a.m21 * b.dx + a.m22 * b.dy + a.dy,
        }
    }
}

/// The result of cropping an image of a given size: its size and where each photo pixel
/// lands in it. Coordinates are continuous, with pixel centres at i + 0.5.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropGeometry {
    pub width: usize,
    pub height: usize,
    pub photo_to_result: Affine,
}

fn turns(quarter_turns: i32) -> i32 {
    quarter_turns.rem_euclid(4)
}

/// Size of the straightened frame (the photo after its quarter turns).
pub fn frame_size(photo_width: usize, photo_height: usize, quarter_turns: i32) -> (usize, usize) {
    if quarter_turns % 2 != 0 { (photo_height, photo_width) } else { (photo_width, photo_height) }
}

/// Straightened frame -> photo pixels after the quarter turns (the inverse rotation).
struct Straighten {
    cx: f64,
    cy: f64,
    c: f64,
    s: f64,
}

impl Straighten {
    fn new(frame_width: usize, frame_height: usize, angle: f32) -> Self {
        let a = f64::from(angle) * PI / 180.0;
        Self { cx: frame_width as f64 / 2.0, cy: frame_height as f64 / 2.0, c: a.cos(), s: a.sin() }
    }

    /// Rotates a frame point back (counter-clockwise by angle) onto the turned photo.
    fn to_photo(&self, x: f64, y: f64) -> (f64, f64) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        (self.cx + self.c * dx + self.s * dy, self.cy - self.s * dx + self.c * dy)
    }
}

impl Crop {
    pub fn has_rectangle(&self) -> bool {
        self.left != 0.0 || self.top != 0.0 || self.right != 1.0 || self.bottom != 1.0
    }

    pub fn is_identity(&self) -> bool {
        self.quarter_turns == 0 && self.angle == 0.0 && !self.has_rectangle()
    }

    /// Geometry of this crop for a photo of the given size. With `whole_frame` the crop
    /// rectangle is ignored and the result is the whole straightened frame (as shown while
    /// cropping). Without rotation, the rectangle is rounded to whole pixels so crops stay
    /// sharp.
    pub fn geometry(&self, photo_width: usize, photo_height: usize, whole_frame: bool) -> CropGeometry {
        let (w, h) = (photo_width as f64, photo_height as f64);
        let turn = match turns(self.quarter_turns) {
            1 => Affine::new(0.0, -1.0, 1.0, 0.0, h, 0.0), // (x, y) -> (h - y, x)
            2 => Affine::new(-1.0, 0.0, 0.0, -1.0, w, h),  // (x, y) -> (w - x, h - y)
            3 => Affine::new(0.0, 1.0, -1.0, 0.0, 0.0, w), // (x, y) -> (y, w - x)
            _ => Affine::IDENTITY,
        };
        let (fw, fh) = frame_size(photo_width, photo_height, self.quarter_turns);
        let (fw, fh) = (fw as f64, fh as f64);

        // Clockwise rotation about the frame centre (y points down).
        let a = f64::from(self.angle) * PI / 180.0;
        let (c, s) = (a.cos(), a.sin());
        let (cx, cy) = (fw / 2.0, fh / 2.0);
        let rotate = Affine::new(c, -s, s, c, cx - c * cx + s * cy, cy - s * cx - c * cy);

        let (mut x0, mut y0, mut cw, mut ch) = (0.0, 0.0, fw, fh);
        if !whole_frame {
            x0 = f64::from(self.left) * fw;
            y0 = f64::from(self.top) * fh;
            cw = f64::from(self.right - self.left) * fw;
            ch = f64::from(self.bottom - self.top) * fh;
            if self.angle == 0.0 {
                let x1 = (x0 + cw).round();
                let y1 = (y0 + ch).round();
                x0 = x0.round();
                y0 = y0.round();
                cw = x1 - x0;
                ch = y1 - y0;
            }
        }
        CropGeometry {
            width: (cw.round() as i64).max(1) as usize,
            height: (ch.round() as i64).max(1) as usize,
            photo_to_result: Affine::translate(-x0, -y0) * rotate * turn,
        }
    }

    /// Whether the crop rectangle lies within the frame and within the straightened photo
    /// (no empty corners).
    pub fn fits_photo(&self, photo_width: usize, photo_height: usize) -> bool {
        if self.left < -1e-6 || self.top < -1e-6 || self.right > 1.0 + 1e-6 || self.bottom > 1.0 + 1e-6 {
            return false;
        }
        let (fw, fh) = frame_size(photo_width, photo_height, self.quarter_turns);
        let st = Straighten::new(fw, fh, self.angle);
        let (fwf, fhf) = (fw as f64, fh as f64);
        for fx in [self.left, self.right] {
            for fy in [self.top, self.bottom] {
                let (x, y) = st.to_photo(f64::from(fx) * fwf, f64::from(fy) * fhf);
                if x < -TOLERANCE || y < -TOLERANCE || x > fwf + TOLERANCE || y > fhf + TOLERANCE {
                    return false;
                }
            }
        }
        true
    }

    /// Shrinks the crop rectangle about its centre (keeping its aspect ratio) until it fits.
    pub fn constrained(self, photo_width: usize, photo_height: usize) -> Crop {
        if photo_width == 0 || photo_height == 0 || self.fits_photo(photo_width, photo_height) {
            return self;
        }
        // Scale about the centre (moved into the photo if needed) until it fits.
        let mut cx = f64::from(self.left + self.right) / 2.0;
        let mut cy = f64::from(self.top + self.bottom) / 2.0;
        let hw = f64::from(self.right - self.left) / 2.0;
        let hh = f64::from(self.bottom - self.top) / 2.0;
        let centre = Crop { left: cx as f32, right: cx as f32, top: cy as f32, bottom: cy as f32, ..self };
        if !centre.fits_photo(photo_width, photo_height) {
            cx = 0.5;
            cy = 0.5;
        }
        let scaled = |k: f64| Crop {
            left: (cx - hw * k) as f32,
            right: (cx + hw * k) as f32,
            top: (cy - hh * k) as f32,
            bottom: (cy + hh * k) as f32,
            ..self
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if scaled(mid).fits_photo(photo_width, photo_height) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        scaled(lo)
    }

    /// The largest rectangle of the given aspect ratio (0 = free), centred on the current
    /// one, that fits the photo.
    pub fn with_aspect(mut self, aspect: f32, photo_width: usize, photo_height: usize) -> Crop {
        let (fw, fh) = frame_size(photo_width, photo_height, self.quarter_turns);
        let (fw, fh) = (fw as f64, fh as f64);
        self.aspect = aspect;
        if aspect <= 0.0 {
            return self;
        }
        // The largest rectangle of this aspect in the frame, centred on the current crop,
        // then shrunk to fit the straightened photo.
        let aspect = f64::from(aspect);
        let (mut w, mut h) = (fw, fw / aspect);
        if h > fh {
            h = fh;
            w = fh * aspect;
        }
        let cx = (f64::from(self.left + self.right) / 2.0 * fw).clamp(w / 2.0, fw - w / 2.0);
        let cy = (f64::from(self.top + self.bottom) / 2.0 * fh).clamp(h / 2.0, fh - h / 2.0);
        self.left = ((cx - w / 2.0) / fw) as f32;
        self.right = ((cx + w / 2.0) / fw) as f32;
        self.top = ((cy - h / 2.0) / fh) as f32;
        self.bottom = ((cy + h / 2.0) / fh) as f32;
        self.constrained(photo_width, photo_height)
    }

    /// Moves the rectangle from `self` (which fits the photo) towards `target`, as far as
    /// it can go while still fitting. Used while dragging, so a drag stops at the photo's
    /// edge instead of jumping.
    pub fn toward(self, target: Crop, photo_width: usize, photo_height: usize) -> Crop {
        if target.fits_photo(photo_width, photo_height) {
            return target;
        }
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let at = |t: f32| Crop {
            left: lerp(self.left, target.left, t),
            top: lerp(self.top, target.top, t),
            right: lerp(self.right, target.right, t),
            bottom: lerp(self.bottom, target.bottom, t),
            ..target
        };
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..30 {
            let mid = (lo + hi) / 2.0;
            if at(mid).fits_photo(photo_width, photo_height) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        at(lo)
    }

    /// Width / height of the crop rectangle in pixels.
    pub fn pixel_aspect(&self, photo_width: usize, photo_height: usize) -> f32 {
        let (fw, fh) = frame_size(photo_width, photo_height, self.quarter_turns);
        let w = (self.right - self.left) * fw as f32;
        let h = (self.bottom - self.top) * fh as f32;
        if h > 0.0 { w / h } else { 1.0 }
    }

    /// Turns the photo by 90°; the crop rectangle turns with it.
    pub fn rotated_quarter(&self, clockwise: bool) -> Crop {
        let mut r = *self;
        r.quarter_turns = (self.quarter_turns + if clockwise { 1 } else { -1 }).rem_euclid(4);
        if clockwise {
            // (u, v) -> (1 - v, u)
            r.left = 1.0 - self.bottom;
            r.top = self.left;
            r.right = 1.0 - self.top;
            r.bottom = self.right;
        } else {
            // (u, v) -> (v, 1 - u)
            r.left = self.top;
            r.top = 1.0 - self.right;
            r.right = self.bottom;
            r.bottom = 1.0 - self.left;
        }
        if r.aspect > 0.0 {
            r.aspect = 1.0 / r.aspect;
        }
        r
    }

    /// Clamps values into range (e.g. after reading a file); the rectangle may still need
    /// [`Crop::constrained`] once the photo size is known.
    pub fn sanitized(mut self) -> Crop {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        self.quarter_turns = self.quarter_turns.rem_euclid(4);
        self.angle = finite(self.angle, 0.0).clamp(-MAX_STRAIGHTEN_ANGLE, MAX_STRAIGHTEN_ANGLE);
        self.left = finite(self.left, 0.0).clamp(0.0, 1.0 - MIN_CROP_FRACTION);
        self.top = finite(self.top, 0.0).clamp(0.0, 1.0 - MIN_CROP_FRACTION);
        self.right = finite(self.right, 1.0).clamp(self.left + MIN_CROP_FRACTION, 1.0);
        self.bottom = finite(self.bottom, 1.0).clamp(self.top + MIN_CROP_FRACTION, 1.0);
        self.aspect = finite(self.aspect, 0.0).clamp(0.0, 100.0);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn quarter_turns_map_corners() {
        let crop = Crop { quarter_turns: 1, ..Default::default() };
        let g = crop.geometry(600, 400, false);
        assert_eq!((g.width, g.height), (400, 600));
        // The photo's top-left corner ends up top-right after a clockwise turn.
        assert!(close(g.photo_to_result.map(0.0, 0.0), (400.0, 0.0)));
        assert!(close(g.photo_to_result.inverted().map(400.0, 0.0), (0.0, 0.0)));
    }

    #[test]
    fn rectangle_is_rounded_without_rotation() {
        let crop = Crop { left: 0.1001, top: 0.2, right: 0.6, bottom: 0.9, ..Default::default() };
        let g = crop.geometry(1000, 500, false);
        assert_eq!((g.width, g.height), (500, 350));
        assert!(close(g.photo_to_result.map(100.0, 100.0), (0.0, 0.0)));
    }

    #[test]
    fn straightened_crop_is_constrained_to_the_photo() {
        let crop = Crop { angle: 10.0, ..Default::default() };
        assert!(!crop.fits_photo(600, 400));
        let fitted = crop.constrained(600, 400);
        assert!(fitted.fits_photo(600, 400));
        assert!(fitted.right - fitted.left > 0.5);
        // The aspect ratio is kept.
        let ratio = (fitted.right - fitted.left) / (fitted.bottom - fitted.top);
        assert!((ratio - 1.0).abs() < 1e-3);
    }

    #[test]
    fn aspect_and_quarter_turns() {
        let square = Crop::default().with_aspect(1.0, 600, 400);
        let w = (square.right - square.left) * 600.0;
        let h = (square.bottom - square.top) * 400.0;
        assert!((w - h).abs() < 0.5 && (h - 400.0).abs() < 0.5);

        let wide = Crop { left: 0.1, top: 0.2, right: 0.5, bottom: 0.6, aspect: 1.5, ..Default::default() };
        let back = wide.rotated_quarter(true).rotated_quarter(false);
        assert!((back.left - wide.left).abs() < 1e-6 && (back.bottom - wide.bottom).abs() < 1e-6);
        assert!((back.aspect - 1.5).abs() < 1e-6);
        assert_eq!(wide.rotated_quarter(false).quarter_turns, 3);
    }

    #[test]
    fn dragging_stops_at_the_photo_edge() {
        let start = Crop { left: 0.2, top: 0.2, right: 0.6, bottom: 0.6, ..Default::default() };
        // Moving 0.6 to the left would leave the frame; it stops at the edge.
        let target = Crop { left: -0.4, right: 0.0, ..start };
        let moved = start.toward(target, 600, 400);
        assert!(moved.fits_photo(600, 400));
        assert!(moved.left.abs() < 1e-3 && (moved.right - 0.4).abs() < 1e-3);
        // A target that fits is taken as is.
        let inside = Crop { left: 0.1, right: 0.5, ..start };
        assert_eq!(start.toward(inside, 600, 400), inside);
        assert!((inside.pixel_aspect(600, 400) - 1.5).abs() < 1e-5);
    }

    #[test]
    fn sanitizes() {
        let c = Crop { quarter_turns: -1, angle: 90.0, left: 0.8, right: 0.5, ..Default::default() }.sanitized();
        assert_eq!(c.quarter_turns, 3);
        assert_eq!(c.angle, 45.0);
        assert!(c.right > c.left);
    }
}
