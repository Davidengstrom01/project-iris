//! Output colour transform: linear Rec.2020 working space -> encoded sRGB, via LittleCMS.

use std::sync::OnceLock;

use lcms2::{
    CIExyY, CIExyYTRIPLE, DisallowCache, Flags, GlobalContext, Intent, PixelFormat, Profile, ToneCurve, Transform,
};

fn linear_rec2020_profile() -> Profile {
    let d65 = CIExyY { x: 0.3127, y: 0.3290, Y: 1.0 };
    let primaries = CIExyYTRIPLE {
        Red: CIExyY { x: 0.708, y: 0.292, Y: 1.0 },
        Green: CIExyY { x: 0.170, y: 0.797, Y: 1.0 },
        Blue: CIExyY { x: 0.131, y: 0.046, Y: 1.0 },
    };
    let linear = ToneCurve::new(1.0);
    Profile::new_rgb(&d65, &primaries, &[&linear, &linear, &linear]).expect("cannot create the working-space profile")
}

type Transform8 = Transform<[f32; 3], [u8; 3], GlobalContext, DisallowCache>;
type Transform16 = Transform<[f32; 3], [u16; 3], GlobalContext, DisallowCache>;

/// Shared sRGB transforms. NO_CACHE makes them safe to use from several threads at once.
struct Transforms {
    srgb8: Transform8,
    srgb16: Transform16,
}

fn transforms() -> &'static Transforms {
    static TRANSFORMS: OnceLock<Transforms> = OnceLock::new();
    TRANSFORMS.get_or_init(|| {
        let working = linear_rec2020_profile();
        let srgb = Profile::new_srgb();
        let intent = Intent::RelativeColorimetric;
        Transforms {
            srgb8: Transform::new_flags_context(
                GlobalContext::new(),
                &working,
                PixelFormat::RGB_FLT,
                &srgb,
                PixelFormat::RGB_8,
                intent,
                Flags::NO_CACHE,
            )
            .expect("cannot create the output colour transform"),
            srgb16: Transform::new_flags_context(
                GlobalContext::new(),
                &working,
                PixelFormat::RGB_FLT,
                &srgb,
                PixelFormat::RGB_16,
                intent,
                Flags::NO_CACHE,
            )
            .expect("cannot create the output colour transform"),
        }
    })
}

fn as_pixels(samples: &[f32]) -> &[[f32; 3]] {
    let (pixels, rest) = samples.as_chunks::<3>();
    debug_assert!(rest.is_empty());
    pixels
}

/// Converts working-space RGB triplets to 8-bit sRGB.
pub fn to_srgb8(input: &[f32], output: &mut [u8]) {
    transforms().srgb8.transform_pixels(as_pixels(input), output.as_chunks_mut::<3>().0);
}

/// Converts working-space RGB triplets to 16-bit sRGB.
pub fn to_srgb16(input: &[f32], output: &mut [u16]) {
    transforms().srgb16.transform_pixels(as_pixels(input), output.as_chunks_mut::<3>().0);
}

/// The ICC profile of the sRGB output, for embedding in exported files.
pub fn srgb_icc_profile() -> Vec<u8> {
    Profile::new_srgb().icc().expect("cannot serialise the sRGB profile")
}
