/// The eight colour ranges of the HSL controls, in hue order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HslColor {
    Red,
    Orange,
    Yellow,
    Green,
    Aqua,
    Blue,
    Purple,
    Magenta,
}

pub const HSL_COLOR_COUNT: usize = 8;

impl HslColor {
    pub const ALL: [HslColor; HSL_COLOR_COUNT] = [
        HslColor::Red,
        HslColor::Orange,
        HslColor::Yellow,
        HslColor::Green,
        HslColor::Aqua,
        HslColor::Blue,
        HslColor::Purple,
        HslColor::Magenta,
    ];

    /// "red", "orange", ... (file format keys)
    pub fn key(self) -> &'static str {
        ["red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta"][self as usize]
    }

    /// "Red", "Orange", ... (display names)
    pub fn name(self) -> &'static str {
        ["Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"][self as usize]
    }
}

/// Adjustments for one colour range, each -100..+100.
///   hue:        shifts the colour towards its neighbour (+ = towards the next range in the
///               list, e.g. red towards orange)
///   saturation: -100 removes the colour, +100 doubles its chroma
///   luminance:  darkens or brightens the colour
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HslBand {
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
}

impl HslBand {
    pub const fn new(hue: f32, saturation: f32, luminance: f32) -> Self {
        Self { hue, saturation, luminance }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HslAdjustments {
    pub bands: [HslBand; HSL_COLOR_COUNT],
}

impl std::ops::Index<HslColor> for HslAdjustments {
    type Output = HslBand;
    fn index(&self, c: HslColor) -> &HslBand {
        &self.bands[c as usize]
    }
}

impl std::ops::IndexMut<HslColor> for HslAdjustments {
    fn index_mut(&mut self, c: HslColor) -> &mut HslBand {
        &mut self.bands[c as usize]
    }
}

impl HslAdjustments {
    pub fn is_neutral(&self) -> bool {
        *self == Self::default()
    }

    /// Clamps all values to -100..+100 (e.g. after reading a file).
    pub fn sanitized(mut self) -> Self {
        for band in &mut self.bands {
            for v in [&mut band.hue, &mut band.saturation, &mut band.luminance] {
                *v = crate::clean(*v, -100.0, 100.0, 0.0);
            }
        }
        self
    }
}
