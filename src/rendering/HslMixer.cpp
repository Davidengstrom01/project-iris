#include "rendering/HslMixer.h"

#include "core/ColorScience.h"

#include <algorithm>
#include <cmath>

namespace iris {

namespace {

constexpr float kPi = 3.14159265358979f;
constexpr float kMaxHueShift = 30.0f;      // degrees at +-100
constexpr float kMaxLuminanceChange = 0.35f; // relative Oklab lightness at +-100
// Chroma below which a colour counts as neutral (Oklab units; vivid colours are 0.1-0.3).
constexpr float kNeutralChroma = 0.005f;
constexpr float kFullChroma = 0.04f;

// Oklab matrices, from CIE XYZ (D65).
const Mat3 kXyzToLms = {
    0.8189330101, 0.3618667424, -0.1288597137,
    0.0329845436, 0.9293118715, 0.0361456387,
    0.0482003018, 0.2643662691, 0.6338517070,
};
const Mat3 kLmsToLab = {
    0.2104542553, 0.7936177850, -0.0040720468,
    1.9779984951, -2.4285922050, 0.4505937099,
    0.0259040371, 0.7827717662, -0.8086757660,
};

struct Matrices {
    std::array<float, 9> rgbToLms;
    std::array<float, 9> lmsToLab;
    std::array<float, 9> labToLms;
    std::array<float, 9> lmsToRgb;
};

std::array<float, 9> toFloat(const Mat3& m)
{
    std::array<float, 9> f;
    for (int i = 0; i < 9; ++i)
        f[i] = float(m[i]);
    return f;
}

const Matrices& matrices()
{
    static const Matrices m = [] {
        const Mat3 rgbToLms = kXyzToLms * kRec2020ToXyz;
        return Matrices{toFloat(rgbToLms), toFloat(kLmsToLab), toFloat(inverse(kLmsToLab)), toFloat(inverse(rgbToLms))};
    }();
    return m;
}

inline void multiply(const std::array<float, 9>& m, const float* in, float* out)
{
    out[0] = m[0] * in[0] + m[1] * in[1] + m[2] * in[2];
    out[1] = m[3] * in[0] + m[4] * in[1] + m[5] * in[2];
    out[2] = m[6] * in[0] + m[7] * in[1] + m[8] * in[2];
}

void rgbToOklab(const float* rgb, float* lab)
{
    const Matrices& m = matrices();
    float lms[3];
    multiply(m.rgbToLms, rgb, lms);
    for (float& v : lms)
        v = std::cbrt(v);
    multiply(m.lmsToLab, lms, lab);
}

void oklabToRgb(const float* lab, float* rgb)
{
    const Matrices& m = matrices();
    float lms[3];
    multiply(m.labToLms, lab, lms);
    for (float& v : lms)
        v = v * v * v;
    multiply(m.lmsToRgb, lms, rgb);
}

float smoothstep(float t)
{
    t = std::clamp(t, 0.0f, 1.0f);
    return t * t * (3 - 2 * t);
}

} // namespace

const std::array<float, kHslColorCount>& HslMixer::bandCenters()
{
    // Reference colours (sRGB): red, orange, yellow, green, aqua, blue, purple, magenta.
    static const std::array<float, kHslColorCount> centers = [] {
        const double srgb[kHslColorCount][3] = {
            {1, 0, 0}, {1, 0.5, 0}, {1, 1, 0}, {0, 1, 0}, {0, 1, 1}, {0, 0, 1}, {0.5, 0, 1}, {1, 0, 1},
        };
        const Mat3 srgbToXyz = {0.4124564, 0.3575761, 0.1804375, 0.2126729, 0.7151522,
                                0.0721750, 0.0193339, 0.1191920, 0.9503041};
        std::array<float, kHslColorCount> result{};
        for (int i = 0; i < kHslColorCount; ++i) {
            Vec3 linear;
            for (int c = 0; c < 3; ++c) // decode sRGB
                linear[c] = srgb[i][c] <= 0.04045 ? srgb[i][c] / 12.92 : std::pow((srgb[i][c] + 0.055) / 1.055, 2.4);
            const Vec3 rgb2020 = kXyzToRec2020 * (srgbToXyz * linear);
            const float rgb[3] = {float(rgb2020[0]), float(rgb2020[1]), float(rgb2020[2])};
            float lab[3];
            rgbToOklab(rgb, lab);
            float h = std::atan2(lab[2], lab[1]) * 180.0f / kPi;
            result[i] = h < 0 ? h + 360 : h;
        }
        return result;
    }();
    return centers;
}

HslMixer::HslMixer(const HslAdjustments& hsl)
{
    m_active = !hsl.isNeutral();
    for (int i = 0; i < kHslColorCount; ++i) {
        m_hueShift[i] = hsl.bands[i].hue / 100.0f * kMaxHueShift;
        m_saturation[i] = hsl.bands[i].saturation / 100.0f;
        m_luminance[i] = hsl.bands[i].luminance / 100.0f * kMaxLuminanceChange;
    }
}

void HslMixer::apply(float* rgb) const
{
    float lab[3];
    rgbToOklab(rgb, lab);
    const float chroma = std::hypot(lab[1], lab[2]);
    if (chroma < kNeutralChroma)
        return;
    float hue = std::atan2(lab[2], lab[1]) * 180.0f / kPi;
    if (hue < 0)
        hue += 360;

    // Find the two colour ranges around this hue and blend between them.
    const auto& centers = bandCenters();
    int lower = kHslColorCount - 1;
    for (int i = 0; i < kHslColorCount; ++i)
        if (centers[i] <= hue)
            lower = i;
    const int upper = (lower + 1) % kHslColorCount;
    float gap = centers[upper] - centers[lower];
    float offset = hue - centers[lower];
    if (gap <= 0)
        gap += 360;
    if (offset < 0)
        offset += 360;
    const float wUpper = smoothstep(offset / gap);
    const float wLower = 1 - wUpper;
    const float strength = smoothstep((chroma - kNeutralChroma) / (kFullChroma - kNeutralChroma));

    const float hueShift = (wLower * m_hueShift[lower] + wUpper * m_hueShift[upper]) * strength;
    const float saturation = (wLower * m_saturation[lower] + wUpper * m_saturation[upper]) * strength;
    const float luminance = (wLower * m_luminance[lower] + wUpper * m_luminance[upper]) * strength;
    if (hueShift == 0 && saturation == 0 && luminance == 0)
        return;

    const float newHue = (hue + hueShift) * kPi / 180.0f;
    const float newChroma = chroma * std::max(0.0f, 1 + saturation);
    lab[0] = std::max(0.0f, lab[0] * (1 + luminance));
    lab[1] = newChroma * std::cos(newHue);
    lab[2] = newChroma * std::sin(newHue);
    oklabToRgb(lab, rgb);
    for (int c = 0; c < 3; ++c)
        rgb[c] = std::max(0.0f, rgb[c]);
}

} // namespace iris
