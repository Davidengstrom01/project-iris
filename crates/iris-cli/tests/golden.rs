//! Compares renders with reference images made by the C++ version of Project Iris.
//!
//! The references in tests/golden were rendered from one RAW file with the sidecars next
//! to them (see tests/golden/README.md). They are not in git; the test is skipped unless
//! IRIS_TEST_RAW points at that RAW file and the reference images exist.

use std::path::{Path, PathBuf};
use std::process::Command;

use iris_core::EditState;
use iris_export::{ExportFormat, ExportSettings, write_image};
use iris_persist::read_sidecar_file;
use iris_raw::{DecodeQuality, decode};
use iris_render::{RenderOptions, render};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

fn test_raw() -> Option<PathBuf> {
    let raw = PathBuf::from(std::env::var_os("IRIS_TEST_RAW")?);
    golden_dir().join("neutral.png").exists().then_some(raw)
}

/// Fails if any sample differs by more than `max` or the mean difference exceeds `mean`
/// (both in 8-bit units).
fn assert_close(name: &str, actual: &[f64], expected: &[f64], max: f64, mean: f64) {
    assert_eq!(actual.len(), expected.len(), "{name}: size differs");
    let mut worst = 0.0f64;
    let mut total = 0.0;
    for (a, e) in actual.iter().zip(expected) {
        let d = (a - e).abs();
        worst = worst.max(d);
        total += d;
    }
    let average = total / actual.len() as f64;
    eprintln!("{name}: max difference {worst:.3}, mean {average:.4}");
    assert!(worst <= max && average <= mean, "{name}: max {worst:.3} (limit {max}), mean {average:.4} (limit {mean})");
}

#[test]
fn matches_cpp_renders() {
    let Some(raw) = test_raw() else {
        eprintln!("Set IRIS_TEST_RAW to the golden RAW file (see tests/golden/README.md) to run this test");
        return;
    };
    let golden = golden_dir();

    // Metadata as printed by `iris-cli --info`.
    let info = Command::new(env!("CARGO_BIN_EXE_iris-cli")).arg("--info").arg(&raw).output().unwrap();
    let expected_info = std::fs::read_to_string(golden.join("info.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&info.stdout), expected_info, "IRIS_TEST_RAW is not the golden RAW file?");

    let decoded = decode(&raw, DecodeQuality::Full, &|| false).unwrap();
    let as_shot = decoded.metadata.as_shot;
    let options = RenderOptions { max_long_edge: 1200, ..Default::default() };

    for name in ["neutral", "basic", "curve-hsl", "masks"] {
        let defaults = EditState::new(as_shot);
        let edits =
            read_sidecar_file(&golden.join(format!("{name}.iris.json")), &defaults).unwrap().unwrap_or(defaults);
        let actual = render(&decoded.image, &as_shot, &edits, &options);
        let expected = image::open(golden.join(format!("{name}.png"))).unwrap().to_rgb8();
        assert_eq!((actual.width as u32, actual.height as u32), expected.dimensions(), "{name}");
        let a: Vec<f64> = actual.data8().iter().map(|&v| f64::from(v)).collect();
        let e: Vec<f64> = expected.as_raw().iter().map(|&v| f64::from(v)).collect();
        assert_close(name, &a, &e, 2.0, 0.5);

        // The PNG encoder round-trips the render exactly.
        let mut png = Vec::new();
        write_image(&actual, &ExportSettings { format: ExportFormat::Png, ..Default::default() }, &mut png).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().to_rgb8().as_raw(), actual.data8());
    }

    // 16-bit TIFF.
    let edits = read_sidecar_file(&golden.join("basic.iris.json"), &EditState::new(as_shot)).unwrap().unwrap();
    let actual = render(
        &decoded.image,
        &as_shot,
        &edits,
        &RenderOptions { max_long_edge: 600, bits_per_channel: 16, ..Default::default() },
    );
    let expected = image::open(golden.join("basic16.tif")).unwrap().to_rgb16();
    assert_eq!((actual.width as u32, actual.height as u32), expected.dimensions());
    let a: Vec<f64> = actual.data16().iter().map(|&v| f64::from(v) / 257.0).collect();
    let e: Vec<f64> = expected.as_raw().iter().map(|&v| f64::from(v) / 257.0).collect();
    assert_close("basic16", &a, &e, 2.0, 0.5);
}
