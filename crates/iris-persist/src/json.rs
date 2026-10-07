//! JSON mapping of the edit state, shared by sidecars and presets. Reading is lenient:
//! missing or mistyped values fall back to defaults and everything is clamped into range.

use iris_core::curve::normalized_curve;
use iris_core::edit_state::ADJUSTMENT_FIELDS;
use iris_core::hsl::HslColor;
use iris_core::mask::{LOCAL_ADJUSTMENT_FIELDS, MAX_MASKS};
use iris_core::{
    BasicAdjustments, BrushMode, BrushStroke, CurvePoint, HslAdjustments, HslBand, Mask, MaskPoint, MaskType, ToneCurve,
};
use serde_json::{Map, Value, json};

/// A double written the way the C++ (Qt) version wrote it: whole numbers without ".0".
fn double(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9e15 { json!(v as i64) } else { json!(v) }
}

/// A float as written by the C++ version (single precision widened to double), so files
/// round-trip exactly.
pub(crate) fn number(v: f32) -> Value {
    double(f64::from(v))
}

/// Mask coordinates are stored with 5 decimals (well below a pixel), keeping sidecars small.
fn rounded(v: f32) -> Value {
    double((f64::from(v) * 1e5).round() / 1e5)
}

pub(crate) fn get_f32(object: &Map<String, Value>, key: &str) -> Option<f32> {
    object.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

fn f32_or(object: Option<&Map<String, Value>>, key: &str, fallback: f32) -> f32 {
    object.and_then(|o| get_f32(o, key)).unwrap_or(fallback)
}

/// `{"exposure": 0.5, "contrast": 10, ..., "temperature": 5600, "tint": 4}`
pub fn adjustments_to_json(adjustments: &BasicAdjustments) -> Value {
    let map: Map<String, Value> =
        ADJUSTMENT_FIELDS.iter().map(|f| (f.key.to_owned(), number(f.get(adjustments)))).collect();
    Value::Object(map)
}

/// Sets the adjustments present in `json`; missing keys keep their current value.
/// Values are clamped to their valid ranges.
pub fn read_adjustments(json: Option<&Value>, adjustments: &mut BasicAdjustments) {
    if let Some(object) = json.and_then(Value::as_object) {
        for field in &ADJUSTMENT_FIELDS {
            if let Some(v) = get_f32(object, field.key) {
                *(field.value)(adjustments) = v;
            }
        }
    }
    *adjustments = adjustments.sanitized();
}

/// `{"points": [[0, 0], [0.25, 0.2], ..., [1, 1]]}`
pub fn tone_curve_to_json(curve: &ToneCurve) -> Value {
    let points: Vec<Value> = curve.rgb.iter().map(|p| json!([number(p.x), number(p.y)])).collect();
    json!({ "points": points })
}

/// Reads a tone curve object; returns `None` if `json` is not a valid curve.
pub fn read_tone_curve(json: Option<&Value>) -> Option<ToneCurve> {
    let points = json?.get("points")?.as_array()?;
    let mut parsed = Vec::with_capacity(points.len());
    for value in points {
        match value.as_array().map(Vec::as_slice) {
            Some([x, y]) => parsed.push(CurvePoint { x: x.as_f64()? as f32, y: y.as_f64()? as f32 }),
            _ => return None,
        }
    }
    Some(ToneCurve { rgb: normalized_curve(&parsed) })
}

/// `{"red": {"hue": 0, "saturation": -20, "luminance": 0}, ...}`; neutral ranges are omitted.
pub fn hsl_to_json(hsl: &HslAdjustments) -> Value {
    let mut map = Map::new();
    for color in HslColor::ALL {
        let band = hsl[color];
        if band != HslBand::default() {
            map.insert(
                color.key().to_owned(),
                json!({
                    "hue": number(band.hue),
                    "saturation": number(band.saturation),
                    "luminance": number(band.luminance),
                }),
            );
        }
    }
    Value::Object(map)
}

/// Reads HSL adjustments; missing ranges and values are 0.
pub fn read_hsl(json: Option<&Value>) -> HslAdjustments {
    let mut hsl = HslAdjustments::default();
    let object = json.and_then(Value::as_object);
    for color in HslColor::ALL {
        let band = object.and_then(|o| o.get(color.key())).and_then(Value::as_object);
        hsl[color] = HslBand {
            hue: f32_or(band, "hue", 0.0),
            saturation: f32_or(band, "saturation", 0.0),
            luminance: f32_or(band, "luminance", 0.0),
        };
    }
    hsl.sanitized()
}

/// `[{"type": "radial", "name": "Radial 1", "invert": false, "radial": {...},
///    "strokes": [{"mode": "add", "radius": 0.05, ..., "points": [[0.1, 0.2], ...]}],
///    "adjustments": {"exposure": 0.5, ...}}, ...]`
/// Only the shape of the mask's own type is written.
pub fn masks_to_json(masks: &[Mask]) -> Value {
    let array = masks
        .iter()
        .map(|mask| {
            let mut object = Map::new();
            object.insert("type".into(), json!(mask.mask_type.key()));
            object.insert("name".into(), json!(mask.name));
            object.insert("invert".into(), json!(mask.invert));
            match mask.mask_type {
                MaskType::Linear => {
                    let l = &mask.linear;
                    object.insert(
                        "linear".into(),
                        json!({"x": rounded(l.x), "y": rounded(l.y), "angle": rounded(l.angle),
                               "feather": rounded(l.feather)}),
                    );
                }
                MaskType::Radial => {
                    let r = &mask.radial;
                    object.insert(
                        "radial".into(),
                        json!({"x": rounded(r.x), "y": rounded(r.y), "width": rounded(r.width),
                               "height": rounded(r.height), "rotation": rounded(r.rotation),
                               "feather": rounded(r.feather)}),
                    );
                }
                MaskType::Brush => {}
            }
            let strokes: Vec<Value> = mask
                .strokes
                .iter()
                .map(|s| {
                    let points: Vec<Value> = s.points.iter().map(|p| json!([rounded(p.x), rounded(p.y)])).collect();
                    json!({"mode": s.mode.key(), "radius": rounded(s.radius), "feather": rounded(s.feather),
                           "opacity": rounded(s.opacity), "points": points})
                })
                .collect();
            object.insert("strokes".into(), Value::Array(strokes));
            let adjustments: Map<String, Value> = LOCAL_ADJUSTMENT_FIELDS
                .iter()
                .filter_map(|f| {
                    let v = f.get(&mask.adjustments);
                    (v != 0.0).then(|| (f.key.to_owned(), number(v)))
                })
                .collect();
            object.insert("adjustments".into(), Value::Object(adjustments));
            Value::Object(object)
        })
        .collect();
    Value::Array(array)
}

/// Reads masks; unknown mask types are skipped. Values are clamped to their valid ranges.
pub fn read_masks(json: Option<&Value>) -> Vec<Mask> {
    let mut masks: Vec<Mask> = Vec::new();
    let Some(array) = json.and_then(Value::as_array) else {
        return masks;
    };
    for value in array {
        let empty = Map::new();
        let object = value.as_object().unwrap_or(&empty);
        let Some(mask_type) = object.get("type").and_then(Value::as_str).and_then(MaskType::from_key) else {
            continue;
        };
        let mut mask = Mask { mask_type, ..Default::default() };
        mask.name = object.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
        if mask.name.is_empty() {
            mask.name = Mask::new(mask_type, &masks).name;
        }
        mask.invert = object.get("invert").and_then(Value::as_bool).unwrap_or(false);

        let l = object.get("linear").and_then(Value::as_object);
        let d = mask.linear;
        mask.linear.x = f32_or(l, "x", d.x);
        mask.linear.y = f32_or(l, "y", d.y);
        mask.linear.angle = f32_or(l, "angle", d.angle);
        mask.linear.feather = f32_or(l, "feather", d.feather);

        let r = object.get("radial").and_then(Value::as_object);
        let d = mask.radial;
        mask.radial.x = f32_or(r, "x", d.x);
        mask.radial.y = f32_or(r, "y", d.y);
        mask.radial.width = f32_or(r, "width", d.width);
        mask.radial.height = f32_or(r, "height", d.height);
        mask.radial.rotation = f32_or(r, "rotation", d.rotation);
        mask.radial.feather = f32_or(r, "feather", d.feather);

        for stroke_value in object.get("strokes").and_then(Value::as_array).into_iter().flatten() {
            let s = stroke_value.as_object();
            let d = BrushStroke::default();
            let mut stroke = BrushStroke {
                mode: BrushMode::from_key(s.and_then(|s| s.get("mode")).and_then(Value::as_str).unwrap_or("")),
                radius: f32_or(s, "radius", d.radius),
                feather: f32_or(s, "feather", d.feather),
                opacity: f32_or(s, "opacity", d.opacity),
                points: Vec::new(),
            };
            let points = s.and_then(|s| s.get("points")).and_then(Value::as_array);
            for point in points.into_iter().flatten() {
                if let Some([x, y]) = point.as_array().map(Vec::as_slice)
                    && let (Some(x), Some(y)) = (x.as_f64(), y.as_f64())
                {
                    stroke.points.push(MaskPoint { x: x as f32, y: y as f32 });
                }
            }
            mask.strokes.push(stroke);
        }

        let adjustments = object.get("adjustments").and_then(Value::as_object);
        for field in &LOCAL_ADJUSTMENT_FIELDS {
            *(field.value)(&mut mask.adjustments) = f32_or(adjustments, field.key, 0.0);
        }

        masks.push(mask.sanitized());
        if masks.len() == MAX_MASKS {
            break;
        }
    }
    masks
}

/// `{"quarterTurns": 1, "angle": -2.5, "left": 0.1, "top": 0, "right": 0.9, "bottom": 1, "aspect": 1.5}`;
/// `None` for an uncropped, unrotated photo (the key is then left out).
pub fn crop_to_json(crop: &iris_core::Crop) -> Option<Value> {
    if crop.is_identity() && crop.aspect == 0.0 {
        return None;
    }
    Some(json!({
        "quarterTurns": crop.quarter_turns,
        // Full precision: a fitted, straightened crop must stay inside the photo.
        "angle": number(crop.angle),
        "left": number(crop.left),
        "top": number(crop.top),
        "right": number(crop.right),
        "bottom": number(crop.bottom),
        "aspect": number(crop.aspect),
    }))
}

/// Reads a crop; missing values are the uncropped defaults. Values are clamped (the
/// rectangle is fitted to the photo once its size is known).
pub fn read_crop(json: Option<&Value>) -> iris_core::Crop {
    let object = json.and_then(Value::as_object);
    let d = iris_core::Crop::default();
    iris_core::Crop {
        quarter_turns: object.and_then(|o| o.get("quarterTurns")).and_then(Value::as_i64).unwrap_or(0) as i32,
        angle: f32_or(object, "angle", d.angle),
        left: f32_or(object, "left", d.left),
        top: f32_or(object, "top", d.top),
        right: f32_or(object, "right", d.right),
        bottom: f32_or(object, "bottom", d.bottom),
        aspect: f32_or(object, "aspect", d.aspect),
    }
    .sanitized()
}

/// `{"amount": 40, "radius": 1, "masking": 0}`
pub fn sharpening_to_json(s: &iris_core::Sharpening) -> Value {
    json!({"amount": number(s.amount), "radius": number(s.radius), "masking": number(s.masking)})
}

/// `{"luminance": 20, "color": 25}`
pub fn noise_reduction_to_json(n: &iris_core::NoiseReduction) -> Value {
    json!({"luminance": number(n.luminance), "color": number(n.color)})
}

pub fn read_sharpening(json: Option<&Value>) -> Option<iris_core::Sharpening> {
    let o = json?.as_object()?;
    let d = iris_core::Sharpening::default();
    Some(iris_core::Sharpening {
        amount: f32_or(Some(o), "amount", d.amount),
        radius: f32_or(Some(o), "radius", d.radius),
        masking: f32_or(Some(o), "masking", d.masking),
    })
}

pub fn read_noise_reduction(json: Option<&Value>) -> Option<iris_core::NoiseReduction> {
    let o = json?.as_object()?;
    Some(iris_core::NoiseReduction {
        luminance: f32_or(Some(o), "luminance", 0.0),
        color: f32_or(Some(o), "color", 0.0),
    })
}

/// `{"sharpening": {...}, "noiseReduction": {...}}`; `None` when untouched (the key is
/// then left out).
pub fn detail_to_json(detail: &iris_core::Detail) -> Option<Value> {
    (*detail != iris_core::Detail::default()).then(|| {
        json!({
            "sharpening": sharpening_to_json(&detail.sharpening),
            "noiseReduction": noise_reduction_to_json(&detail.noise_reduction),
        })
    })
}

/// Reads detail settings; missing values are the defaults (none). Values are clamped.
pub fn read_detail(json: Option<&Value>) -> iris_core::Detail {
    iris_core::Detail {
        sharpening: read_sharpening(json.and_then(|j| j.get("sharpening"))).unwrap_or_default(),
        noise_reduction: read_noise_reduction(json.and_then(|j| j.get("noiseReduction"))).unwrap_or_default(),
    }
    .sanitized()
}

/// `[{"mode": "heal", "offset": [0.05, -0.01], "radius": 0.02, "hardness": 0.5,
///    "opacity": 1, "flow": 1, "points": [[0.4, 0.5], ...]}, ...]`
pub fn retouch_to_json(strokes: &[iris_core::RetouchStroke]) -> Value {
    Value::Array(
        strokes
            .iter()
            .map(|s| {
                let points: Vec<Value> = s.points.iter().map(|p| json!([rounded(p.x), rounded(p.y)])).collect();
                json!({
                    "mode": s.mode.key(),
                    "offset": [rounded(s.offset.x), rounded(s.offset.y)],
                    "radius": rounded(s.radius),
                    "hardness": rounded(s.hardness),
                    "opacity": rounded(s.opacity),
                    "flow": rounded(s.flow),
                    "points": points,
                })
            })
            .collect(),
    )
}

/// Reads retouch strokes; unknown modes and strokes without points are skipped.
pub fn read_retouch(json: Option<&Value>) -> Vec<iris_core::RetouchStroke> {
    let point = |v: &Value| match v.as_array().map(Vec::as_slice) {
        Some([x, y]) => Some(MaskPoint { x: x.as_f64()? as f32, y: y.as_f64()? as f32 }),
        _ => None,
    };
    let strokes = json
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let o = v.as_object()?;
            let mode = iris_core::RetouchMode::from_key(o.get("mode")?.as_str()?)?;
            let d = iris_core::RetouchStroke::default();
            Some(iris_core::RetouchStroke {
                mode,
                offset: o.get("offset").and_then(point).unwrap_or(d.offset),
                radius: f32_or(Some(o), "radius", d.radius),
                hardness: f32_or(Some(o), "hardness", d.hardness),
                opacity: f32_or(Some(o), "opacity", d.opacity),
                flow: f32_or(Some(o), "flow", d.flow),
                points: o.get("points").and_then(Value::as_array).into_iter().flatten().filter_map(point).collect(),
            })
        })
        .collect();
    iris_core::retouch::sanitized_strokes(strokes)
}
