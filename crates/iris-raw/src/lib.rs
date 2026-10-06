//! RAW decoding through LibRaw. The file is opened read-only and never modified.
//!
//! All unsafe code of Project Iris lives here: a small, safe wrapper around LibRaw's C API.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::ptr::NonNull;

use iris_core::color::{D65, Mat3, chromaticity, inverse, mul_vec, white_balance_from_white_point};
use iris_core::{ImageF, PhotoMetadata, WhiteBalance};
use libraw_sys as sys;
use rayon::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeQuality {
    /// Half-size demosaic: ~4x faster, used for the first on-screen preview.
    Preview,
    /// Full-resolution demosaic: used for 100% view and export.
    Full,
}

/// A decoded photo.
#[derive(Clone, Debug, Default)]
pub struct DecodedRaw {
    /// Linear Rec.2020, camera white balance applied, orientation applied.
    pub image: ImageF,
    pub metadata: PhotoMetadata,
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("Decoding cancelled")]
    Cancelled,
    #[error("{step}: {message}")]
    LibRaw { step: &'static str, message: String },
    #[error("Unsupported RAW image layout")]
    UnsupportedLayout,
    #[error("Invalid file path")]
    InvalidPath,
}

/// Lower-case RAW file extensions (without the dot) that Project Iris offers to open.
pub const RAW_FILE_EXTENSIONS: &[&str] = &[
    "arw", "srf", "sr2", "cr2", "cr3", "crw", "nef", "nrw", "raf", "dng", "orf", "rw2", "pef", "srw", "3fr", "iiq",
    "erf", "kdc", "mos", "rwl",
];

/// Whether the path has one of [`RAW_FILE_EXTENSIONS`].
pub fn is_raw_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| RAW_FILE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

const OUTPUT_COLOR_REC2020: c_int = 8; // LibRaw/dcraw "-o 8"

/// Owns a LibRaw handle.
struct Handle(NonNull<sys::libraw_data_t>);

impl Handle {
    fn new() -> Option<Self> {
        // SAFETY: libraw_init has no preconditions; a null result means out of memory.
        NonNull::new(unsafe { sys::libraw_init(0) }).map(Handle)
    }

    fn data(&self) -> &sys::libraw_data_t {
        // SAFETY: the pointer is valid until libraw_close in Drop, and LibRaw does not
        // write to it concurrently (we only call into LibRaw through &mut self).
        unsafe { self.0.as_ref() }
    }

    fn data_mut(&mut self) -> &mut sys::libraw_data_t {
        // SAFETY: as above, and we hold the only reference.
        unsafe { self.0.as_mut() }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from libraw_init and is closed exactly once.
        unsafe { sys::libraw_close(self.0.as_ptr()) }
    }
}

/// Owns an image from libraw_dcraw_make_mem_image.
struct ProcessedImage(NonNull<sys::libraw_processed_image_t>);

impl Drop for ProcessedImage {
    fn drop(&mut self) {
        // SAFETY: allocated by LibRaw and freed exactly once.
        unsafe { sys::libraw_dcraw_clear_mem(self.0.as_ptr()) }
    }
}

fn check(result: c_int, step: &'static str) -> Result<(), DecodeError> {
    if result == sys::LibRaw_errors_LIBRAW_CANCELLED_BY_CALLBACK {
        return Err(DecodeError::Cancelled);
    }
    if result != sys::LibRaw_errors_LIBRAW_SUCCESS {
        // SAFETY: libraw_strerror returns a static, NUL-terminated string.
        let message = unsafe { CStr::from_ptr(sys::libraw_strerror(result)) }.to_string_lossy().into_owned();
        return Err(DecodeError::LibRaw { step, message });
    }
    Ok(())
}

type CancelCheck<'a> = &'a (dyn Fn() -> bool + Sync);

unsafe extern "C" fn progress_callback(data: *mut c_void, _: sys::LibRaw_progress, _: c_int, _: c_int) -> c_int {
    // SAFETY: `data` points at the CancelCheck owned by decode(), which outlives the handle's
    // use of the callback.
    let cancelled = unsafe { &*(data as *const CancelCheck) };
    c_int::from(cancelled())
}

