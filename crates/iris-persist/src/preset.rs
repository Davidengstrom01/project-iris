use std::collections::BTreeMap;

use iris_core::edit_state::{ADJUSTMENT_FIELDS, find_adjustment_field};
use iris_core::{EditState, HslAdjustments, ToneCurve};
use serde_json::{Map, Value, json};

use crate::Error;
use crate::json::{get_f32, hsl_to_json, number, read_hsl, read_tone_curve, tone_curve_to_json};

pub const PRESET_VERSION: i64 = 1;

/// Key that selects the tone curve in [`Preset::from_edits`].
pub const TONE_CURVE_KEY: &str = "toneCurve";
/// Key that selects the HSL adjustments in [`Preset::from_edits`].
pub const HSL_KEY: &str = "hsl";

/// A reusable set of develop settings. Only the settings a preset contains are changed
/// when it is applied; everything else (and anything photo-specific, such as crop or
/// masks) is left alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub name: String,
    /// Sort key within its folder (used by the built-in presets).
    pub order: i64,
    /// Absolute values by adjustment key ("exposure", "temperature", ...).
    pub values: BTreeMap<String, f32>,
    /// Relative white balance, so a look can warm or cool any photo: in mired
    /// (positive = warmer).
    pub temperature_shift: Option<f32>,
    /// Relative tint, in tint units.
    pub tint_shift: Option<f32>,
    pub tone_curve: Option<ToneCurve>,
    /// All eight colour ranges when present.
    pub hsl: Option<HslAdjustments>,
}

impl Preset {
    /// Applies the preset as a new edit state (the caller records it for undo).
    pub fn apply(&self, state: &EditState) -> EditState {
        let mut result = state.clone();
        let a = &mut result.basic;
        for (key, &value) in &self.values {
            if let Some(field) = find_adjustment_field(key) {
                *(field.value)(a) = value;
            }
        }
        if let Some(shift) = self.temperature_shift {
            let mired = 1e6 / f64::from(a.white_balance.temperature) - f64::from(shift);
            a.white_balance.temperature = (1e6 / mired.max(1.0)) as f32;
        }
        if let Some(shift) = self.tint_shift {
            a.white_balance.tint += shift;
        }
        *a = a.sanitized();
        if let Some(curve) = &self.tone_curve {
            result.tone_curve = curve.clone();
        }
        if let Some(hsl) = self.hsl {
            result.hsl = hsl;
        }
        result.applied_preset = self.name.clone();
        result
    }

    /// Builds a preset from the current edits, keeping only the given keys (adjustment keys
    /// such as "exposure", [`TONE_CURVE_KEY`] or [`HSL_KEY`]).
    pub fn from_edits(name: &str, edits: &EditState, keys: &[&str]) -> Preset {
        let mut preset = Preset { name: name.to_owned(), ..Default::default() };
        for &key in keys {
            if key == TONE_CURVE_KEY {
                preset.tone_curve = Some(edits.tone_curve.clone());
            } else if key == HSL_KEY {
                preset.hsl = Some(edits.hsl);
            } else if let Some(field) = find_adjustment_field(key) {
                preset.values.insert(key.to_owned(), field.get(&edits.basic));
            }
        }
        preset
    }

    pub fn to_json(&self) -> Value {
        let mut adjustments: Map<String, Value> = self.values.iter().map(|(k, &v)| (k.clone(), number(v))).collect();
        if let Some(v) = self.temperature_shift {
            adjustments.insert("temperatureShift".into(), number(v));
        }
        if let Some(v) = self.tint_shift {
            adjustments.insert("tintShift".into(), number(v));
        }

        let mut json = Map::new();
        json.insert("version".into(), json!(PRESET_VERSION));
        json.insert("name".into(), json!(self.name));
        if self.order != 0 {
            json.insert("order".into(), json!(self.order));
        }
        json.insert("adjustments".into(), Value::Object(adjustments));
        if let Some(curve) = &self.tone_curve {
            json.insert("toneCurve".into(), tone_curve_to_json(curve));
        }
        if let Some(hsl) = &self.hsl {
            json.insert("hsl".into(), hsl_to_json(hsl));
        }
        Value::Object(json)
    }

