//! Rendered images on the GPU. Large images are split into tiles so that a full-resolution
//! photo fits within any GPU's maximum texture size.

use egui::{Color32, ColorImage, Painter, Pos2, Rect, TextureFilter, TextureHandle, TextureOptions, Vec2};
use iris_core::EncodedImage;

/// Largest tile edge in pixels; renderers with a smaller maximum texture size get smaller
/// tiles (see [`tile_size`]).
const TILE: usize = 4096;

/// The tile edge to use with this context's renderer.
pub fn tile_size(ctx: &egui::Context) -> usize {
    ctx.input(|i| i.max_texture_side).clamp(256, TILE)
}

/// An image prepared for upload (built on a worker thread).
pub struct TiledImage {
    pub width: usize,
    pub height: usize,
    tiles: Vec<([usize; 2], ColorImage)>,
}

impl TiledImage {
    /// From an 8-bit encoded sRGB image.
    pub fn from_encoded(image: &EncodedImage, tile: usize) -> Self {
        let (w, h) = (image.width, image.height);
        let data = image.data8();
        Self::build(w, h, tile, |x0, y0, tw, th| {
            let mut rgb = Vec::with_capacity(tw * th * 3);
            for y in y0..y0 + th {
                rgb.extend_from_slice(&data[(y * w + x0) * 3..(y * w + x0 + tw) * 3]);
            }
            ColorImage::from_rgb([tw, th], &rgb)
        })
    }

    /// From premultiplied colours.
    pub fn from_pixels(width: usize, height: usize, pixels: &[Color32], tile: usize) -> Self {
        Self::build(width, height, tile, |x0, y0, tw, th| {
            let mut tile = Vec::with_capacity(tw * th);
            for y in y0..y0 + th {
                tile.extend_from_slice(&pixels[y * width + x0..y * width + x0 + tw]);
            }
            ColorImage::new([tw, th], tile)
        })
    }

    fn build(
        width: usize,
        height: usize,
        size: usize,
        tile: impl Fn(usize, usize, usize, usize) -> ColorImage,
    ) -> Self {
        let mut tiles = Vec::new();
        for y0 in (0..height).step_by(size) {
            for x0 in (0..width).step_by(size) {
                let (tw, th) = (size.min(width - x0), size.min(height - y0));
                tiles.push(([x0, y0], tile(x0, y0, tw, th)));
            }
        }
        Self { width, height, tiles }
    }
}

/// How an image is sampled when drawn larger than its pixels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Magnify {
    /// Smooth (a preview that is temporarily enlarged).
    Smooth,
    /// Crisp pixels (inspecting detail at high zoom).
    Pixels,
}

/// A [`TiledImage`] uploaded to the GPU.
pub struct TiledTexture {
    pub width: usize,
    pub height: usize,
    tiles: Vec<(Rect, TextureHandle)>,
}

impl TiledTexture {
    pub fn upload(ctx: &egui::Context, name: &str, image: TiledImage, magnify: Magnify) -> Self {
        let options = TextureOptions {
            magnification: match magnify {
                Magnify::Smooth => TextureFilter::Linear,
                Magnify::Pixels => TextureFilter::Nearest,
            },
            minification: TextureFilter::Linear,
            mipmap_mode: Some(TextureFilter::Linear),
            ..TextureOptions::LINEAR
        };
        let tiles = image
            .tiles
            .into_iter()
            .enumerate()
            .map(|(i, ([x, y], tile))| {
                let rect = Rect::from_min_size(
                    Pos2::new(x as f32, y as f32),
                    Vec2::new(tile.width() as f32, tile.height() as f32),
                );
                (rect, ctx.load_texture(format!("{name}-{i}"), tile, options))
            })
            .collect();
        Self { width: image.width, height: image.height, tiles }
    }

    /// Draws the whole image into `dest` (screen points), clipped to `clip`.
    pub fn paint(&self, painter: &Painter, dest: Rect, clip: Rect) {
        let painter = painter.with_clip_rect(clip.intersect(painter.clip_rect()));
        let scale = Vec2::new(dest.width() / self.width as f32, dest.height() / self.height as f32);
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        for (rect, texture) in &self.tiles {
            let screen =
                Rect::from_min_max(dest.min + rect.min.to_vec2() * scale, dest.min + rect.max.to_vec2() * scale);
            if screen.intersects(clip) {
                painter.image(texture.id(), screen, uv, Color32::WHITE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::Samples;

    #[test]
    fn large_images_are_tiled() {
        let (w, h) = (TILE + 10, 20);
        let mut data = vec![0u8; w * h * 3];
        data[(5 * w + TILE + 3) * 3] = 200; // a red pixel in the second tile
        let image =
            TiledImage::from_encoded(&EncodedImage { width: w, height: h, samples: Samples::Eight(data) }, TILE);
        assert_eq!(image.tiles.len(), 2);
        assert_eq!(image.tiles[1].0, [TILE, 0]);
        assert_eq!(image.tiles[1].1.size, [10, 20]);
        assert_eq!(image.tiles[1].1.pixels[5 * 10 + 3], Color32::from_rgb(200, 0, 0));
    }
}
