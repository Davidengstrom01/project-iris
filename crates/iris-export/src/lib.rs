//! Export: renders the full-resolution source through the pipeline and writes an sRGB
//! JPEG, PNG or TIFF with an embedded ICC profile.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use image::ImageEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use iris_core::{EditState, EncodedImage, ImageF, Samples, WhiteBalance};
use iris_render::color_transform::srgb_icc_profile;
use iris_render::{RenderOptions, render};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    #[default]
    Jpeg,
    Png,
    Tiff,
}

impl ExportFormat {
    /// Lower-case file extension without the dot ("jpg", "png", "tif").
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "jpg",
            ExportFormat::Png => "png",
            ExportFormat::Tiff => "tif",
        }
    }

    /// The format for a file name's extension (.jpg/.jpeg, .png, .tif/.tiff).
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(ExportFormat::Jpeg),
            "png" => Some(ExportFormat::Png),
            "tif" | "tiff" => Some(ExportFormat::Tiff),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportSettings {
    pub format: ExportFormat,
    /// 1-100
    pub jpeg_quality: u8,
    /// 0 = original resolution; otherwise resize so the long edge fits.
    pub long_edge: usize,
    /// PNG/TIFF: 8 or 16. JPEG is always 8.
    pub bits_per_channel: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self { format: ExportFormat::Jpeg, jpeg_quality: 92, long_edge: 0, bits_per_channel: 8 }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("Nothing to export")]
    Empty,
    #[error("Cannot write {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("Cannot encode image: {0}")]
    Encode(String),
}

fn encode_error(e: impl std::fmt::Display) -> ExportError {
    ExportError::Encode(e.to_string())
}

/// Writes an encoded sRGB image in the given format.
pub fn write_image(image: &EncodedImage, settings: &ExportSettings, out: &mut impl Write) -> Result<(), ExportError> {
    let (w, h) = (image.width as u32, image.height as u32);
    let icc = srgb_icc_profile();
    match (settings.format, &image.samples) {
        (ExportFormat::Jpeg, Samples::Eight(data)) => {
            let mut encoder = JpegEncoder::new_with_quality(out, settings.jpeg_quality.clamp(1, 100));
            encoder.set_icc_profile(icc).map_err(encode_error)?;
            encoder.write_image(data, w, h, image::ExtendedColorType::Rgb8).map_err(encode_error)
        }
        (ExportFormat::Png, samples) => {
            let mut encoder = PngEncoder::new_with_quality(out, CompressionType::Default, FilterType::Adaptive);
            encoder.set_icc_profile(icc).map_err(encode_error)?;
            match samples {
                Samples::Eight(data) => encoder.write_image(data, w, h, image::ExtendedColorType::Rgb8),
                Samples::Sixteen(data) => {
                    // PNG stores 16-bit samples big-endian; the encoder expects native-endian bytes.
                    let bytes: Vec<u8> = data.iter().flat_map(|v| v.to_ne_bytes()).collect();
                    encoder.write_image(&bytes, w, h, image::ExtendedColorType::Rgb16)
                }
            }
            .map_err(encode_error)
        }
        (ExportFormat::Tiff, samples) => {
            let mut seekable = std::io::Cursor::new(Vec::new());
            {
                let mut encoder = tiff::encoder::TiffEncoder::new(&mut seekable)
                    .map_err(encode_error)?
                    .with_compression(tiff::encoder::Compression::Lzw);
                macro_rules! write_tiff {
                    ($color:ty, $data:expr) => {{
                        let mut tiff_image = encoder.new_image::<$color>(w, h).map_err(encode_error)?;
                        tiff_image.encoder().write_tag(tiff::tags::Tag::IccProfile, &icc[..]).map_err(encode_error)?;
                        tiff_image.write_data($data).map_err(encode_error)
                    }};
                }
                match samples {
                    Samples::Eight(data) => write_tiff!(tiff::encoder::colortype::RGB8, data)?,
                    Samples::Sixteen(data) => write_tiff!(tiff::encoder::colortype::RGB16, data)?,
                }
            }
            out.write_all(seekable.get_ref()).map_err(encode_error)
        }
        (ExportFormat::Jpeg, Samples::Sixteen(_)) => Err(ExportError::Encode("JPEG is always 8-bit".into())),
    }
}