    pub fn from_json(json: &Map<String, Value>) -> Result<Preset, Error> {
        if json.get("version").and_then(Value::as_i64).unwrap_or(0) > PRESET_VERSION {
            return Err(Error::NewerVersion);
        }
        let name = json.get("name").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
        if name.is_empty() {
            return Err(Error::Invalid("preset has no name".into()));
        }
        let mut preset =
            Preset { name, order: json.get("order").and_then(Value::as_i64).unwrap_or(0), ..Default::default() };

        let empty = Map::new();
        let adjustments = json.get("adjustments").and_then(Value::as_object).unwrap_or(&empty);
        for field in &ADJUSTMENT_FIELDS {
            if let Some(v) = get_f32(adjustments, field.key) {
                preset.values.insert(field.key.to_owned(), v.clamp(field.minimum, field.maximum));
            }
        }
        preset.temperature_shift = get_f32(adjustments, "temperatureShift");
        preset.tint_shift = get_f32(adjustments, "tintShift");
        if json.contains_key("toneCurve") {
            preset.tone_curve = Some(
                read_tone_curve(json.get("toneCurve")).ok_or_else(|| Error::Invalid("invalid tone curve".into()))?,
            );
        }
        if json.get("hsl").is_some_and(Value::is_object) {
            preset.hsl = Some(read_hsl(json.get("hsl")));
        }
        Ok(preset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_core::curve::{inverse_s_curve, s_curve};
    use iris_core::{HslColor, Mask, MaskType, WhiteBalance};

    const AS_SHOT: WhiteBalance = WhiteBalance { temperature: 5500.0, tint: 10.0 };

    fn round_trip(p: &Preset) -> Preset {
        Preset::from_json(p.to_json().as_object().unwrap()).unwrap()
    }

    #[test]
    fn preset_changes_only_its_settings() {
        let mut state = EditState::new(AS_SHOT);
        state.basic.exposure = 0.7;
        state.basic.shadows = 30.0;
        state.basic.white_balance = WhiteBalance { temperature: 4800.0, tint: -3.0 };
        state.masks = vec![Mask::new(MaskType::Radial, &[])];

        let preset = Preset {
            name: "Punchy".into(),
            values: [("contrast".to_owned(), 25.0), ("vibrance".to_owned(), 30.0)].into(),
            ..Default::default()
        };
        let applied = preset.apply(&state);
        assert_eq!(applied.basic.contrast, 25.0);
        assert_eq!(applied.basic.vibrance, 30.0);
        assert_eq!(applied.basic.exposure, 0.7);
        assert_eq!(applied.basic.shadows, 30.0);
        assert_eq!(applied.basic.white_balance, state.basic.white_balance);
        assert_eq!(applied.applied_preset, "Punchy");
        assert_eq!(applied.masks, state.masks); // masks are photo-specific
    }

    #[test]
    fn relative_white_balance_shift() {
        let warm =
            Preset { name: "Warm".into(), temperature_shift: Some(20.0), tint_shift: Some(5.0), ..Default::default() };
        let applied = warm.apply(&EditState::new(WhiteBalance { temperature: 5000.0, tint: 0.0 }));
        // 1e6/5000 = 200 mired -> 180 mired = 5556 K
        assert!((applied.basic.white_balance.temperature - 5555.6).abs() < 1.0);
        assert_eq!(applied.basic.white_balance.tint, 5.0);
    }

    #[test]
    fn selective_preset_and_json() {
        let mut edits = EditState::default();
        edits.basic.exposure = 0.3;
        edits.basic.contrast = 12.0;
        edits.basic.white_balance = WhiteBalance { temperature: 6100.0, tint: 7.0 };
        edits.tone_curve.rgb = s_curve();
        let preset = Preset::from_edits("Mine", &edits, &["contrast", "temperature", "tint"]);
        assert_eq!(preset.values.len(), 3);
        assert!(!preset.values.contains_key("exposure"));
        assert!(preset.tone_curve.is_none());

        let with_curve = Preset::from_edits("Curve", &edits, &[TONE_CURVE_KEY]);
        assert!(with_curve.values.is_empty());
        assert_eq!(with_curve.tone_curve.as_ref().unwrap().rgb, s_curve());
        assert_eq!(round_trip(&with_curve), with_curve);
        // Applying a preset without a curve keeps the photo's curve; one with a curve sets it.
        let mut photo = EditState::new(AS_SHOT);
        photo.tone_curve.rgb = inverse_s_curve();
        assert_eq!(preset.apply(&photo).tone_curve.rgb, inverse_s_curve());
        assert_eq!(with_curve.apply(&photo).tone_curve.rgb, s_curve());

        // HSL is included as a whole when selected.
        edits.hsl[HslColor::Green].hue = 30.0;
        let color = Preset::from_edits("Color", &edits, &[HSL_KEY]);
        assert_eq!(color.hsl.unwrap()[HslColor::Green].hue, 30.0);
        assert_eq!(round_trip(&color), color);
        photo.hsl[HslColor::Red].saturation = 50.0;
        assert_eq!(color.apply(&photo).hsl, edits.hsl);
        assert_eq!(preset.apply(&photo).hsl, photo.hsl); // presets without HSL keep it

        assert_eq!(round_trip(&preset), preset);
        assert!(Preset::from_json(json!({"version": 1}).as_object().unwrap()).is_err());
    }

    #[test]
    fn reads_presets_written_by_the_cpp_version() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/cpp-preset.json");
        let original: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let preset = Preset::from_json(original.as_object().unwrap()).unwrap();
        assert_eq!(preset.name, "Mine");
        assert_eq!(preset.temperature_shift, Some(12.0));
        assert_eq!(preset.values.len(), 2);
        assert!(preset.tone_curve.is_some() && preset.hsl.is_some());
        assert_eq!(preset.to_json(), original);
    }
}
