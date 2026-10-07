//! End-to-end test of the desktop UI on a real RAW file, following the MVP workflow:
//! open, zoom, edit, save, presets, undo, masks, before/after, export, reopen.
//!
//!   IRIS_TEST_RAW=/path/to/photo.ARW cargo test -p iris-app ui_
//!
//! The RAW file is copied to a temporary folder (sidecars are written next to it), and user
//! presets go to a temporary folder too. Runs headless (no window, no GPU).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::action::Action;
use crate::app::IrisApp;
use crate::dialogs::Dialog;
use crate::view::CompareMode;

struct Fixture {
    _dir: tempfile::TempDir,
    photo_a: PathBuf,
    photo_b: PathBuf,
    presets: PathBuf,
    raw_bytes: Vec<u8>,
}

fn fixture() -> Option<Fixture> {
    let raw = PathBuf::from(std::env::var_os("IRIS_TEST_RAW")?);
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let ext = raw.extension().unwrap().to_string_lossy().into_owned();
    let photo_a = photos.join(format!("a.{ext}"));
    let photo_b = photos.join(format!("b.{ext}"));
    std::fs::copy(&raw, &photo_a).unwrap();
    std::fs::copy(&raw, &photo_b).unwrap();
    let raw_bytes = std::fs::read(&raw).unwrap();
    Some(Fixture { presets: dir.path().join("presets"), _dir: dir, photo_a, photo_b, raw_bytes })
}

fn harness(fixture: &Fixture, open: &Path) -> Harness<'static, IrisApp> {
    let (open, presets) = (open.to_owned(), fixture.presets.clone());
    Harness::builder()
        .with_size([1500.0, 950.0])
        .build_eframe(move |cc| IrisApp::with_preset_directory(cc, Some(open), presets))
}