/// Renders the full-resolution source with the edits through the pipeline and writes it
/// as an sRGB file with an embedded ICC profile. The write is atomic: an existing file is
/// only replaced once the new one has been written completely.
pub fn export_image(
    full_resolution_source: &ImageF,
    as_shot: &WhiteBalance,
    edits: &EditState,
    settings: &ExportSettings,
    output_path: &Path,
) -> Result<(), ExportError> {
    if full_resolution_source.is_empty() {
        return Err(ExportError::Empty);
    }
    let options = RenderOptions {
        max_long_edge: settings.long_edge,
        bits_per_channel: if settings.format == ExportFormat::Jpeg { 8 } else { settings.bits_per_channel },
        ..Default::default()
    };
    let image = render(full_resolution_source, as_shot, edits, &options);

    let io_error = |source| ExportError::Io { path: output_path.display().to_string(), source };
    let dir = output_path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let file = tempfile::NamedTempFile::new_in(dir).map_err(io_error)?;
    let mut writer = BufWriter::new(file);
    write_image(&image, settings, &mut writer)?;
    let file = writer.into_inner().map_err(|e| io_error(e.into_error()))?;
    file.as_file().sync_all().map_err(io_error)?;
    // Exported files get the usual permissions rather than the temporary file's 0600.
    set_default_permissions(file.as_file());
    file.persist(output_path).map_err(|e| io_error(e.error))?;
    Ok(())
}

#[cfg(unix)]
fn set_default_permissions(file: &File) {
    use std::os::unix::fs::PermissionsExt;
    let _ = file.set_permissions(std::fs::Permissions::from_mode(0o644));
}

#[cfg(not(unix))]
fn set_default_permissions(_: &File) {}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageDecoder;

    const AS_SHOT: WhiteBalance = WhiteBalance { temperature: 5500.0, tint: 10.0 };

    fn solid(w: usize, h: usize, r: f32, g: f32, b: f32) -> ImageF {
        let mut image = ImageF::new(w, h);
        for px in image.pixels.as_chunks_mut::<3>().0 {
            px.copy_from_slice(&[r, g, b]);
        }
        image
    }

    #[test]
    fn jpeg_with_icc_and_resize() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.jpg");
        let settings = ExportSettings { long_edge: 150, ..Default::default() };
        export_image(&solid(300, 200, 0.18, 0.18, 0.18), &AS_SHOT, &EditState::new(AS_SHOT), &settings, &path).unwrap();
        let mut decoder =
            image::codecs::jpeg::JpegDecoder::new(std::io::BufReader::new(File::open(&path).unwrap())).unwrap();
        assert_eq!(decoder.dimensions(), (150, 100));
        let icc = decoder.icc_profile().unwrap().unwrap();
        assert_eq!(icc, srgb_icc_profile());
    }

    #[test]
    fn export_applies_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        let mut edits = EditState::new(AS_SHOT);
        edits.basic.exposure = 1.0;
        let settings = ExportSettings { format: ExportFormat::Png, ..Default::default() };
        export_image(&solid(8, 8, 0.09, 0.09, 0.09), &AS_SHOT, &edits, &settings, &path).unwrap();
        let image = image::open(&path).unwrap().to_rgb8();
        assert!((i32::from(image.get_pixel(4, 4)[0]) - 118).abs() <= 1);
    }

    #[test]
    fn sixteen_bit_png_and_tiff() {
        let dir = tempfile::tempdir().unwrap();
        for format in [ExportFormat::Png, ExportFormat::Tiff] {
            let path = dir.path().join(format!("out.{}", format.extension()));
            let settings = ExportSettings { format, bits_per_channel: 16, ..Default::default() };
            export_image(&solid(64, 32, 0.5, 0.25, 0.1), &AS_SHOT, &EditState::new(AS_SHOT), &settings, &path).unwrap();
            let image = image::open(&path).unwrap();
            assert_eq!((image.width(), image.height()), (64, 32));
            assert_eq!(image.color(), image::ColorType::Rgb16);
            // Same colour in both formats, and the same as the 8-bit render.
            let px = image.to_rgb16().get_pixel(10, 10).0;
            let eight = render(&solid(1, 1, 0.5, 0.25, 0.1), &AS_SHOT, &EditState::new(AS_SHOT), &Default::default());
            assert!((i32::from(px[0] >> 8) - i32::from(eight.data8()[0])).abs() <= 1, "{format:?}");
        }
    }

    #[test]
    fn failed_export_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-folder/out.jpg");
        let result =
            export_image(&solid(4, 4, 1.0, 1.0, 1.0), &AS_SHOT, &EditState::new(AS_SHOT), &Default::default(), &path);
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn formats_from_paths() {
        assert_eq!(ExportFormat::from_path(Path::new("a.JPEG")), Some(ExportFormat::Jpeg));
        assert_eq!(ExportFormat::from_path(Path::new("a.tiff")), Some(ExportFormat::Tiff));
        assert_eq!(ExportFormat::from_path(Path::new("a.webp")), None);
    }
}
