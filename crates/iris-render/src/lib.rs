//! The rendering engine: one pipeline from the decoded source to an encoded sRGB image,
//! shared by the interactive preview and the full-resolution export. No UI and no I/O.

pub mod color_transform;
pub mod detail;
pub mod histogram;
pub mod hsl_mixer;
pub mod mask_coverage;
pub mod pipeline;
pub mod resample;
pub mod tone;
pub mod white_balance;

pub use histogram::Histogram;
pub use mask_coverage::{MaskCoverage, render_mask_coverage};
pub use pipeline::{RenderOptions, crop_gray, render};
pub use resample::{downscale_to_fit, fit_size, resize_area};
pub use white_balance::{estimate_white_balance, sample_white_balance};

#[cfg(test)]
pub(crate) mod tests {
    //! Pipeline tests on synthetic images; no RAW file needed.

    use iris_core::curve::{inverse_s_curve, s_curve};
    use iris_core::mask::LOCAL_ADJUSTMENT_FIELDS;
    use iris_core::{CurvePoint, EditState, HslBand, HslColor, ImageF, LinearGradient, Mask, MaskType, WhiteBalance};

    use super::*;

    pub const AS_SHOT: WhiteBalance = WhiteBalance { temperature: 5500.0, tint: 10.0 };

    pub fn solid(w: usize, h: usize, r: f32, g: f32, b: f32) -> ImageF {
        let mut image = ImageF::new(w, h);
        for px in image.pixels.as_chunks_mut::<3>().0 {
            px.copy_from_slice(&[r, g, b]);
        }
        image
    }

    fn neutral() -> EditState {
        EditState::new(AS_SHOT)
    }

    /// Renders one pixel of the given colour and returns its 8-bit sRGB value.
    fn pixel_with(r: f32, g: f32, b: f32, edits: &EditState) -> [i32; 3] {
        let out = render(&solid(1, 1, r, g, b), &AS_SHOT, edits, &RenderOptions::default());
        let d = out.data8();
        [d[0].into(), d[1].into(), d[2].into()]
    }

    fn pixel(r: f32, g: f32, b: f32) -> [i32; 3] {
        pixel_with(r, g, b, &neutral())
    }

    fn neutral_px(px: [i32; 3]) -> bool {
        (px[0] - px[1]).abs() <= 1 && (px[1] - px[2]).abs() <= 1
    }

    fn saturation_of(px: [i32; 3]) -> f64 {
        let hi = px.into_iter().max().unwrap();
        let lo = px.into_iter().min().unwrap();
        if hi == 0 { 0.0 } else { f64::from(hi - lo) / f64::from(hi) }
    }

    // --- Output transform -------------------------------------------------------

    #[test]
    fn output_transform_maps_neutrals_to_srgb() {
        // Linear 0, 0.18 (mid grey) and 1.0 must land on the sRGB curve and stay neutral.
        for (level, expected) in [(0.0, 0), (0.18, 118), (1.0, 255)] {
            let px = pixel(level, level, level);
            assert!((px[0] - expected).abs() <= 1);
            assert_eq!(px[0], px[1]);
            assert_eq!(px[1], px[2]);
        }
    }

    #[test]
    fn sixteen_bit_output() {
        let out = render(
            &solid(1, 1, 1.0, 1.0, 1.0),
            &AS_SHOT,
            &neutral(),
            &RenderOptions { bits_per_channel: 16, ..Default::default() },
        );
        assert_eq!(out.bits_per_channel(), 16);
        assert!(out.data16()[0] >= 65534);
    }

    // --- White balance ----------------------------------------------------------

    #[test]
    fn higher_temperature_renders_warmer() {
        let mut warm = neutral();
        warm.basic.white_balance.temperature = 8000.0;
        let px = pixel_with(0.18, 0.18, 0.18, &warm);
        assert!(px[0] > px[2] + 10);

        let mut cool = neutral();
        cool.basic.white_balance.temperature = 3500.0;
        let cp = pixel_with(0.18, 0.18, 0.18, &cool);
        assert!(cp[2] > cp[0] + 10);

        let mut magenta = neutral();
        magenta.basic.white_balance.tint = 60.0;
        let mp = pixel_with(0.18, 0.18, 0.18, &magenta);
        assert!(mp[1] < mp[0] && mp[1] < mp[2]);
    }

