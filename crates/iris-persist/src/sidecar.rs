//! Edits are stored next to the RAW file as JSON: photo.ARW -> photo.iris.json.
//! If that name is already taken by another photo's sidecar (photo.ARW and photo.CR2 in
//! one folder), photo.ARW.iris.json is used instead.

use std::path::{Path, PathBuf};

use iris_core::EditState;
use serde_json::{Map, Value, json};

use crate::json::{
    adjustments_to_json, crop_to_json, detail_to_json, hsl_to_json, masks_to_json, read_adjustments, read_crop,
    read_detail, read_hsl, read_masks, read_retouch, read_tone_curve, retouch_to_json, tone_curve_to_json,
};
use crate::{Error, read_json_object, write_atomically};

pub const SIDECAR_VERSION: i64 = 1;

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The sidecar path for a RAW file.
pub fn sidecar_path_for(raw_path: &Path) -> PathBuf {
    let dir = raw_path.parent().unwrap_or(Path::new(""));
    let name = file_name(raw_path);
    // Like QFileInfo::completeBaseName: everything before the last dot.
    let base = match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name.as_str(),
    };
    let primary = dir.join(format!("{base}.iris.json"));
    if primary.exists() {
        let owner = read_json_object(&primary)
            .ok()
            .and_then(|o| o.get("originalFilename").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_default();
        if !owner.is_empty() && owner != name {
            return dir.join(format!("{name}.iris.json"));
        }
    }
    primary
}

/// Reads edits from a sidecar file. `Ok(None)` if there is no sidecar; missing values
/// fall back to `defaults`.
pub fn read_sidecar_file(sidecar_path: &Path, defaults: &EditState) -> Result<Option<EditState>, Error> {
    if !sidecar_path.exists() {
        return Ok(None);
    }
    let json = read_json_object(sidecar_path)?;
    if json.get("version").and_then(Value::as_i64).unwrap_or(0) > SIDECAR_VERSION {
        return Err(Error::NewerVersion);
    }
    let mut edits = defaults.clone();
    read_adjustments(json.get("adjustments"), &mut edits.basic);
    if let Some(curve) = read_tone_curve(json.get("toneCurve")) {
        edits.tone_curve = curve;
    }
    edits.hsl = read_hsl(json.get("hsl"));
    edits.masks = read_masks(json.get("masks"));
    edits.crop = read_crop(json.get("crop"));
    edits.detail = read_detail(json.get("detail"));
    edits.retouch = read_retouch(json.get("retouch"));
    edits.applied_preset = json.get("preset").and_then(Value::as_str).unwrap_or_default().to_owned();
    Ok(Some(edits))
}

/// Reads the sidecar of `raw_path`.
pub fn read_sidecar(raw_path: &Path, defaults: &EditState) -> Result<Option<EditState>, Error> {
    read_sidecar_file(&sidecar_path_for(raw_path), defaults)
}

/// The sidecar document for `edits` of the photo at `raw_path`.
pub fn sidecar_json(raw_path: &Path, edits: &EditState) -> Value {
    let mut json = Map::new();
    json.insert("version".into(), json!(SIDECAR_VERSION));
    json.insert("software".into(), json!("Project Iris"));
    json.insert("originalFilename".into(), json!(file_name(raw_path)));
    if !edits.applied_preset.is_empty() {
        json.insert("preset".into(), json!(edits.applied_preset));
    }
    json.insert("adjustments".into(), adjustments_to_json(&edits.basic));
    json.insert("toneCurve".into(), tone_curve_to_json(&edits.tone_curve));
    json.insert("hsl".into(), hsl_to_json(&edits.hsl));
    json.insert("masks".into(), masks_to_json(&edits.masks));
    if let Some(crop) = crop_to_json(&edits.crop) {
        json.insert("crop".into(), crop);
    }
    if let Some(detail) = detail_to_json(&edits.detail) {
        json.insert("detail".into(), detail);
    }
    if !edits.retouch.is_empty() {
        json.insert("retouch".into(), retouch_to_json(&edits.retouch));
    }
    Value::Object(json)
}

