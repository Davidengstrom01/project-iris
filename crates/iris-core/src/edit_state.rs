use crate::color::{MAX_TEMPERATURE, MAX_TINT, MIN_TEMPERATURE};
use crate::{Crop, Detail, HslAdjustments, Mask, RetouchStroke, ToneCurve};

/// Illuminant the photo is balanced for, in Lightroom-style units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhiteBalance {
    /// Kelvin; higher renders warmer.
    pub temperature: f32,
    /// Positive renders more magenta, negative more green.
    pub tint: f32,
}

impl Default for WhiteBalance {
    fn default() -> Self {
        Self { temperature: 6500.0, tint: 0.0 }
    }
}

/// Global develop adjustments. Ranges follow Lightroom conventions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BasicAdjustments {
    pub white_balance: WhiteBalance,
    /// EV, -5..+5
    pub exposure: f32,
    /// -100..+100 (and the same for the rest)
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub vibrance: f32,
    pub saturation: f32,
}

/// Every edit applied to a photo. A plain value: cloning it is cheap enough, and undo/redo
/// keeps snapshots of it. The RAW file itself is never changed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditState {
    pub basic: BasicAdjustments,
    pub tone_curve: ToneCurve,
    pub hsl: HslAdjustments,
    /// Local adjustments, applied in order.
    pub masks: Vec<Mask>,
    pub crop: Crop,
    /// Sharpening and noise reduction.
    pub detail: Detail,
    /// Clone and heal strokes, applied in order before everything else.
    pub retouch: Vec<RetouchStroke>,
    /// Name of the last preset applied, if any.
    pub applied_preset: String,
}

impl EditState {
    /// The untouched state for a photo: all adjustments neutral, white balance as shot.
    pub fn new(as_shot: WhiteBalance) -> Self {
        let mut state = Self::default();
        state.basic.white_balance = as_shot;
        state
    }
}

/// Describes one adjustment so that persistence, presets and the CLI share a single list.
pub struct AdjustmentField {
    /// JSON / CLI name, e.g. "exposure".
    pub key: &'static str,
    /// Display name, e.g. "Exposure".
    pub label: &'static str,
    pub minimum: f32,
    pub maximum: f32,
    pub value: fn(&mut BasicAdjustments) -> &mut f32,
}

impl AdjustmentField {
    pub fn get(&self, a: &BasicAdjustments) -> f32 {
        let mut copy = *a;
        *(self.value)(&mut copy)
    }
}

pub static ADJUSTMENT_FIELDS: [AdjustmentField; 10] = [
    AdjustmentField { key: "exposure", label: "Exposure", minimum: -5.0, maximum: 5.0, value: |a| &mut a.exposure },
    AdjustmentField { key: "contrast", label: "Contrast", minimum: -100.0, maximum: 100.0, value: |a| &mut a.contrast },
    AdjustmentField {
        key: "highlights",
        label: "Highlights",
        minimum: -100.0,
        maximum: 100.0,
        value: |a| &mut a.highlights,
    },
    AdjustmentField { key: "shadows", label: "Shadows", minimum: -100.0, maximum: 100.0, value: |a| &mut a.shadows },
    AdjustmentField { key: "whites", label: "Whites", minimum: -100.0, maximum: 100.0, value: |a| &mut a.whites },
    AdjustmentField { key: "blacks", label: "Blacks", minimum: -100.0, maximum: 100.0, value: |a| &mut a.blacks },
    AdjustmentField {
        key: "temperature",
        label: "Temperature",
        minimum: MIN_TEMPERATURE,
        maximum: MAX_TEMPERATURE,
        value: |a| &mut a.white_balance.temperature,
    },
    AdjustmentField {
        key: "tint",
        label: "Tint",
        minimum: -MAX_TINT,
        maximum: MAX_TINT,
        value: |a| &mut a.white_balance.tint,
    },
    AdjustmentField { key: "vibrance", label: "Vibrance", minimum: -100.0, maximum: 100.0, value: |a| &mut a.vibrance },
    AdjustmentField {
        key: "saturation",
        label: "Saturation",
        minimum: -100.0,
        maximum: 100.0,
        value: |a| &mut a.saturation,
    },
];

pub fn find_adjustment_field(key: &str) -> Option<&'static AdjustmentField> {
    ADJUSTMENT_FIELDS.iter().find(|f| f.key == key)
}

impl BasicAdjustments {
    /// Clamps every adjustment into its valid range (e.g. after reading a file).
    pub fn sanitized(mut self) -> Self {
        for field in &ADJUSTMENT_FIELDS {
            let v = (field.value)(&mut self);
            *v = if v.is_finite() { v.clamp(field.minimum, field.maximum) } else { 0.0 };
        }
        // A temperature of 0 (from a NaN above) is meaningless; fall back to D65-ish.
        if !self.white_balance.temperature.is_finite() || self.white_balance.temperature == 0.0 {
            self.white_balance.temperature = 6500.0;
        }
        self
    }
}

/// Short description of what changed between two states, e.g. "Exposure", "White Balance".
pub fn describe_change(before: &BasicAdjustments, after: &BasicAdjustments) -> &'static str {
    if before.white_balance != after.white_balance {
        let (mut a, mut b) = (*before, *after);
        a.white_balance = WhiteBalance::default();
        b.white_balance = WhiteBalance::default();
        return if a == b { "White Balance" } else { "Basic" };
    }
    let mut changed = None;
    for field in &ADJUSTMENT_FIELDS {
        if field.get(before) != field.get(after) {
            if changed.is_some() {
                return "Basic";
            }
            changed = Some(field.label);
        }
    }
    changed.unwrap_or("Basic")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_changes() {
        let a = BasicAdjustments::default();
        let mut b = a;
        b.exposure = 1.0;
        assert_eq!(describe_change(&a, &b), "Exposure");
        b = a;
        b.white_balance.tint = 5.0;
        assert_eq!(describe_change(&a, &b), "White Balance");
        b.contrast = 3.0;
        assert_eq!(describe_change(&a, &b), "Basic");
    }

    #[test]
    fn sanitizes() {
        let mut a = BasicAdjustments { exposure: 9.0, shadows: f32::NAN, ..Default::default() };
        a.white_balance.temperature = f32::NAN;
        let s = a.sanitized();
        assert_eq!(s.exposure, 5.0);
        assert_eq!(s.shadows, 0.0);
        assert_eq!(s.white_balance.temperature, 6500.0);
    }
}
