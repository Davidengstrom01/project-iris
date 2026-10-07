//! Clone and heal strokes, applied to the scene-linear source before anything is developed,
//! so cloned pixels go through exposure, masks, curves and sharpening like the rest.
//!
//! Each stroke reads from the image as the earlier strokes left it (like painting).
//!   Clone: the destination becomes the source pixels.
//!   Heal:  the destination gets the source's texture with the destination's colour and
//!          light: source + D, where D is the smooth (harmonic) correction that matches the
//!          destination around the stroke's edge. D is solved on a coarse grid (it is smooth
//!          by construction), which keeps it fast and the same at every resolution.

use iris_core::{ImageF, RetouchMode, RetouchStroke};
use rayon::prelude::*;

/// Distance between dabs along a stroke, as a fraction of the radius (for flow).
const DAB_SPACING: f32 = 0.25;
/// Cells along the long side of the grid the healing correction is solved on.
const HEAL_GRID: usize = 64;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance from p to the segment a-b.
fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq > 0.0 { ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_sq } else { 0.0 };
    let t = t.clamp(0.0, 1.0);
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// A rectangle of pixels, half-open.
#[derive(Clone, Copy, Debug)]
struct Region {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Region {
    fn width(&self) -> usize {
        self.x1 - self.x0
    }
    fn height(&self) -> usize {
        self.y1 - self.y0
    }
}

/// Bilinear sample of the image at a continuous position (pixel centres at i + 0.5),
/// clamped at the edges.
fn sample(image: &ImageF, x: f32, y: f32) -> [f32; 3] {
    let fx = (x - 0.5).clamp(0.0, (image.width - 1) as f32);
    let fy = (y - 0.5).clamp(0.0, (image.height - 1) as f32);
    let (ix, iy) = (fx as usize, fy as usize);
    let (ix1, iy1) = ((ix + 1).min(image.width - 1), (iy + 1).min(image.height - 1));
    let (tx, ty) = (fx - ix as f32, fy - iy as f32);
    let at = |x: usize, y: usize, c: usize| image.pixels[(y * image.width + x) * 3 + c];
    std::array::from_fn(|c| {
        let top = at(ix, iy, c) + (at(ix1, iy, c) - at(ix, iy, c)) * tx;
        let bottom = at(ix, iy1, c) + (at(ix1, iy1, c) - at(ix, iy1, c)) * tx;
        top + (bottom - top) * ty
    })
}

/// One stroke in pixel units.
struct Stroke {
    points: Vec<(f32, f32)>,
    offset: (f32, f32),
    radius: f32,
    inner: f32,
    opacity: f32,
    flow: f32,
}

impl Stroke {
    fn new(s: &RetouchStroke, width: usize, height: usize) -> Self {
        let (w, h) = (width as f32, height as f32);
        let radius = (s.radius * w.max(h)).max(0.5);
        Self {
            points: s.points.iter().map(|p| (p.x * w, p.y * h)).collect(),
            offset: (s.offset.x * w, s.offset.y * h),
            radius,
            inner: (radius * s.hardness).min(radius - 1.0).max(0.0),
            opacity: s.opacity,
            flow: s.flow,
        }
    }