/// Writes edits for `raw_path` to `sidecar_path` atomically. A favorite flag already in
/// the file is kept.
pub fn write_sidecar(sidecar_path: &Path, raw_path: &Path, edits: &EditState) -> Result<(), Error> {
    let mut json = sidecar_json(raw_path, edits);
    let favorite = sidecar_path
        .exists()
        .then(|| read_json_object(sidecar_path).ok())
        .flatten()
        .and_then(|old| old.get("favorite").cloned());
    if let (Some(favorite), Some(object)) = (favorite, json.as_object_mut()) {
        object.insert("favorite".into(), favorite);
    }
    write_atomically(sidecar_path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::{BrushMode, BrushStroke, CurvePoint, HslBand, HslColor, Mask, MaskPoint, MaskType, WhiteBalance};
    use std::fs;

    const AS_SHOT: WhiteBalance = WhiteBalance { temperature: 5500.0, tint: 10.0 };

    fn with_exposure(ev: f32) -> EditState {
        let mut s = EditState::new(AS_SHOT);
        s.basic.exposure = ev;
        s
    }

    #[test]
    fn sidecar_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("photo.ARW");
        fs::write(&raw, "raw").unwrap();
        assert_eq!(sidecar_path_for(&raw), dir.path().join("photo.iris.json"));

        let mut edits = EditState::new(AS_SHOT);
        edits.basic.exposure = 0.5;
        edits.basic.contrast = 10.0;
        edits.basic.white_balance = WhiteBalance { temperature: 5600.0, tint: 4.0 };
        edits.applied_preset = "Warm Film".into();
        edits.tone_curve.rgb = [(0.0, 0.0), (0.25, 0.20), (0.5, 0.52), (0.75, 0.82), (1.0, 1.0)]
            .map(|(x, y)| CurvePoint::new(x, y))
            .to_vec();
        edits.hsl[HslColor::Blue] = HslBand::new(-10.0, -25.0, -40.0);
        edits.hsl[HslColor::Orange].saturation = 15.0;
        let mut radial = Mask::new(MaskType::Radial, &[]);
        radial.radial =
            iris_core::RadialGradient { x: 0.25, y: 0.75, width: 0.5, height: 0.125, rotation: 30.0, feather: 0.75 };
        radial.invert = true;
        radial.adjustments.exposure = -0.5;
        radial.adjustments.temperature = 20.0;
        let mut brush = Mask::new(MaskType::Brush, std::slice::from_ref(&radial));
        brush.strokes = vec![BrushStroke {
            mode: BrushMode::Subtract,
            radius: 0.0625,
            points: vec![MaskPoint::new(0.125, 0.25), MaskPoint::new(0.5, 0.375)],
            ..Default::default()
        }];
        brush.adjustments.shadows = 40.0;
        edits.masks = vec![radial, brush];
        write_sidecar(&sidecar_path_for(&raw), &raw, &edits).unwrap();

        let read = read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap();
        assert_eq!(read, edits);

        // The documented format.
        let json: Value = serde_json::from_slice(&fs::read(sidecar_path_for(&raw)).unwrap()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["originalFilename"], "photo.ARW");
        assert_eq!(json["adjustments"]["exposure"], 0.5);
        assert_eq!(json["adjustments"]["temperature"], 5600.0);
        let points = json["toneCurve"]["points"].as_array().unwrap();
        assert_eq!(points.len(), 5);
        assert_eq!(points[2][1].as_f64().unwrap(), f64::from(0.52f32));
        let hsl = json["hsl"].as_object().unwrap();
        assert_eq!(hsl.keys().collect::<Vec<_>>(), ["blue", "orange"]); // neutral ranges are omitted
        assert_eq!(hsl["blue"]["luminance"], -40.0);
        let masks = json["masks"].as_array().unwrap();
        assert_eq!(masks.len(), 2);
        assert_eq!(masks[0]["type"], "radial");
        assert_eq!(masks[0]["name"], "Radial 1");
        assert!(masks[0].get("linear").is_none()); // only the mask's own shape
        assert_eq!(masks[1]["strokes"][0]["mode"], "subtract");
    }

    #[test]
    fn crop_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("c.ARW");
        let mut edits = EditState::new(AS_SHOT);
        edits.crop = iris_core::Crop {
            quarter_turns: 1,
            angle: -2.5,
            left: 0.125,
            top: 0.0,
            right: 0.875,
            bottom: 0.75,
            aspect: 1.5,
        };
        write_sidecar(&sidecar_path_for(&raw), &raw, &edits).unwrap();
        assert_eq!(read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap(), edits);
        let json: Value = serde_json::from_slice(&fs::read(sidecar_path_for(&raw)).unwrap()).unwrap();
        assert_eq!(json["crop"]["quarterTurns"], 1);
        assert_eq!(json["crop"]["angle"], -2.5);

        // An uncropped photo has no crop key; out-of-range values are clamped on reading.
        write_sidecar(&sidecar_path_for(&raw), &raw, &EditState::new(AS_SHOT)).unwrap();
        let json: Value = serde_json::from_slice(&fs::read(sidecar_path_for(&raw)).unwrap()).unwrap();
        assert!(json.get("crop").is_none());
        fs::write(
            sidecar_path_for(&raw),
            r#"{"version": 1, "crop": {"quarterTurns": 7, "angle": 90, "left": 0.9, "right": 0.2}}"#,
        )
        .unwrap();
        let crop = read_sidecar(&raw, &EditState::default()).unwrap().unwrap().crop;
        assert_eq!(crop.quarter_turns, 3);
        assert_eq!(crop.angle, 45.0);
        assert!(crop.right > crop.left);
    }

    #[test]
    fn detail_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("d.ARW");
        let mut edits = EditState::new(AS_SHOT);
        edits.detail.sharpening = iris_core::Sharpening { amount: 60.0, radius: 1.5, masking: 30.0 };
        edits.detail.noise_reduction.color = 25.0;
        write_sidecar(&sidecar_path_for(&raw), &raw, &edits).unwrap();
        assert_eq!(read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap(), edits);
        let json: Value = serde_json::from_slice(&fs::read(sidecar_path_for(&raw)).unwrap()).unwrap();
        assert_eq!(json["detail"]["sharpening"]["amount"], 60);
        assert_eq!(json["detail"]["noiseReduction"]["color"], 25);
        // Untouched detail is left out.
        write_sidecar(&sidecar_path_for(&raw), &raw, &EditState::new(AS_SHOT)).unwrap();
        let json: Value = serde_json::from_slice(&fs::read(sidecar_path_for(&raw)).unwrap()).unwrap();
        assert!(json.get("detail").is_none());
    }

    #[test]
    fn retouch_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("r.ARW");
        let mut edits = EditState::new(AS_SHOT);
        edits.retouch = vec![
            iris_core::RetouchStroke {
                mode: iris_core::RetouchMode::Heal,
                offset: MaskPoint::new(0.125, -0.0625),
                radius: 0.03125,
                hardness: 0.75,
                opacity: 0.5,
                flow: 0.25,
                points: vec![MaskPoint::new(0.25, 0.5), MaskPoint::new(0.375, 0.5)],
            },
            iris_core::RetouchStroke { points: vec![MaskPoint::new(0.5, 0.5)], ..Default::default() },
        ];
        write_sidecar(&sidecar_path_for(&raw), &raw, &edits).unwrap();
        assert_eq!(read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap(), edits);
        let json: Value = serde_json::from_slice(&fs::read(sidecar_path_for(&raw)).unwrap()).unwrap();
        assert_eq!(json["retouch"][0]["mode"], "heal");
        assert_eq!(json["retouch"][1]["mode"], "clone");
        // None: no key.
        write_sidecar(&sidecar_path_for(&raw), &raw, &EditState::new(AS_SHOT)).unwrap();
        let json: Value = serde_json::from_slice(&fs::read(sidecar_path_for(&raw)).unwrap()).unwrap();
        assert!(json.get("retouch").is_none());
    }

    #[test]
    fn masks_are_sanitized() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("m.ARW");
        let sidecar = dir.path().join("m.iris.json");
        fs::write(
            &sidecar,
            r#"{"version": 1, "masks": [
            {"type": "radial", "radial": {"feather": 7, "rotation": 450}, "adjustments": {"exposure": 12}},
            {"type": "lasso"},
            {"type": "brush", "strokes": [{"radius": -1, "points": []}, {"mode": "erase", "points": [[0.5, 0.5]]}]}
        ]}"#,
        )
        .unwrap();
        let read = read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap();
        let masks = &read.masks;
        assert_eq!(masks.len(), 2); // unknown type skipped
        assert_eq!(masks[0].name, "Radial 1");
        assert_eq!(masks[0].radial.feather, 1.0);
        assert_eq!(masks[0].radial.rotation, 90.0);
        assert_eq!(masks[0].adjustments.exposure, 4.0);
        assert_eq!(masks[1].strokes.len(), 1); // the stroke without points is dropped
        assert_eq!(masks[1].strokes[0].mode, BrushMode::Erase);

        // Older sidecars have no masks.
        fs::write(&sidecar, r#"{"version": 1, "adjustments": {}}"#).unwrap();
        assert!(read_sidecar(&raw, &EditState::default()).unwrap().unwrap().masks.is_empty());
    }

    #[test]
    fn missing_values_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("a.NEF");
        fs::write(dir.path().join("a.iris.json"), r#"{"version": 1, "adjustments": {"exposure": 9, "shadows": 25}}"#)
            .unwrap();
        let read = read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap();
        assert_eq!(read.basic.exposure, 5.0); // clamped
        assert_eq!(read.basic.shadows, 25.0);
        assert_eq!(read.basic.white_balance, AS_SHOT);
        assert!(read.tone_curve.is_identity());
    }

    #[test]
    fn unreadable_sidecars_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("b.CR2");
        assert!(read_sidecar(&raw, &EditState::default()).unwrap().is_none()); // no sidecar: no error

        fs::write(dir.path().join("b.iris.json"), "{ not json").unwrap();
        assert!(read_sidecar(&raw, &EditState::default()).is_err());

        fs::write(dir.path().join("b.iris.json"), r#"{"version": 99, "adjustments": {}}"#).unwrap();
        assert!(matches!(read_sidecar(&raw, &EditState::default()), Err(Error::NewerVersion)));
    }

    #[test]
    fn sidecar_name_collision() {
        // photo.ARW and photo.CR2 in one folder must not share a sidecar.
        let dir = tempfile::tempdir().unwrap();
        let arw = dir.path().join("photo.ARW");
        let cr2 = dir.path().join("photo.CR2");
        write_sidecar(&sidecar_path_for(&arw), &arw, &with_exposure(1.0)).unwrap();
        assert_eq!(sidecar_path_for(&cr2), dir.path().join("photo.CR2.iris.json"));
        write_sidecar(&sidecar_path_for(&cr2), &cr2, &with_exposure(-1.0)).unwrap();
        let d = EditState::default();
        assert_eq!(read_sidecar(&arw, &d).unwrap().unwrap().basic.exposure, 1.0);
        assert_eq!(read_sidecar(&cr2, &d).unwrap().unwrap().basic.exposure, -1.0);
    }

    #[test]
    fn reads_sidecars_written_by_the_cpp_version() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/cpp-written.iris.json");
        let edits = read_sidecar_file(&path, &EditState::new(WhiteBalance { temperature: 5540.0, tint: 45.0 }))
            .unwrap()
            .unwrap();
        assert_eq!(edits.basic.exposure, 0.5);
        assert_eq!(edits.basic.white_balance, WhiteBalance { temperature: 5600.0, tint: 4.0 });
        assert_eq!(edits.applied_preset, "Warm Film");
        assert_eq!(edits.tone_curve.rgb[3], CurvePoint::new(0.75, 0.82));
        assert_eq!(edits.hsl[HslColor::Blue], HslBand::new(-10.0, -25.0, -40.0));
        assert_eq!(edits.masks.len(), 3);
        assert_eq!(edits.masks[1].linear.angle, -25.0);
        assert_eq!(edits.masks[2].strokes[0].opacity, 0.7);

        // Writing it back gives the same document.
        let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(sidecar_json(Path::new("photo.ARW"), &edits), original);
    }
}
