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