    /// The pixels the stroke can change, with `margin` around them.
    fn region(&self, margin: f32, width: usize, height: usize) -> Region {
        let r = self.radius + 1.0 + margin;
        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for &(x, y) in &self.points {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        let clamp = |v: f32, limit: usize| v.clamp(0.0, limit as f32) as usize;
        Region {
            x0: clamp((min_x - r).floor(), width),
            y0: clamp((min_y - r).floor(), height),
            x1: clamp((max_x + r).ceil(), width),
            y1: clamp((max_y + r).ceil(), height),
        }
    }

    /// How strongly the stroke applies at a pixel centre, 0..1.
    fn alpha(&self, x: f32, y: f32) -> f32 {
        let segments = self.points.len().saturating_sub(1).max(1);
        let mut d = f32::INFINITY;
        for i in 0..segments {
            let j = (i + 1).min(self.points.len() - 1);
            d = d.min(segment_distance((x, y), self.points[i], self.points[j]));
        }
        if d >= self.radius {
            return 0.0;
        }
        let shape = 1.0 - smoothstep(self.inner, self.radius, d);
        // Flow: each dab adds `flow`; a pixel is covered by more dabs the closer it is to
        // the path (the chord of the brush circle), and by one at least.
        let built_up = if self.flow >= 1.0 {
            1.0
        } else {
            let chord = 2.0 * (self.radius * self.radius - d * d).max(0.0).sqrt();
            let dabs = 1.0 + chord / (DAB_SPACING * self.radius);
            1.0 - (1.0 - self.flow).powf(dabs)
        };
        shape * self.opacity * built_up
    }
}

/// Applies the strokes to `image` in order.
pub fn apply(image: &mut ImageF, strokes: &[RetouchStroke]) {
    if image.is_empty() {
        return;
    }
    for s in strokes.iter().filter(|s| !s.points.is_empty()) {
        let stroke = Stroke::new(s, image.width, image.height);
        match s.mode {
            RetouchMode::Clone => clone(image, &stroke),
            RetouchMode::Heal => heal(image, &stroke),
        }
    }
}

/// The source pixels for a region (read before the stroke changes anything).
fn source_pixels(image: &ImageF, stroke: &Stroke, region: Region) -> Vec<f32> {
    let w = region.width();
    let mut source = vec![0.0f32; w * region.height() * 3];
    source.par_chunks_mut(w * 3).enumerate().for_each(|(i, row)| {
        let y = (region.y0 + i) as f32 + 0.5 + stroke.offset.1;
        for (j, p) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let x = (region.x0 + j) as f32 + 0.5 + stroke.offset.0;
            p.copy_from_slice(&sample(image, x, y));
        }
    });
    source
}

/// Blends `result` (a region's worth of pixels) into the image by the stroke's alpha.
fn blend(image: &mut ImageF, stroke: &Stroke, region: Region, result: &[f32]) {
    let (w, iw) = (region.width(), image.width);
    image.pixels.par_chunks_mut(iw * 3).enumerate().skip(region.y0).take(region.height()).for_each(|(y, row)| {
        let r = &result[(y - region.y0) * w * 3..];
        for x in region.x0..region.x1 {
            let a = stroke.alpha(x as f32 + 0.5, y as f32 + 0.5);
            if a <= 0.0 {
                continue;
            }
            for c in 0..3 {
                let dest = &mut row[x * 3 + c];
                *dest = (*dest + (r[(x - region.x0) * 3 + c] - *dest) * a).max(0.0);
            }
        }
    });
}

fn clone(image: &mut ImageF, stroke: &Stroke) {
    let region = stroke.region(0.0, image.width, image.height);
    if region.width() == 0 || region.height() == 0 {
        return;
    }
    let source = source_pixels(image, stroke, region);
    blend(image, stroke, region, &source);
}

fn heal(image: &mut ImageF, stroke: &Stroke) {
    // A margin of known pixels around the stroke, at least two grid cells wide.
    let core = stroke.region(0.0, image.width, image.height);
    if core.width() == 0 || core.height() == 0 {
        return;
    }
    let cell = core.width().max(core.height()).div_ceil(HEAL_GRID - 4).max(1);
    let region = stroke.region((2 * cell + 2) as f32, image.width, image.height);
    let (w, h) = (region.width(), region.height());
    let source = source_pixels(image, stroke, region);

    // D0 = destination - source everywhere; it is known where the stroke does not reach.
    let mut known_sum = vec![[0.0f32; 3]; 0];
    let (gw, gh) = (w.div_ceil(cell), h.div_ceil(cell));
    known_sum.resize(gw * gh, [0.0; 3]);
    let mut counts = vec![0u32; gw * gh];
    let mut inside = vec![false; gw * gh];
    for y in 0..h {
        for x in 0..w {
            let g = (y / cell) * gw + x / cell;
            let (px, py) = ((region.x0 + x) as f32 + 0.5, (region.y0 + y) as f32 + 0.5);
            if stroke.alpha(px, py) > 0.0 {
                inside[g] = true;
            }
            let i = ((region.y0 + y) * image.width + region.x0 + x) * 3;
            for c in 0..3 {
                known_sum[g][c] += image.pixels[i + c] - source[(y * w + x) * 3 + c];
            }
            counts[g] += 1;
        }
    }
    let mut d: Vec<[f32; 3]> = known_sum.iter().zip(&counts).map(|(s, &n)| s.map(|v| v / n.max(1) as f32)).collect();
    // Unknown cells start at the mean of the known ones, then relax to a harmonic surface.
    let known: Vec<usize> = (0..gw * gh).filter(|&g| !inside[g]).collect();
    if known.is_empty() {
        return clone(image, stroke);
    }
    let mean: [f32; 3] = std::array::from_fn(|c| known.iter().map(|&g| d[g][c]).sum::<f32>() / known.len() as f32);
    for g in (0..gw * gh).filter(|&g| inside[g]) {
        d[g] = mean;
    }
    const OMEGA: f32 = 1.8;
    for _ in 0..4000 {
        let mut change = 0.0f32;
        for gy in 0..gh {
            for gx in 0..gw {
                let g = gy * gw + gx;
                if !inside[g] {
                    continue;
                }
                let mut sum = [0.0f32; 3];
                let mut n = 0.0;
                for (nx, ny) in [(gx.wrapping_sub(1), gy), (gx + 1, gy), (gx, gy.wrapping_sub(1)), (gx, gy + 1)] {
                    if nx < gw && ny < gh {
                        let v = d[ny * gw + nx];
                        for c in 0..3 {
                            sum[c] += v[c];
                        }
                        n += 1.0;
                    }
                }
                for c in 0..3 {
                    let next = d[g][c] + OMEGA * (sum[c] / n - d[g][c]);
                    change = change.max((next - d[g][c]).abs());
                    d[g][c] = next;
                }
            }
        }
        if change < 1e-6 {
            break;
        }
    }

    // Result = source + D (bilinear between cell centres).
    let mut result = source;
    result.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        let fy = ((y as f32 + 0.5) / cell as f32 - 0.5).clamp(0.0, (gh - 1) as f32);
        let (gy0, ty) = (fy as usize, fy.fract());
        let gy1 = (gy0 + 1).min(gh - 1);
        for (x, p) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
            let fx = ((x as f32 + 0.5) / cell as f32 - 0.5).clamp(0.0, (gw - 1) as f32);
            let (gx0, tx) = (fx as usize, fx.fract());
            let gx1 = (gx0 + 1).min(gw - 1);
            for c in 0..3 {
                let top = d[gy0 * gw + gx0][c] + (d[gy0 * gw + gx1][c] - d[gy0 * gw + gx0][c]) * tx;
                let bottom = d[gy1 * gw + gx0][c] + (d[gy1 * gw + gx1][c] - d[gy1 * gw + gx0][c]) * tx;
                p[c] += top + (bottom - top) * ty;
            }
        }
    });
    blend(image, stroke, region, &result);
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::MaskPoint;

    /// Brightness rising left to right, with a fine checker texture.
    fn textured(w: usize, h: usize) -> ImageF {
        let mut image = ImageF::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let base = 0.1 + 0.4 * x as f32 / w as f32;
                let texture = if (x / 2 + y / 2) % 2 == 0 { 1.05 } else { 0.95 };
                image.row_mut(y)[x * 3..x * 3 + 3].fill(base * texture);
            }
        }
        image
    }

    fn dab(mode: RetouchMode, at: (f32, f32), offset: (f32, f32), radius: f32) -> RetouchStroke {
        RetouchStroke {
            mode,
            offset: MaskPoint::new(offset.0, offset.1),
            radius,
            hardness: 1.0,
            opacity: 1.0,
            flow: 1.0,
            points: vec![MaskPoint::new(at.0, at.1)],
        }
    }

    fn px(image: &ImageF, x: usize, y: usize) -> f32 {
        image.row(y)[x * 3]
    }

    #[test]
    fn clone_copies_the_source_and_nothing_else() {
        let original = textured(200, 100);
        let mut image = original.clone();
        // Copy from 50 px to the left into a 10 px dab at (150, 50).
        apply(&mut image, &[dab(RetouchMode::Clone, (0.75, 0.5), (-0.25, 0.0), 0.05)]);
        assert_eq!(px(&image, 150, 50), px(&original, 100, 50));
        assert_eq!(px(&image, 154, 47), px(&original, 104, 47));
        assert_eq!(px(&image, 170, 50), px(&original, 170, 50)); // outside the dab
        assert_eq!(px(&image, 100, 50), px(&original, 100, 50)); // the source is untouched
    }

    #[test]
    fn opacity_and_flow_blend_partly() {
        let original = textured(200, 100);
        let blended = |opacity: f32, flow: f32| {
            let mut image = original.clone();
            let mut s = dab(RetouchMode::Clone, (0.75, 0.5), (-0.5, 0.0), 0.05);
            s.opacity = opacity;
            s.flow = flow;
            apply(&mut image, &[s]);
            px(&image, 150, 50)
        };
        let (dest, source) = (px(&original, 150, 50), px(&original, 50, 50));
        assert!((blended(0.5, 1.0) - (dest + source) / 2.0).abs() < 1e-5);
        let low_flow = blended(1.0, 0.2);
        assert!(low_flow > source && low_flow < dest); // partly built up
    }

    #[test]
    fn heal_keeps_the_destination_brightness() {
        // A dark blemish on the bright side; healed from the darker left side.
        let mut image = textured(200, 100);
        let clean = image.clone();
        for y in 45..55 {
            for x in 145..155 {
                image.row_mut(y)[x * 3..x * 3 + 3].fill(0.02);
            }
        }
        let mut healed = image.clone();
        apply(&mut healed, &[dab(RetouchMode::Heal, (0.75, 0.5), (-0.5, 0.0), 0.06)]);
        // The blemish is gone and the brightness matches the surroundings (not the source's).
        let mean = |img: &ImageF| {
            (45..55).flat_map(|y| (145..155).map(move |x| (x, y))).map(|(x, y)| px(img, x, y)).sum::<f32>() / 100.0
        };
        assert!((mean(&healed) - mean(&clean)).abs() / mean(&clean) < 0.05, "{} vs {}", mean(&healed), mean(&clean));
        // A plain clone would bring the darker source brightness.
        let mut cloned = image.clone();
        apply(&mut cloned, &[dab(RetouchMode::Clone, (0.75, 0.5), (-0.5, 0.0), 0.06)]);
        assert!((mean(&cloned) - mean(&clean)).abs() / mean(&clean) > 0.3);
        // The texture comes along.
        assert!((px(&healed, 150, 50) - px(&healed, 152, 50)).abs() > 0.01);
    }

    #[test]
    fn strokes_apply_in_order_and_paths_are_covered() {
        let original = textured(200, 100);
        let mut image = original.clone();
        let mut line = dab(RetouchMode::Clone, (0.6, 0.3), (0.0, 0.4), 0.02);
        line.points.push(MaskPoint::new(0.9, 0.3));
        apply(&mut image, &[line]);
        for x in [120, 150, 180] {
            assert_eq!(px(&image, x, 30), px(&original, x, 70));
        }
    }

    #[test]
    fn heal_is_similar_at_preview_and_full_resolution() {
        let blemished = |w: usize, h: usize| {
            let mut image = textured(w, h);
            for y in h * 45 / 100..h * 55 / 100 {
                for x in w * 145 / 200..w * 155 / 200 {
                    image.row_mut(y)[x * 3..x * 3 + 3].fill(0.02);
                }
            }
            image
        };
        let stroke = [dab(RetouchMode::Heal, (0.75, 0.5), (-0.5, 0.0), 0.06)];
        let (mut small, mut large) = (blemished(200, 100), blemished(800, 400));
        apply(&mut small, &stroke);
        apply(&mut large, &stroke);
        let mean = |img: &ImageF, scale: usize| {
            let (x0, y0) = (145 * scale, 47 * scale);
            (0..6 * scale)
                .flat_map(|dy| (0..10 * scale).map(move |dx| (x0 + dx, y0 + dy)))
                .map(|(x, y)| px(img, x, y))
                .sum::<f32>()
                / (60 * scale * scale) as f32
        };
        assert!((mean(&small, 1) - mean(&large, 4)).abs() < 0.02);
    }
}