fn c_string(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).trim_end().to_owned()
}

fn exif_orientation_from_flip(flip: c_int) -> i32 {
    match flip {
        3 => 3,
        5 => 8,
        6 => 6,
        _ => 1,
    }
}

/// The camera's as-shot white balance: the colour a neutral surface had in camera RGB
/// (inverse of the WB multipliers), converted to XYZ through the camera matrix.
fn as_shot_white_balance(color: &sys::libraw_colordata_t) -> WhiteBalance {
    let mul = if color.cam_mul[..3].iter().all(|&m| m > 0.0) { &color.cam_mul } else { &color.pre_mul };
    let c = &color.cam_xyz;
    let xyz_to_camera: Mat3 = [
        c[0][0], c[0][1], c[0][2], //
        c[1][0], c[1][1], c[1][2], //
        c[2][0], c[2][1], c[2][2],
    ]
    .map(f64::from);
    let camera_to_xyz = inverse(&xyz_to_camera);
    if mul[..3].iter().all(|&m| m > 0.0) && camera_to_xyz[0].is_finite() {
        let neutral = [1.0 / f64::from(mul[0]), 1.0 / f64::from(mul[1]), 1.0 / f64::from(mul[2])];
        let xyz = mul_vec(&camera_to_xyz, &neutral);
        let sum = xyz[0] + xyz[1] + xyz[2];
        if sum.is_finite() && sum > 0.0 && xyz[1] > 0.0 {
            return white_balance_from_white_point(chromaticity(&xyz));
        }
    }
    white_balance_from_white_point(D65)
}

fn extract_metadata(data: &sys::libraw_data_t) -> PhotoMetadata {
    let mut lens = c_string(&data.lens.Lens);
    if lens.is_empty() {
        lens = c_string(&data.lens.makernotes.Lens);
    }
    let swapped = data.sizes.flip & 4 != 0;
    let (w, h) = (usize::from(data.sizes.width), usize::from(data.sizes.height));
    PhotoMetadata {
        make: c_string(&data.idata.make),
        model: c_string(&data.idata.model),
        lens,
        iso: data.other.iso_speed,
        shutter_seconds: data.other.shutter,
        aperture: data.other.aperture,
        focal_length_mm: data.other.focal_len,
        timestamp: data.other.timestamp,
        orientation: exif_orientation_from_flip(data.sizes.flip),
        width: if swapped { h } else { w },
        height: if swapped { w } else { h },
        as_shot: as_shot_white_balance(&data.color),
    }
}

