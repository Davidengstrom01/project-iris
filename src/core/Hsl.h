#pragma once

#include <array>

namespace iris {

// The eight colour ranges of the HSL controls, in hue order.
enum class HslColor { Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta };
inline constexpr int kHslColorCount = 8;

// "red", "orange", ... (file format keys)
const char* hslColorKey(HslColor color);
// "Red", "Orange", ... (display names)
const char* hslColorName(HslColor color);

// Adjustments for one colour range, each -100..+100.
//   hue:        shifts the colour towards its neighbour (+ = towards the next range in the
//               list, e.g. red towards orange)
//   saturation: -100 removes the colour, +100 doubles its chroma
//   luminance:  darkens or brightens the colour
struct HslBand {
    float hue = 0;
    float saturation = 0;
    float luminance = 0;

    bool operator==(const HslBand&) const = default;
};

struct HslAdjustments {
    std::array<HslBand, kHslColorCount> bands{};

    HslBand& operator[](HslColor c) { return bands[int(c)]; }
    const HslBand& operator[](HslColor c) const { return bands[int(c)]; }
    bool isNeutral() const { return *this == HslAdjustments{}; }
    bool operator==(const HslAdjustments&) const = default;
};

// Clamps all values to -100..+100 (e.g. after reading a file).
HslAdjustments sanitized(HslAdjustments hsl);

} // namespace iris
