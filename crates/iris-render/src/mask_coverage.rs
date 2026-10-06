//! How strongly a mask applies at each pixel of an image of a given size, 0..1.
//!
//! Gradients are evaluated analytically per pixel; brush strokes are rasterised once at
//! the image's resolution (only over the area they cover). Because mask geometry is
//! resolution-independent, the preview and the export get the same mask.

use std::f32::consts::PI;

use iris_core::{BrushMode, BrushStroke, LinearGradient, Mask, MaskType, RadialGradient};
use rayon::prelude::*;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance from p to the segment a-b.
fn segment_distance(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq > 0.0 { ((px - ax) * dx + (py - ay) * dy) / length_sq } else { 0.0 };
    let t = t.clamp(0.0, 1.0);
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

/// Half-open pixel rectangle.
#[derive(Clone, Copy)]
struct Rect {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

/// A rasterised brush layer covering [x0, x0 + w) x [y0, y0 + h); 0 elsewhere.
#[derive(Default)]
struct Layer {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
    values: Vec<f32>,
}

impl Layer {
    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn row(&self, y: usize) -> Option<&[f32]> {
        (y >= self.y0 && y < self.y0 + self.h && !self.is_empty())
            .then(|| &self.values[(y - self.y0) * self.w..(y - self.y0 + 1) * self.w])
    }
}

pub struct MaskCoverage {
    mask_type: MaskType,
    invert: bool,
    width: usize,
    height: usize,
    long_edge: f32,
    linear: LinearGradient,
    radial: RadialGradient,
    /// Add strokes, minus Erase strokes.
    paint: Layer,
    /// Subtract strokes, minus Erase strokes.
    subtract: Layer,
}

impl MaskCoverage {
    pub fn new(mask: &Mask, width: usize, height: usize) -> Self {
        let mut coverage = Self {
            mask_type: mask.mask_type,
            invert: mask.invert,
            width,
            height,
            long_edge: width.max(height) as f32,
            linear: mask.linear,
            radial: mask.radial,
            paint: Layer::default(),
            subtract: Layer::default(),
        };
        coverage.rasterize_strokes(&mask.strokes);
        coverage
    }

    fn stroke_bounds(&self, s: &BrushStroke) -> Rect {
        let (w, h) = (self.width as f32, self.height as f32);
        let r = s.radius * self.long_edge + 1.0;
        let (mut min_x, mut min_y) = (s.points[0].x * w, s.points[0].y * h);
        let (mut max_x, mut max_y) = (min_x, min_y);
        for p in &s.points {
            min_x = min_x.min(p.x * w);
            max_x = max_x.max(p.x * w);
            min_y = min_y.min(p.y * h);
            max_y = max_y.max(p.y * h);
        }
        Rect {
            x0: ((min_x - r).floor() as i64).max(0),
            y0: ((min_y - r).floor() as i64).max(0),
            x1: ((max_x + r).ceil() as i64).min(self.width as i64),
            y1: ((max_y + r).ceil() as i64).min(self.height as i64),
        }
    }

    /// Each layer covers the union of the strokes that can change it.
    fn allocate(&self, strokes: &[&BrushStroke], mode: BrushMode) -> Layer {
        let mut u = Rect { x0: self.width as i64, y0: self.height as i64, x1: 0, y1: 0 };
        for s in strokes.iter().filter(|s| s.mode == mode) {
            let b = self.stroke_bounds(s);
            u = Rect { x0: u.x0.min(b.x0), y0: u.y0.min(b.y0), x1: u.x1.max(b.x1), y1: u.y1.max(b.y1) };
        }
        if u.x1 <= u.x0 || u.y1 <= u.y0 {
            return Layer::default();
        }
        let (w, h) = ((u.x1 - u.x0) as usize, (u.y1 - u.y0) as usize);
        Layer { x0: u.x0 as usize, y0: u.y0 as usize, w, h, values: vec![0.0; w * h] }
    }

    fn rasterize_strokes(&mut self, strokes: &[BrushStroke]) {
        if strokes.is_empty() || self.width == 0 || self.height == 0 {
            return;
        }
        let strokes: Vec<&BrushStroke> = strokes.iter().filter(|s| !s.points.is_empty()).collect();
        self.paint = self.allocate(&strokes, BrushMode::Add);
        self.subtract = self.allocate(&strokes, BrushMode::Subtract);

        let mut stroke = Vec::new();
        for s in strokes {
            let is_empty = match s.mode {
                BrushMode::Add => self.paint.is_empty(),
                BrushMode::Subtract => self.subtract.is_empty(),
                BrushMode::Erase => self.paint.is_empty() && self.subtract.is_empty(), // nothing to erase
            };
            if is_empty {
                continue;
            }
            let b = self.stroke_bounds(s);
            if b.x1 <= b.x0 || b.y1 <= b.y0 {
                continue;
            }
            let w = (b.x1 - b.x0) as usize;

            // Coverage of this stroke alone: the strongest of its segments, so a stroke does
            // not build up where it crosses itself.
            let radius = s.radius * self.long_edge;
            let inner = (radius * (1.0 - s.feather)).min(radius - 1.0).max(0.0);
            let px: Vec<f32> = s.points.iter().map(|p| p.x * self.width as f32).collect();
            let py: Vec<f32> = s.points.iter().map(|p| p.y * self.height as f32).collect();
            let segments = (s.points.len() - 1).max(1);
            stroke.clear();
            stroke.resize(w * (b.y1 - b.y0) as usize, 0.0f32);
            stroke.par_chunks_mut(w).enumerate().for_each(|(row, out)| {
                let cy = (b.y0 + row as i64) as f32 + 0.5;
                for i in 0..segments {
                    let j = (i + 1).min(s.points.len() - 1);
                    if cy < py[i].min(py[j]) - radius || cy > py[i].max(py[j]) + radius {
                        continue;
                    }
                    let xa = b.x0.max((px[i].min(px[j]) - radius).floor() as i64);
                    let xb = b.x1.min((px[i].max(px[j]) + radius).ceil() as i64);
                    for x in xa..xb {
                        let d = segment_distance(x as f32 + 0.5, cy, px[i], py[i], px[j], py[j]);
                        if d < radius {
                            let o = &mut out[(x - b.x0) as usize];
                            *o = o.max(1.0 - smoothstep(inner, radius, d));
                        }
                    }
                }
            });

            // Composite the stroke into its layer(s).
            let composite = |layer: &mut Layer, erase: bool| {
                if layer.is_empty() {
                    return;
                }
                let (lx0, ly0) = (layer.x0 as i64, layer.y0 as i64);
                let y0 = b.y0.max(ly0);
                let y1 = b.y1.min(ly0 + layer.h as i64);
                let x0 = b.x0.max(lx0);
                let x1 = b.x1.min(lx0 + layer.w as i64);
                for y in y0..y1 {
                    let input = &stroke[(y - b.y0) as usize * w..];
                    let out = &mut layer.values[(y - ly0) as usize * layer.w..];
                    for x in x0..x1 {
                        let v = input[(x - b.x0) as usize] * s.opacity;
                        let o = &mut out[(x - lx0) as usize];
                        *o = if erase { *o * (1.0 - v) } else { *o + v * (1.0 - *o) };
                    }
                }
            };
            match s.mode {
                BrushMode::Add => composite(&mut self.paint, false),
                BrushMode::Subtract => composite(&mut self.subtract, false),
                BrushMode::Erase => {
                    composite(&mut self.paint, true);
                    composite(&mut self.subtract, true);
                }
            }
        }
    }

    /// Writes the coverage of row y to `out[0..width]`.
    pub fn row(&self, y: usize, out: &mut [f32]) {
        let out = &mut out[..self.width];
        let cy = y as f32 + 0.5;
        match self.mask_type {
            MaskType::Brush => out.fill(0.0),
            MaskType::Linear => {
                // Signed distance from the centre line, positive towards the affected side.
                let a = self.linear.angle * PI / 180.0;
                let (nx, ny) = (-a.sin(), -a.cos());
                let half = (self.linear.feather * self.long_edge / 2.0).max(0.5);
                let cx0 = self.linear.x * self.width as f32;
                let cy0 = self.linear.y * self.height as f32;
                for (x, o) in out.iter_mut().enumerate() {
                    let d = (x as f32 + 0.5 - cx0) * nx + (cy - cy0) * ny;
                    *o = smoothstep(-half, half, d);
                }
            }
            MaskType::Radial => {
                let r = &self.radial;
                let a = r.rotation * PI / 180.0;
                let (ux, uy) = (a.cos(), -a.sin()); // ellipse axes on screen
                let (vx, vy) = (a.sin(), a.cos());
                let rx = (r.width * self.long_edge / 2.0).max(0.5);
                let ry = (r.height * self.long_edge / 2.0).max(0.5);
                let edge0 = (1.0 - r.feather).min(1.0 - 1.0 / rx.min(ry));
                let cx0 = r.x * self.width as f32;
                let cy0 = r.y * self.height as f32;
                for (x, o) in out.iter_mut().enumerate() {
                    let (dx, dy) = (x as f32 + 0.5 - cx0, cy - cy0);
                    let (lx, ly) = ((dx * ux + dy * uy) / rx, (dx * vx + dy * vy) / ry);
                    *o = 1.0 - smoothstep(edge0, 1.0, (lx * lx + ly * ly).sqrt());
                }
            }
        }

        // Shape + paint (screen), inverted, minus Subtract strokes.
        if let Some(paint) = self.paint.row(y) {
            for (o, &p) in out[self.paint.x0..self.paint.x0 + self.paint.w].iter_mut().zip(paint) {
                *o += p * (1.0 - *o);
            }
        }
        if self.invert {
            for o in out.iter_mut() {
                *o = 1.0 - *o;
            }
        }
        if let Some(subtract) = self.subtract.row(y) {
            for (o, &s) in out[self.subtract.x0..self.subtract.x0 + self.subtract.w].iter_mut().zip(subtract) {
                *o *= 1.0 - s;
            }
        }
    }
}

/// The coverage of a mask as an 8-bit greyscale image (255 = full effect), for overlays.
pub fn render_mask_coverage(mask: &Mask, width: usize, height: usize) -> Vec<u8> {
    let mut result = vec![0u8; width * height];
    if width == 0 || height == 0 {
        return result;
    }
    let coverage = MaskCoverage::new(mask, width, height);
    result.par_chunks_mut(width).enumerate().for_each_init(
        || vec![0.0f32; width],
        |row, (y, out)| {
            coverage.row(y, row);
            for (o, &v) in out.iter_mut().zip(row.iter()) {
                *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        },
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::MaskPoint;

    fn at(mask: &Mask, w: usize, h: usize, x: usize, y: usize) -> f32 {
        let mut row = vec![0.0; w];
        MaskCoverage::new(mask, w, h).row(y, &mut row);
        row[x]
    }

    #[test]
    fn linear_gradient_coverage() {
        let mut mask = Mask::new(MaskType::Linear, &[]);
        mask.linear = LinearGradient { x: 0.5, y: 0.5, angle: 0.0, feather: 0.2 }; // effect above
        let a = |m: &Mask, y| at(m, 100, 100, 50, y);
        assert_eq!(a(&mask, 0), 1.0);
        assert!((a(&mask, 50) - 0.5).abs() < 0.05);
        assert_eq!(a(&mask, 99), 0.0);
        assert!(a(&mask, 42) > a(&mask, 45) && a(&mask, 45) > a(&mask, 55)); // smooth transition

        mask.linear.angle = 90.0; // rotated counter-clockwise: effect on the left
        assert_eq!(at(&mask, 100, 100, 0, 50), 1.0);
        assert_eq!(at(&mask, 100, 100, 99, 50), 0.0);

        mask.invert = true;
        assert_eq!(at(&mask, 100, 100, 0, 50), 0.0);
        assert_eq!(at(&mask, 100, 100, 99, 50), 1.0);
    }

    #[test]
    fn radial_gradient_coverage() {
        let mut mask = Mask::new(MaskType::Radial, &[]);
        mask.radial = RadialGradient { x: 0.5, y: 0.5, width: 0.6, height: 0.2, rotation: 0.0, feather: 0.3 };
        let a = |m: &Mask, x, y| at(m, 200, 200, x, y);
        assert_eq!(a(&mask, 100, 100), 1.0); // centre
        assert_eq!(a(&mask, 65, 100), 1.0); // inside along the long axis (60 px radius)
        let edge = a(&mask, 42, 100);
        assert!(edge > 0.0 && edge < 1.0); // in the feathered edge
        assert_eq!(a(&mask, 5, 100), 0.0); // outside
        assert_eq!(a(&mask, 100, 70), 0.0); // 30 px above the centre: beyond the 20 px half-height

        mask.radial.rotation = 90.0; // now tall
        assert_eq!(a(&mask, 100, 70), 1.0);
        assert_eq!(a(&mask, 40, 100), 0.0);
    }

    fn stroke(mode: BrushMode, y: f32, opacity: f32) -> BrushStroke {
        BrushStroke {
            mode,
            radius: 0.05,
            feather: 0.0,
            opacity,
            points: vec![MaskPoint::new(0.2, y), MaskPoint::new(0.8, y)],
        }
    }

    #[test]
    fn brush_strokes_add_subtract_and_erase() {
        let a = |m: &Mask, x, y| at(m, 100, 100, x, y);
        let mut brush = Mask::new(MaskType::Brush, &[]);
        assert_eq!(a(&brush, 50, 50), 0.0); // nothing painted
        brush.strokes = vec![stroke(BrushMode::Add, 0.5, 1.0)];
        assert_eq!(a(&brush, 50, 50), 1.0);
        assert_eq!(a(&brush, 50, 60), 0.0);
        assert_eq!(a(&brush, 10, 50), 0.0);

        // Opacity, and a stroke does not build up where it overlaps itself.
        brush.strokes = vec![stroke(BrushMode::Add, 0.5, 0.5)];
        brush.strokes[0].points.push(MaskPoint::new(0.5, 0.5));
        assert!((a(&brush, 50, 50) - 0.5).abs() < 0.01);

        // Erase removes earlier paint.
        brush.strokes = vec![stroke(BrushMode::Add, 0.5, 1.0), stroke(BrushMode::Erase, 0.5, 1.0)];
        assert_eq!(a(&brush, 50, 50), 0.0);

        // Subtract removes the gradient underneath; erasing it brings the gradient back.
        let mut linear = Mask::new(MaskType::Linear, &[]);
        linear.linear = LinearGradient { x: 0.5, y: 1.5, angle: 0.0, feather: 0.01 }; // effect everywhere
        assert_eq!(a(&linear, 50, 50), 1.0);
        linear.strokes = vec![stroke(BrushMode::Subtract, 0.5, 1.0)];
        assert_eq!(a(&linear, 50, 50), 0.0);
        assert_eq!(a(&linear, 50, 20), 1.0);
        linear.strokes.push(stroke(BrushMode::Erase, 0.5, 1.0));
        assert_eq!(a(&linear, 50, 50), 1.0);

        // Invert flips paint, but Subtract always removes.
        brush.strokes = vec![stroke(BrushMode::Add, 0.5, 1.0), stroke(BrushMode::Subtract, 0.2, 1.0)];
        brush.invert = true;
        assert_eq!(a(&brush, 50, 50), 0.0);
        assert_eq!(a(&brush, 50, 80), 1.0);
        assert_eq!(a(&brush, 50, 20), 0.0);
    }

    #[test]
    fn coverage_matches_across_resolutions() {
        let mut mask = Mask::new(MaskType::Radial, &[]);
        mask.radial = RadialGradient { x: 0.4, y: 0.55, width: 0.5, height: 0.3, rotation: 30.0, feather: 0.6 };
        mask.strokes = vec![BrushStroke {
            radius: 0.04,
            points: vec![MaskPoint::new(0.1, 0.1), MaskPoint::new(0.5, 0.3), MaskPoint::new(0.9, 0.2)],
            ..Default::default()
        }];
        let small = render_mask_coverage(&mask, 300, 200);
        let large = render_mask_coverage(&mask, 1500, 1000);
        for fx in [0.1, 0.3, 0.45, 0.6, 0.8] {
            for fy in [0.15, 0.3, 0.55, 0.7] {
                let (sx, sy) = ((fx * 300.0) as usize, (fy * 200.0) as usize);
                let a = i32::from(small[sy * 300 + sx]);
                let b = i32::from(large[(sy * 5 + 2) * 1500 + sx * 5 + 2]);
                assert!((a - b).abs() <= 8, "{a} vs {b} at {fx},{fy}");
            }
        }
    }
}