    #[test]
    fn eyedropper_neutralises_the_sample() {
        let image = solid(40, 30, 0.22, 0.18, 0.12); // warm cast
        let wb = sample_white_balance(&image, 0.5, 0.5, &AS_SHOT).unwrap();
        let mut edits = neutral();
        edits.basic.white_balance = wb;
        let px = pixel_with(0.22, 0.18, 0.12, &edits);
        assert!(neutral_px(px), "{px:?}");
        assert!(wb.temperature < AS_SHOT.temperature); // warm cast -> lower temperature

        // Clipped samples are rejected.
        assert!(sample_white_balance(&solid(10, 10, 1.0, 1.0, 1.0), 0.5, 0.5, &AS_SHOT).is_none());
    }

    #[test]
    fn auto_white_balance_of_neutral_image_is_as_shot() {
        let wb = estimate_white_balance(&solid(20, 20, 0.3, 0.3, 0.3), &AS_SHOT);
        assert!((wb.temperature - AS_SHOT.temperature).abs() < 5.0);
        assert!((wb.tint - AS_SHOT.tint).abs() < 0.5);
    }

    // --- Tone -------------------------------------------------------------------

    #[test]
    fn exposure_is_linear_gain() {
        let mut plus_one = neutral();
        plus_one.basic.exposure = 1.0;
        assert_eq!(pixel_with(0.09, 0.09, 0.09, &plus_one), pixel(0.18, 0.18, 0.18));
    }

    #[test]
    fn contrast_keeps_middle_grey() {
        let mut edits = neutral();
        edits.basic.contrast = 80.0;
        assert!((pixel_with(0.18, 0.18, 0.18, &edits)[0] - 118).abs() <= 1);
        assert!(pixel_with(0.05, 0.05, 0.05, &edits)[0] < pixel(0.05, 0.05, 0.05)[0]);
        assert!(pixel_with(0.6, 0.6, 0.6, &edits)[0] > pixel(0.6, 0.6, 0.6)[0]);
    }

    #[test]
    fn whites_recover_values_above_white() {
        let mut edits = neutral();
        edits.basic.exposure = 1.0; // 0.8 -> 1.6, clipped by default
        assert_eq!(pixel_with(0.8, 0.8, 0.8, &edits)[0], 255);
        edits.basic.whites = -100.0;
        assert!(pixel_with(0.8, 0.8, 0.8, &edits)[0] < 250);
    }

    #[test]
    fn shadows_and_highlights_act_on_their_regions() {
        let mut shadows = neutral();
        shadows.basic.shadows = 100.0;
        let dark_before = pixel(0.01, 0.01, 0.01)[0];
        assert!(pixel_with(0.01, 0.01, 0.01, &shadows)[0] > dark_before + 15);
        assert!((pixel_with(0.7, 0.7, 0.7, &shadows)[0] - pixel(0.7, 0.7, 0.7)[0]).abs() <= 1);

        let mut highlights = neutral();
        highlights.basic.highlights = -100.0;
        assert!(pixel_with(0.7, 0.7, 0.7, &highlights)[0] < pixel(0.7, 0.7, 0.7)[0] - 15);
        assert!((pixel_with(0.01, 0.01, 0.01, &highlights)[0] - dark_before).abs() <= 1);
    }