/// Runs frames until `done` holds (worker threads finish in the background).
fn wait_until(harness: &mut Harness<'_, IrisApp>, what: &str, done: impl Fn(&IrisApp) -> bool) {
    let start = Instant::now();
    while !done(harness.state()) {
        assert!(start.elapsed() < Duration::from_secs(90), "timed out waiting for {what}");
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    harness.step();
}

/// Clicks the last widget with this label: a dialog's button rather than the toolbar's.
fn click(harness: &mut Harness<'_, IrisApp>, label: &str) {
    harness.get_all_by_label(label).last().unwrap_or_else(|| panic!("no {label:?}")).scroll_to_me();
    harness.run_steps(30); // let the animated scrolling settle
    harness.get_all_by_label(label).last().unwrap().click();
    harness.step();
}

fn press(harness: &mut Harness<'_, IrisApp>, modifiers: Modifiers, key: Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.step();
}

fn drag(harness: &mut Harness<'_, IrisApp>, from: Pos2, to: Pos2) {
    let pointer = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    harness.event(egui::Event::PointerMoved(from));
    harness.step();
    harness.event(pointer(from, true));
    harness.step();
    for i in 1..=10 {
        harness.event(egui::Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
        harness.step();
    }
    harness.event(pointer(to, false));
    harness.step();
}

#[test]
fn ui_mvp_workflow() {
    let Some(f) = fixture() else {
        eprintln!("Set IRIS_TEST_RAW to a RAW file to run this test");
        return;
    };
    let mut h = harness(&f, &f.photo_a);

    // Open and view: the photo fits the window; full resolution only when zoomed in.
    wait_until(&mut h, "the preview", |app| app.test_loaded());
    assert!(h.state().view().is_fit());
    assert!(!h.state().view().has_full_image());
    press(&mut h, Modifiers::NONE, Key::Num2);
    assert!(!h.state().view().is_fit());
    wait_until(&mut h, "the full-resolution image", |app| app.view().has_full_image());
    press(&mut h, Modifiers::NONE, Key::Num1);
    assert!(h.state().view().is_fit());

    // Edit, save, and the title shows unsaved changes.
    let mut basic = h.state().test_edits().basic;
    basic.exposure = 0.7;
    basic.contrast = 20.0;
    h.state_mut().test_apply(Action::EditBasic(basic));
    h.step();
    assert!(h.state().test_title().contains('•'));
    press(&mut h, Modifiers::COMMAND, Key::S);
    let sidecar = f.photo_a.with_extension("iris.json");
    assert!(sidecar.exists());
    assert!(!h.state().test_title().contains('•'));

    // Presets: clicking one applies it; undo takes it back.
    click(&mut h, "Warm Film");
    assert_eq!(h.state().test_edits().applied_preset, "Warm Film");
    assert_eq!(h.state().test_edits().basic.exposure, 0.7); // not part of the preset
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(h.state().test_edits().applied_preset, "");
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(h.state().test_edits().applied_preset, "Warm Film");

    // Masks: M adds a brush mask; painting on the photo is one undo step.
    press(&mut h, Modifiers::NONE, Key::M);
    assert_eq!(h.state().test_edits().masks.len(), 1);
    let rect = h.state().view().rect();
    drag(&mut h, rect.center() - egui::vec2(100.0, 0.0), rect.center() + egui::vec2(100.0, 20.0));
    let strokes = &h.state().test_edits().masks[0].strokes;
    assert_eq!(strokes.len(), 1);
    assert!(strokes[0].points.len() > 2);
    let mut mask = h.state().test_edits().masks[0].clone();
    mask.adjustments.exposure = 1.0;
    h.state_mut().test_apply(Action::EditMask(mask, "Brush 1 Exposure".into()));
    wait_until(&mut h, "the mask overlay", |app| app.test_has_overlay());
    press(&mut h, Modifiers::COMMAND, Key::Z); // exposure
    press(&mut h, Modifiers::COMMAND, Key::Z); // the stroke, in one step
    assert!(h.state().test_edits().masks[0].strokes.is_empty());
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(h.state().test_edits().masks[0].adjustments.exposure, 1.0);
    press(&mut h, Modifiers::NONE, Key::Escape);
    assert!(h.state().test_selected_mask().is_none());

    // Before / after.
    press(&mut h, Modifiers::NONE, Key::Y);
    assert_eq!(h.state().view().compare_mode(), CompareMode::Split);
    press(&mut h, Modifiers::NONE, Key::Y);
    assert_eq!(h.state().view().compare_mode(), CompareMode::Off);
    h.key_down(Key::Backslash);
    h.step();
    assert_eq!(h.state().view().compare_mode(), CompareMode::Before);
    h.key_up(Key::Backslash);
    h.step();
    assert_eq!(h.state().view().compare_mode(), CompareMode::Before); // a tap toggles
    h.key_press(Key::Backslash);
    h.step();
    assert_eq!(h.state().view().compare_mode(), CompareMode::Off);

    // Export through the dialog.
    press(&mut h, Modifiers::COMMAND, Key::E);
    assert!(matches!(h.state().test_dialog(), Some(Dialog::Export(_))));
    click(&mut h, "Export");
    assert!(h.state().test_dialog().is_none());
    wait_until(&mut h, "the export", |app| app.test_exports_running() == 0);
    let exported = f.photo_a.with_extension("jpg");
    let image = image_size(&exported);
    assert_eq!(image, h.state().test_full_size());

    // Opening another photo with unsaved edits asks first; saving continues.
    click(&mut h, &f.photo_b.file_name().unwrap().to_string_lossy());
    assert!(matches!(h.state().test_dialog(), Some(Dialog::Unsaved { .. })));
    click(&mut h, "Save");
    wait_until(&mut h, "photo b", |app| app.test_loaded() && app.test_path().ends_with(f.photo_b.file_name().unwrap()));
    assert_eq!(h.state().test_edits().basic.exposure, 0.0); // b has no edits

    // Reopening restores everything that was saved.
    let mut h = harness(&f, &f.photo_a);
    wait_until(&mut h, "the preview", |app| app.test_loaded());
    let edits = h.state().test_edits();
    assert_eq!(edits.basic.exposure, 0.7);
    assert_eq!(edits.applied_preset, "Warm Film");
    assert_eq!(edits.masks.len(), 1);
    assert_eq!(edits.masks[0].strokes.len(), 1);

    // The RAW files were never modified.
    assert_eq!(std::fs::read(&f.photo_a).unwrap(), f.raw_bytes);
    assert_eq!(std::fs::read(&f.photo_b).unwrap(), f.raw_bytes);
}

#[test]
fn ui_custom_presets() {
    let Some(f) = fixture() else { return };
    let mut h = harness(&f, &f.photo_a);
    wait_until(&mut h, "the preview", |app| app.test_loaded());

    let mut basic = h.state().test_edits().basic;
    basic.contrast = 35.0;
    basic.exposure = 1.0;
    h.state_mut().test_apply(Action::EditBasic(basic));
    click(&mut h, "Save Preset…");
    let Some(Dialog::SavePreset(dialog)) = h.state_mut().test_dialog_mut() else { panic!("no dialog") };
    dialog.name = "Punchy".into();
    h.step();
    click(&mut h, "Exposure"); // leave exposure out (the dialog's checkbox)
    click(&mut h, "Save");
    assert!(f.presets.join("My Presets/punchy.json").exists());

    // Apply it to another photo.
    h.state_mut().test_apply(Action::OpenPhoto(f.photo_b.clone()));
    h.step();
    if matches!(h.state().test_dialog(), Some(Dialog::Unsaved { .. })) {
        click(&mut h, "Discard");
    }
    wait_until(&mut h, "photo b", |app| app.test_loaded() && app.test_path().ends_with(f.photo_b.file_name().unwrap()));
    click(&mut h, "Punchy");
    assert_eq!(h.state().test_edits().basic.contrast, 35.0);
    assert_eq!(h.state().test_edits().basic.exposure, 0.0);
}

#[test]
fn ui_crop_and_rotate() {
    let Some(f) = fixture() else { return };
    let mut h = harness(&f, &f.photo_a);
    wait_until(&mut h, "the preview", |app| app.test_loaded());
    let [w, h0] = h.state().test_full_size();
    let size = |app: &IrisApp| app.test_size_label().to_owned();
    assert_eq!(size(h.state()), format!("{w} × {h0}"));

    // R starts cropping; drag the right edge halfway in.
    press(&mut h, Modifiers::NONE, Key::R);
    assert!(h.state().test_cropping());
    let image = h.state().view().test_image_rect();
    drag(&mut h, image.right_center(), image.center());
    let crop = h.state().test_edits().crop;
    assert!((crop.right - 0.5).abs() < 0.02, "{crop:?}");
    assert_eq!(crop.left, 0.0);
    // While cropping the whole frame stays on screen.
    assert_eq!(size(h.state()), format!("{w} × {h0}"));

    // Enter finishes; the rendering is the cropped part.
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert!(!h.state().test_cropping());
    let cropped_width = (crop.right * w as f32).round() as usize;
    let expected = format!("{cropped_width} × {h0}");
    wait_until(&mut h, "the cropped preview", |app| app.test_size_label() == expected);

    // The drag was one undo step.
    press(&mut h, Modifiers::COMMAND, Key::Z);
    assert_eq!(h.state().test_edits().crop, iris_core::Crop::default());
    press(&mut h, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    assert_eq!(h.state().test_edits().crop, crop);

    // Ctrl+] turns the photo clockwise, crop included.
    press(&mut h, Modifiers::COMMAND, Key::CloseBracket);
    let expected = format!("{h0} × {cropped_width}");
    wait_until(&mut h, "the rotated preview", |app| app.test_size_label() == expected);
    assert_eq!(h.state().test_edits().crop.quarter_turns, 1);

    // Straightening keeps the crop inside the photo.
    h.state_mut().test_apply(Action::Straighten(6.0));
    let crop = h.state().test_edits().crop;
    assert_eq!(crop.angle, 6.0);
    assert!(crop.fits_photo(w, h0));

    // A locked aspect ratio.
    h.state_mut().test_apply(Action::SetCropAspect(crate::crop_tool::Aspect::Ratio(1, 1)));
    let crop = h.state().test_edits().crop;
    assert!((crop.pixel_aspect(w, h0) - 1.0).abs() < 0.01);

    let [cw, ch] = crate::session::Framing::new(&crop, [w, h0], false).result_size;
    let expected = format!("{cw} × {ch}");
    wait_until(&mut h, "the square preview", |app| app.test_size_label() == expected);

    // Export: the file has the cropped size.
    press(&mut h, Modifiers::COMMAND, Key::E);
    click(&mut h, "Export");
    wait_until(&mut h, "the export", |app| app.test_exports_running() == 0);
    assert_eq!(image_size(&f.photo_a.with_extension("jpg")), [cw, ch]);

    // Saved with the photo, and restored on reopening.
    press(&mut h, Modifiers::COMMAND, Key::S);
    let saved = h.state().test_edits().crop;
    let mut h = harness(&f, &f.photo_a);
    wait_until(&mut h, "the preview", |app| app.test_loaded());
    assert_eq!(h.state().test_edits().crop, saved);
}

#[test]
fn ui_detail_panel() {
    let Some(f) = fixture() else { return };
    let mut h = harness(&f, &f.photo_a);
    wait_until(&mut h, "the preview", |app| app.test_loaded());

    // Moving the Amount slider sharpens; it is one undo step labelled "Sharpening".
    click(&mut h, "Amount");
    let slider_y = h.get_all_by_label("Amount").last().unwrap().rect().bottom() + 9.0;
    let left = h.get_all_by_label("Amount").last().unwrap().rect().left() + 8.0;
    drag(&mut h, egui::pos2(left, slider_y), egui::pos2(left + 120.0, slider_y));
    let amount = h.state().test_edits().detail.sharpening.amount;
    assert!(amount > 20.0, "{amount}");
    assert_eq!(h.state().test_undo_label(), "Sharpening");

    let detail = iris_core::Detail {
        noise_reduction: iris_core::NoiseReduction { luminance: 30.0, color: 25.0 },
        ..h.state().test_edits().detail
    };
    h.state_mut().test_apply(Action::EditDetail(detail));
    assert_eq!(h.state().test_undo_label(), "Noise Reduction");

    // Saved with the photo and restored; Reset removes it.
    press(&mut h, Modifiers::COMMAND, Key::S);
    let saved = h.state().test_edits().detail;
    let mut h = harness(&f, &f.photo_a);
    wait_until(&mut h, "the preview", |app| app.test_loaded());
    assert_eq!(h.state().test_edits().detail, saved);
    h.state_mut().test_apply(Action::ResetDetail);
    assert!(h.state().test_edits().detail.is_neutral());
}

/// Screenshots of the main states, for looking at the UI without a display:
///   IRIS_TEST_RAW=photo.ARW IRIS_TEST_SCREENSHOTS=/some/dir cargo test -p iris-app ui_screenshots
#[test]
fn ui_screenshots() {
    let (Some(f), Some(dir)) = (fixture(), std::env::var_os("IRIS_TEST_SCREENSHOTS").map(PathBuf::from)) else {
        return;
    };
    let (open, presets) = (f.photo_a.clone(), f.presets.clone());
    let mut h = Harness::builder()
        .with_size([1500.0, 950.0])
        .wgpu()
        .build_eframe(move |cc| IrisApp::with_preset_directory(cc, Some(open), presets));
    let shot = |h: &mut Harness<'_, IrisApp>, name: &str| {
        h.run_steps(5);
        h.render().expect("render").save(dir.join(name)).unwrap();
    };
    wait_until(&mut h, "the preview", |app| app.test_loaded());
    shot(&mut h, "01-photo.png");
    h.set_size(egui::vec2(780.0, 900.0));
    shot(&mut h, "01-narrow.png");
    h.set_size(egui::vec2(1500.0, 950.0));

    press(&mut h, Modifiers::NONE, Key::R);
    h.state_mut().test_apply(Action::Straighten(4.0));
    h.state_mut().test_apply(Action::SetCropAspect(crate::crop_tool::Aspect::Ratio(4, 3)));
    let image = h.state().view().test_image_rect();
    drag(&mut h, image.right_bottom() - egui::vec2(80.0, 60.0), image.center() + egui::vec2(150.0, 80.0));
    wait_until(&mut h, "the straightened frame", |app| {
        app.view().test_framing().is_some_and(|f| f.photo_to_result.m12 != 0.0)
    });
    shot(&mut h, "02-cropping.png");

    press(&mut h, Modifiers::NONE, Key::Enter);
    let crop = h.state().test_edits().crop;
    let [cw, ch] = crate::session::Framing::new(&crop, h.state().test_full_size(), false).result_size;
    let expected = format!("{cw} × {ch}");
    wait_until(&mut h, "the cropped preview", |app| app.test_size_label() == expected);
    shot(&mut h, "03-cropped.png");

    press(&mut h, Modifiers::NONE, Key::M);
    let rect = h.state().view().rect();
    drag(&mut h, rect.center() - egui::vec2(150.0, 0.0), rect.center() + egui::vec2(150.0, 40.0));
    assert_eq!(h.state().test_edits().masks[0].strokes.len(), 1);
    // Let the overlay of the stroke (not just the empty mask) arrive.
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1500) {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
    shot(&mut h, "04-mask-on-crop.png");

    press(&mut h, Modifiers::NONE, Key::Escape);
    let mut detail = h.state().test_edits().detail;
    detail.sharpening.amount = 120.0;
    detail.noise_reduction.color = 25.0;
    h.state_mut().test_apply(Action::EditDetail(detail));
    press(&mut h, Modifiers::NONE, Key::Num2);
    wait_until(&mut h, "the full-resolution image", |app| app.view().has_full_image());
    h.get_all_by_label("Amount").last().unwrap().scroll_to_me();
    shot(&mut h, "05-detail.png");
}

fn image_size(path: &Path) -> [usize; 2] {
    let data = std::fs::read(path).unwrap();
    // JPEG SOF0/SOF2 marker: height and width follow the marker length and precision.
    let mut i = 2;
    while i + 9 < data.len() {
        let marker = data[i + 1];
        let length = usize::from(data[i + 2]) << 8 | usize::from(data[i + 3]);
        if marker == 0xC0 || marker == 0xC2 {
            let h = usize::from(data[i + 5]) << 8 | usize::from(data[i + 6]);
            let w = usize::from(data[i + 7]) << 8 | usize::from(data[i + 8]);
            return [w, h];
        }
        i += 2 + length;
    }
    panic!("not a JPEG");
}
