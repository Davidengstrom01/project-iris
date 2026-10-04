#pragma once

#include "core/Hsl.h"

#include <array>

namespace iris {

// Applies HSL adjustments to display-linear Rec.2020 pixels.
//
// Works in Oklab (Ottosson 2020), a perceptual colour space in which hue angles match
// how colours look and lightness is separate from chroma. Each pixel is affected by the
// two colour ranges its hue falls between, blended smoothly; near-neutral pixels (whose
// hue is meaningless) are left alone.
class HslMixer {
public:
    explicit HslMixer(const HslAdjustments& hsl);

    bool active() const { return m_active; }
    void apply(float* rgb) const;

    // Oklab hue angle (degrees) at the centre of each colour range, for tests.
    static const std::array<float, kHslColorCount>& bandCenters();

private:
    bool m_active = false;
    std::array<float, kHslColorCount> m_hueShift{};  // degrees
    std::array<float, kHslColorCount> m_saturation{}; // chroma factor - 1
    std::array<float, kHslColorCount> m_luminance{};  // lightness factor - 1
};

} // namespace iris