    #[test]
    fn local_tone_matches_across_resolutions() {
        // Preview and export must agree: a left-dark / right-bright image rendered at two
        // resolutions gives the same values at corresponding points.
        let make = |w: usize, h: usize| {
            let mut image = ImageF::new(w, h);
            for y in 0..h {
                for x in 0..w {
                    let v = if x < w / 2 { 0.02 } else { 0.6 };
                    image.row_mut(y)[x * 3..x * 3 + 3].fill(v);
                }
            }
            image
        };
        let mut edits = neutral();
        edits.basic.shadows = 70.0;
        edits.basic.highlights = -70.0;
        let small = render(&make(400, 300), &AS_SHOT, &edits, &RenderOptions::default());
        let large = render(&make(2000, 1500), &AS_SHOT, &edits, &RenderOptions::default());
        for fx in [0.1, 0.4, 0.6, 0.9] {
            let a = i32::from(small.data8()[(150 * 400 + (fx * 400.0) as usize) * 3]);
            let b = i32::from(large.data8()[(750 * 2000 + (fx * 2000.0) as usize) * 3]);
            assert!((a - b).abs() <= 2, "{a} vs {b} at {fx}");
        }
    }

    // --- Tone curve -------------------------------------------------------------

    #[test]
    fn s_curve_adds_contrast() {
        let mut edits = neutral();
        edits.tone_curve.rgb = s_curve();
        assert!(pixel_with(0.03, 0.03, 0.03, &edits)[0] < pixel(0.03, 0.03, 0.03)[0] - 3);
        assert!(pixel_with(0.5, 0.5, 0.5, &edits)[0] > pixel(0.5, 0.5, 0.5)[0] + 3);
        assert_eq!(pixel_with(1.0, 1.0, 1.0, &edits)[0], 255);
        assert_eq!(pixel_with(0.0, 0.0, 0.0, &edits)[0], 0);

        edits.tone_curve.rgb = inverse_s_curve();
        assert!(pixel_with(0.03, 0.03, 0.03, &edits)[0] > pixel(0.03, 0.03, 0.03)[0] + 3);

        // A lifted black point fades blacks.
        edits.tone_curve.rgb = vec![CurvePoint::new(0.0, 0.1), CurvePoint::new(1.0, 1.0)];
        assert!(pixel_with(0.0, 0.0, 0.0, &edits)[0] > 12); // 0.1 in gamma 2.2 is ~19/255 in sRGB
    }

    // --- HSL --------------------------------------------------------------------

    #[test]
    fn hsl_leaves_neutrals_and_other_colours_alone() {
        let mut edits = neutral();
        edits.hsl.bands = [HslBand::new(60.0, -80.0, 50.0); 8];
        // Greys have no hue, so HSL must not touch them.
        assert_eq!(pixel_with(0.18, 0.18, 0.18, &edits), pixel(0.18, 0.18, 0.18));

        // Adjusting blue does not change red.
        let mut blue = neutral();
        blue.hsl[HslColor::Blue] = HslBand::new(50.0, -100.0, -100.0);
        assert_eq!(pixel_with(0.5, 0.05, 0.04, &blue), pixel(0.5, 0.05, 0.04));
    }

    #[test]
    fn hsl_adjusts_its_colour() {
        let sky = [0.105, 0.15, 0.335]; // a typical sky, sRGB (100, 150, 220) at half brightness

        let mut desaturate = neutral();
        desaturate.hsl[HslColor::Blue].saturation = -100.0;
        assert!(saturation_of(pixel_with(sky[0], sky[1], sky[2], &desaturate)) < 0.12);

        let mut darker = neutral();
        darker.hsl[HslColor::Blue].luminance = -100.0;
        let before = pixel(sky[0], sky[1], sky[2]);
        let after = pixel_with(sky[0], sky[1], sky[2], &darker);
        assert!(after[2] < before[2] - 20);

        // Red hue +100 moves red towards orange (more green), -100 towards magenta (more blue).
        // (A red inside the sRGB gamut, sRGB (200, 40, 40), so the result is not clipped.)
        let mut warmer = neutral();
        warmer.hsl[HslColor::Red].hue = 100.0;
        let red = pixel(0.37, 0.06, 0.03);
        assert!(pixel_with(0.37, 0.06, 0.03, &warmer)[1] > red[1] + 15);
        warmer.hsl[HslColor::Red].hue = -100.0;
        assert!(pixel_with(0.37, 0.06, 0.03, &warmer)[2] > red[2] + 15);
    }

    // --- Presence ---------------------------------------------------------------