/// Decodes a RAW file. `cancelled` is polled from the decoding thread; when it returns
/// true decoding stops with [`DecodeError::Cancelled`].
pub fn decode(
    path: &Path,
    quality: DecodeQuality,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<DecodedRaw, DecodeError> {
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| DecodeError::InvalidPath)?;

    // Declared before `raw` so that it outlives the handle on every return path (locals
    // are dropped in reverse order).
    let mut cancel_check: CancelCheck = cancelled;
    let mut raw =
        Handle::new().ok_or(DecodeError::LibRaw { step: "Cannot start LibRaw", message: "out of memory".into() })?;
    // SAFETY: the callback data pointer stays valid for as long as `raw` exists.
    unsafe {
        sys::libraw_set_progress_handler(
            raw.0.as_ptr(),
            Some(progress_callback),
            (&mut cancel_check as *mut CancelCheck).cast(),
        );
    }

    // Scene-linear output in a wide-gamut working space; no automatic brightening,
    // so the pipeline sees the sensor's real exposure. Clipped highlights stay neutral.
    let p = &mut raw.data_mut().params;
    p.output_color = OUTPUT_COLOR_REC2020;
    p.output_bps = 16;
    p.gamm[0] = 1.0;
    p.gamm[1] = 1.0;
    p.no_auto_bright = 1;
    p.use_camera_wb = 1;
    p.use_camera_matrix = 1;
    p.highlight = 0;
    p.half_size = c_int::from(quality == DecodeQuality::Preview);

    // SAFETY (all LibRaw calls below): `raw` is a valid handle and `c_path` a valid C string.
    check(unsafe { sys::libraw_open_file(raw.0.as_ptr(), c_path.as_ptr()) }, "Cannot open RAW file")?;
    let mut metadata = extract_metadata(raw.data());

    check(unsafe { sys::libraw_unpack(raw.0.as_ptr()) }, "Cannot read RAW data")?;
    if cancelled() {
        return Err(DecodeError::Cancelled);
    }
    check(unsafe { sys::libraw_dcraw_process(raw.0.as_ptr()) }, "Cannot process RAW data")?;

    let mut error: c_int = 0;
    let processed = NonNull::new(unsafe { sys::libraw_dcraw_make_mem_image(raw.0.as_ptr(), &mut error) });
    check(error, "Cannot create image")?;
    let processed = ProcessedImage(processed.ok_or(DecodeError::UnsupportedLayout)?);
    // SAFETY: LibRaw returned a valid image that lives until `processed` is dropped.
    let header = unsafe { processed.0.as_ref() };
    if header.type_ != sys::LibRaw_image_formats_LIBRAW_IMAGE_BITMAP || header.colors != 3 || header.bits != 16 {
        return Err(DecodeError::UnsupportedLayout);
    }
    let (width, height) = (usize::from(header.width), usize::from(header.height));
    let samples = width * height * 3;
    if (header.data_size as usize) < samples * 2 {
        return Err(DecodeError::UnsupportedLayout);
    }
    // SAFETY: `data` is a flexible array of `data_size` bytes holding native-endian u16
    // samples; LibRaw allocates it with malloc, so it is suitably aligned for u16.
    let src: &[u16] = unsafe { std::slice::from_raw_parts(header.data.as_ptr().cast::<u16>(), samples) };

    let mut image = ImageF::new(width, height);
    const SCALE: f32 = 1.0 / 65535.0;
    image.pixels.par_chunks_mut(1 << 16).zip(src.par_chunks(1 << 16)).for_each(|(out, input)| {
        for (o, &i) in out.iter_mut().zip(input) {
            *o = f32::from(i) * SCALE;
        }
    });

    if quality == DecodeQuality::Full {
        metadata.width = image.width;
        metadata.height = image.height;
    }
    Ok(DecodedRaw { image, metadata })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_raw() -> Option<std::path::PathBuf> {
        std::env::var_os("IRIS_TEST_RAW").map(Into::into)
    }

    #[test]
    fn recognises_raw_extensions() {
        assert!(is_raw_file(Path::new("/a/b/photo.ARW")));
        assert!(is_raw_file(Path::new("x.nef")));
        assert!(!is_raw_file(Path::new("x.jpg")));
        assert!(!is_raw_file(Path::new("arw")));
    }

    #[test]
    fn missing_file_is_an_error() {
        let err = decode(Path::new("/nonexistent/photo.ARW"), DecodeQuality::Preview, &|| false).unwrap_err();
        assert!(matches!(err, DecodeError::LibRaw { .. }), "{err}");
    }

    #[test]
    fn decodes_a_raw_file() {
        let Some(path) = test_raw() else {
            eprintln!("Set IRIS_TEST_RAW to a RAW file to run this test");
            return;
        };
        let decoded = decode(&path, DecodeQuality::Preview, &|| false).unwrap();
        assert!(decoded.image.width > 100 && decoded.image.height > 100);
        assert!(decoded.metadata.width >= decoded.image.width);
        assert!(!decoded.metadata.make.is_empty());
        let mean: f32 = decoded.image.pixels.iter().sum::<f32>() / decoded.image.pixels.len() as f32;
        assert!(mean > 0.001 && mean < 1.0);
        assert!((2000.0..=15000.0).contains(&decoded.metadata.as_shot.temperature));
    }

    #[test]
    fn decoding_can_be_cancelled() {
        let Some(path) = test_raw() else { return };
        let err = decode(&path, DecodeQuality::Full, &|| true).unwrap_err();
        assert!(matches!(err, DecodeError::Cancelled));
    }
}