    #[test]
    fn saturation_minus_100_is_monochrome() {
        let mut edits = neutral();
        edits.basic.saturation = -100.0;
        assert!(neutral_px(pixel_with(0.4, 0.1, 0.05, &edits)));
    }

    #[test]
    fn vibrance_favours_muted_colours() {
        let mut edits = neutral();
        edits.basic.vibrance = 100.0;
        // Muted blue-green vs. strongly saturated blue-green (away from skin hues).
        let muted_gain = saturation_of(pixel_with(0.15, 0.2, 0.22, &edits)) - saturation_of(pixel(0.15, 0.2, 0.22));
        let vivid_gain = saturation_of(pixel_with(0.02, 0.2, 0.3, &edits)) - saturation_of(pixel(0.02, 0.2, 0.3));
        assert!(muted_gain > 0.05);
        assert!(muted_gain > vivid_gain);
    }

    // --- Masks ------------------------------------------------------------------

    #[test]
    fn local_adjustments_only_inside_the_mask() {
        // A grey image; a linear mask covering the left half brightens it by 1 EV.
        let mut edits = neutral();
        let mut mask = Mask::new(MaskType::Linear, &[]);
        mask.linear = LinearGradient { x: 0.5, y: 0.5, angle: 90.0, feather: 0.02 };
        edits.masks = vec![mask];
        let grey = solid(100, 20, 0.1, 0.1, 0.1);
        let options = RenderOptions::default();
        let plain = render(&grey, &AS_SHOT, &neutral(), &options);
        let neutral_mask = render(&grey, &AS_SHOT, &edits, &options);
        assert_eq!(neutral_mask, plain); // a mask with no adjustments changes nothing

        edits.masks[0].adjustments.exposure = 1.0;
        let out = render(&grey, &AS_SHOT, &edits, &options);
        let px = |image: &iris_core::EncodedImage, x: usize| i32::from(image.data8()[(10 * 100 + x) * 3]);
        assert_eq!(px(&out, 90), px(&plain, 90));
        assert!((px(&out, 10) - pixel(0.2, 0.2, 0.2)[0]).abs() <= 1); // +1 EV = twice the light
        assert!(px(&out, 48) > px(&out, 52));
    }

    #[test]
    fn local_adjustments_move_in_the_right_direction() {
        let with_local = |key: &str, value: f32| {
            let mut edits = neutral();
            let mut mask = Mask::new(MaskType::Brush, &[]);
            mask.invert = true; // everywhere
            let field = LOCAL_ADJUSTMENT_FIELDS.iter().find(|f| f.key == key).unwrap();
            *(field.value)(&mut mask.adjustments) = value;
            edits.masks = vec![mask];
            edits
        };
        let grey = pixel(0.18, 0.18, 0.18);

        let warm = pixel_with(0.18, 0.18, 0.18, &with_local("temperature", 60.0));
        assert!(warm[0] > grey[0] + 3 && warm[2] < grey[2] - 3);

        let contrast = with_local("contrast", 100.0);
        assert!(pixel_with(0.6, 0.6, 0.6, &contrast)[0] > pixel(0.6, 0.6, 0.6)[0] + 5);
        assert!(pixel_with(0.03, 0.03, 0.03, &contrast)[0] < pixel(0.03, 0.03, 0.03)[0] - 5);
        assert!((pixel_with(0.18, 0.18, 0.18, &contrast)[0] - grey[0]).abs() <= 1);

        assert!(neutral_px(pixel_with(0.4, 0.1, 0.05, &with_local("saturation", -100.0))));

        assert!(pixel_with(0.01, 0.01, 0.01, &with_local("shadows", 100.0))[0] > pixel(0.01, 0.01, 0.01)[0] + 15);
        assert!(pixel_with(0.7, 0.7, 0.7, &with_local("highlights", -100.0))[0] < pixel(0.7, 0.7, 0.7)[0] - 15);
    }

    // --- Crop and rotation ------------------------------------------------------

    /// A gradient so every pixel differs: red increases left to right, green top to bottom.
    fn gradient(w: usize, h: usize) -> ImageF {
        let mut image = ImageF::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = &mut image.row_mut(y)[x * 3..x * 3 + 3];
                p.copy_from_slice(&[0.05 + 0.6 * x as f32 / w as f32, 0.05 + 0.6 * y as f32 / h as f32, 0.2]);
            }
        }
        image
    }

    fn px(image: &iris_core::EncodedImage, x: usize, y: usize) -> [u8; 3] {
        let d = image.data8();
        let i = (y * image.width + x) * 3;
        [d[i], d[i + 1], d[i + 2]]
    }

    fn cropped(crop: iris_core::Crop) -> EditState {
        EditState { crop, ..neutral() }
    }

    #[test]
    fn crop_rectangle_is_an_exact_cut_out() {
        let source = gradient(40, 20);
        let full = render(&source, &AS_SHOT, &neutral(), &RenderOptions::default());
        let crop = iris_core::Crop { left: 0.25, top: 0.5, right: 0.75, bottom: 1.0, ..Default::default() };
        let out = render(&source, &AS_SHOT, &cropped(crop), &RenderOptions::default());
        assert_eq!((out.width, out.height), (20, 10));
        for y in 0..10 {
            for x in 0..20 {
                assert_eq!(px(&out, x, y), px(&full, x + 10, y + 10));
            }
        }
        // While cropping, the whole frame is shown.
        let whole =
            render(&source, &AS_SHOT, &cropped(crop), &RenderOptions { whole_frame: true, ..Default::default() });
        assert_eq!(whole, full);
    }

    #[test]
    fn quarter_turns_rotate_the_result() {
        let source = gradient(40, 20);
        let full = render(&source, &AS_SHOT, &neutral(), &RenderOptions::default());
        let turned = |turns| {
            render(
                &source,
                &AS_SHOT,
                &cropped(iris_core::Crop { quarter_turns: turns, ..Default::default() }),
                &Default::default(),
            )
        };
        let cw = turned(1);
        assert_eq!((cw.width, cw.height), (20, 40));
        assert_eq!(px(&cw, 0, 0), px(&full, 0, 19)); // bottom-left comes to the top-left
        assert_eq!(px(&cw, 19, 39), px(&full, 39, 0));
        let half = turned(2);
        assert_eq!(px(&half, 0, 0), px(&full, 39, 19));
        let ccw = turned(3);
        assert_eq!(px(&ccw, 0, 0), px(&full, 39, 0));
    }

    #[test]
    fn straightening_fills_the_frame_once_fitted() {
        let source = solid(300, 200, 0.4, 0.4, 0.4);
        let straight = iris_core::Crop { angle: 8.0, ..Default::default() };
        let is_empty = |p: [u8; 3]| p[0] < 30;
        // Unfitted, the corners are empty (as shown while cropping) ...
        let whole =
            render(&source, &AS_SHOT, &cropped(straight), &RenderOptions { whole_frame: true, ..Default::default() });
        assert!(is_empty(px(&whole, 0, 0)));
        assert!(!is_empty(px(&whole, 150, 100)));
        // ... and the fitted crop has none.
        let fitted = straight.constrained(300, 200);
        let out = render(&source, &AS_SHOT, &cropped(fitted), &Default::default());
        assert!(out.width < 300 && out.height < 200);
        for (x, y) in [(0, 0), (out.width - 1, 0), (0, out.height - 1), (out.width - 1, out.height - 1)] {
            assert!(!is_empty(px(&out, x, y)), "empty corner at {x},{y}");
        }
    }

    #[test]
    fn long_edge_applies_to_the_cropped_result() {
        let source = gradient(400, 200);
        let crop = iris_core::Crop { right: 0.5, ..Default::default() }; // 200 x 200
        for angle in [0.0, 3.0] {
            let crop = iris_core::Crop { angle, ..crop }.constrained(400, 200);
            let out =
                render(&source, &AS_SHOT, &cropped(crop), &RenderOptions { max_long_edge: 100, ..Default::default() });
            assert_eq!(out.width.max(out.height), 100, "angle {angle}");
        }
    }

    #[test]
    fn masks_stay_on_the_photo_under_a_crop() {
        // A mask over the left half brightens it; cropping to the right half shows no change,
        // cropping to the left half shows it everywhere.
        let grey = solid(100, 20, 0.1, 0.1, 0.1);
        let mut edits = neutral();
        let mut mask = Mask::new(MaskType::Linear, &[]);
        mask.linear = LinearGradient { x: 0.5, y: 0.5, angle: 90.0, feather: 0.02 };
        mask.adjustments.exposure = 1.0;
        edits.masks = vec![mask];
        let plain = pixel(0.1, 0.1, 0.1);
        let bright = pixel(0.2, 0.2, 0.2);
        edits.crop = iris_core::Crop { left: 0.6, ..Default::default() };
        let right = render(&grey, &AS_SHOT, &edits, &Default::default());
        assert!((i32::from(px(&right, 5, 10)[0]) - plain[0]).abs() <= 1);
        // After a clockwise turn the photo's left side is the top of the frame.
        edits.crop = iris_core::Crop { bottom: 0.4, quarter_turns: 1, ..Default::default() };
        let left = render(&grey, &AS_SHOT, &edits, &Default::default());
        assert_eq!((left.width, left.height), (20, 40));
        assert!((i32::from(px(&left, 10, 5)[0]) - bright[0]).abs() <= 1);
    }

    // --- Detail -------------------------------------------------------------------

    fn sharpened(amount: f32) -> EditState {
        let mut edits = neutral();
        edits.detail.sharpening = iris_core::Sharpening { amount, radius: 1.0, masking: 0.0 };
        edits
    }

    #[test]
    fn sharpening_shows_on_the_full_image_and_is_scaled_for_previews() {
        let source = gradient(80, 40);
        let plain = render(&source, &AS_SHOT, &neutral(), &RenderOptions::default());
        let sharp = render(&source, &AS_SHOT, &sharpened(150.0), &RenderOptions::default());
        // A smooth gradient has nothing to sharpen (away from the image border, where the
        // blur's clamped edge looks like an edge).
        for y in 2..38 {
            for x in 2..78 {
                let (a, b) = (px(&plain, x, y), px(&sharp, x, y));
                assert!((0..3).all(|c| a[c].abs_diff(b[c]) <= 1), "{a:?} vs {b:?} at {x},{y}");
            }
        }

        // An edge gets crisper.
        let mut edge = solid(80, 40, 0.05, 0.05, 0.05);
        for y in 0..40 {
            edge.row_mut(y)[40 * 3..].fill(0.5);
        }
        let plain = render(&edge, &AS_SHOT, &neutral(), &RenderOptions::default());
        let sharp = render(&edge, &AS_SHOT, &sharpened(150.0), &RenderOptions::default());
        assert!(px(&sharp, 39, 20)[0] < px(&plain, 39, 20)[0]);
        assert!(px(&sharp, 40, 20)[0] > px(&plain, 40, 20)[0]);
        // A quarter-size preview of a 1 px radius shows nothing.
        let preview = RenderOptions { source_scale: 0.2, ..Default::default() };
        assert_eq!(render(&edge, &AS_SHOT, &sharpened(150.0), &preview), plain);
    }

    #[test]
    fn detail_under_a_crop_matches_the_uncropped_render() {
        let mut edge = solid(80, 40, 0.05, 0.05, 0.05);
        for y in 0..40 {
            edge.row_mut(y)[40 * 3..].fill(0.5);
        }
        let mut edits = sharpened(150.0);
        edits.detail.noise_reduction.luminance = 30.0;
        let full = render(&edge, &AS_SHOT, &edits, &RenderOptions::default());
        // Crop right up to the edge: the margin lets sharpening see the other side.
        edits.crop = iris_core::Crop { left: 0.4375, right: 0.6, ..Default::default() };
        let cropped = render(&edge, &AS_SHOT, &edits, &RenderOptions::default());
        for x in 0..cropped.width {
            assert_eq!(px(&cropped, x, 20), px(&full, x + 35, 20), "x {x}");
        }
    }
}
