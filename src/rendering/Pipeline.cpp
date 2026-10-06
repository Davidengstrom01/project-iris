#include "rendering/Pipeline.h"

#include "core/ColorScience.h"
#include "rendering/ColorTransform.h"
#include "rendering/HslMixer.h"
#include "rendering/MaskCoverage.h"
#include "rendering/Resample.h"
#include "rendering/Tone.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <optional>
#include <vector>

namespace iris {

namespace {

constexpr float kMiddleGreyLog2 = -2.4739312f; // log2(0.18)
constexpr float kMinLuminance = 1.0f / 65536.0f;
constexpr float kMaxShadowsEv = 2.0f;    // lift of the darkest regions at Shadows +100
constexpr float kMaxHighlightsEv = 1.5f; // change of the brightest regions at Highlights ±100
constexpr float kMaxLocalMired = 60.0f;  // white balance shift at local Temperature ±100
constexpr float kMaxLocalContrast = 0.35f; // log-luminance slope change at local Contrast ±100
constexpr float kMaxContrastStops = 8.0f;  // local contrast acts within ±8 EV of middle grey

float smoothstep(float edge0, float edge1, float x)
{
    const float t = std::clamp((x - edge0) / (edge1 - edge0), 0.0f, 1.0f);
    return t * t * (3.0f - 2.0f * t);
}

// Highlights / Shadows: exposure change in EV for a region whose base luminance is
// `ev` stops from middle grey (sensor white is about +2.5).
struct RegionalExposure {
    float shadows;
    float highlights;

    float operator()(float ev) const
    {
        const float shadowWeight = 1.0f - smoothstep(-5.0f, 0.5f, ev);
        const float highlightWeight = smoothstep(-1.0f, 2.5f, ev);
        return shadows * kMaxShadowsEv * shadowWeight + highlights * kMaxHighlightsEv * highlightWeight;
    }
};

// Vibrance / Saturation on display-linear RGB.
struct Presence {
    float saturation; // -1..1
    float vibrance;   // -1..1

    bool active() const { return saturation != 0 || vibrance != 0; }

    static float hueDegrees(const float* rgb, float hi, float lo)
    {
        const float d = hi - lo;
        float h;
        if (hi == rgb[0])
            h = 60.0f * std::fmod((rgb[1] - rgb[2]) / d + 6.0f, 6.0f);
        else if (hi == rgb[1])
            h = 60.0f * ((rgb[2] - rgb[0]) / d + 2.0f);
        else
            h = 60.0f * ((rgb[0] - rgb[1]) / d + 4.0f);
        return h;
    }

    void apply(float* rgb) const
    {
        const float hi = std::max({rgb[0], rgb[1], rgb[2]});
        const float lo = std::min({rgb[0], rgb[1], rgb[2]});
        if (hi <= 0 || hi - lo < 1e-9f)
            return;
        float factor = 1.0f + saturation;
        if (vibrance != 0) {
            // Vibrance favours muted colours and, when boosting, spares skin tones.
            const float currentSaturation = (hi - lo) / hi;
            float amount = vibrance * (1.0f - currentSaturation);
            if (vibrance > 0) {
                const float skin = std::clamp(1.0f - std::abs(hueDegrees(rgb, hi, lo) - 25.0f) / 30.0f, 0.0f, 1.0f);
                amount *= 1.0f - 0.6f * skin;
            }
            factor *= 1.0f + amount;
        }
        const float y = float(kLumaR) * rgb[0] + float(kLumaG) * rgb[1] + float(kLumaB) * rgb[2];
        for (int c = 0; c < 3; ++c)
            rgb[c] = std::max(0.0f, y + (rgb[c] - y) * factor);
    }
};

// One mask's local adjustments, applied in scene-linear light right after the global
// white balance and exposure, so they behave like the global sliders (local exposure can
// recover highlights, for example). Each is scaled by the mask's coverage at the pixel.
struct LocalStage {
    MaskCoverage coverage;
    float exposure;               // EV
    RegionalExposure regional;    // highlights / shadows
    float contrast;               // log-luminance slope change
    float saturation;             // -1..1
    std::optional<std::array<float, 9>> whiteBalance; // relative to the global white balance

    LocalStage(const Mask& mask, int width, int height, const WhiteBalance& asShot, const WhiteBalance& global)
        : coverage(mask, width, height),
          exposure(mask.adjustments.exposure),
          regional{mask.adjustments.shadows / 100.0f, mask.adjustments.highlights / 100.0f},
          contrast(mask.adjustments.contrast / 100.0f * kMaxLocalContrast),
          saturation(mask.adjustments.saturation / 100.0f)
    {
        if (mask.adjustments.temperature != 0) {
            // Shift in mired; fewer mired is a warmer setting.
            const double mired = 1e6 / global.temperature - mask.adjustments.temperature / 100.0 * kMaxLocalMired;
            WhiteBalance shifted = global;
            shifted.temperature =
                float(std::clamp(1e6 / std::max(mired, 1.0), double(kMinTemperature), double(kMaxTemperature)));
            const Mat3 m = whiteBalanceMatrix(asShot, shifted) * inverse(whiteBalanceMatrix(asShot, global));
            whiteBalance.emplace();
            for (int i = 0; i < 9; ++i)
                (*whiteBalance)[i] = float(m[i]);
        }
    }

    bool needsBaseLayer() const { return regional.shadows != 0 || regional.highlights != 0; }

    // baseEv: the region's brightness in EV from middle grey (if known).
    void apply(float* p, float w, float baseEv) const
    {
        if (whiteBalance) {
            const auto& m = *whiteBalance;
            const float q[3] = {m[0] * p[0] + m[1] * p[1] + m[2] * p[2], m[3] * p[0] + m[4] * p[1] + m[5] * p[2],
                                m[6] * p[0] + m[7] * p[1] + m[8] * p[2]};
            for (int c = 0; c < 3; ++c)
                p[c] = std::max(0.0f, p[c] + w * (q[c] - p[c]));
        }
        float ev = w * exposure;
        if (needsBaseLayer())
            ev += w * regional(baseEv + ev);
        float y = float(kLumaR) * p[0] + float(kLumaG) * p[1] + float(kLumaB) * p[2];
        if (contrast != 0 && y > 0) {
            const float stops = std::clamp(std::log2(y) + ev - kMiddleGreyLog2, -kMaxContrastStops, kMaxContrastStops);
            ev += w * contrast * stops;
        }
        if (ev != 0) {
            const float k = std::exp2(ev);
            for (int c = 0; c < 3; ++c)
                p[c] *= k;
            y *= k;
        }
        if (saturation != 0) {
            const float factor = 1 + w * saturation;
            for (int c = 0; c < 3; ++c)
                p[c] = std::max(0.0f, y + (p[c] - y) * factor);
        }
    }
};

} // namespace

EncodedImage render(const ImageF& source, const WhiteBalance& asShot, const EditState& edits,
                    const RenderOptions& options)
{
    // Resize first so every later stage runs at the output resolution.
    const ImageF* input = &source;
    ImageF resized;
    int width = 0, height = 0;
    fitSize(source.width, source.height, options.maxLongEdge, width, height);
    if (width != source.width || height != source.height) {
        resized = resizeArea(source, width, height);
        input = &resized;
    }

    const BasicAdjustments& a = edits.basic;

    // White balance and exposure combined into one matrix.
    const Mat3 wb = whiteBalanceMatrix(asShot, a.whiteBalance);
    const float gain = std::exp2(a.exposure);
    float m[9];
    for (int i = 0; i < 9; ++i)
        m[i] = float(wb[i]) * gain;
    const float luma[3] = {
        float(kLumaR * m[0] + kLumaG * m[3] + kLumaB * m[6]),
        float(kLumaR * m[1] + kLumaG * m[4] + kLumaB * m[7]),
        float(kLumaR * m[2] + kLumaG * m[5] + kLumaB * m[8]),
    };

    const RegionalExposure regional{a.shadows / 100.0f, a.highlights / 100.0f};
    const bool globalRegional = a.shadows != 0 || a.highlights != 0;

    // Masks that change nothing are skipped.
    std::vector<LocalStage> locals;
    for (const Mask& mask : edits.masks)
        if (!mask.adjustments.isNeutral())
            locals.emplace_back(mask, input->width, input->height, asShot, a.whiteBalance);
    const bool localRegional =
        std::any_of(locals.begin(), locals.end(), [](const LocalStage& l) { return l.needsBaseLayer(); });

    std::optional<ToneBaseLayer> base;
    if (globalRegional || localRegional)
        base.emplace(*input, luma);

    // Hue-preserving steps compose, so the basic tone and the RGB curve share one table.
    const ToneLut curve(a, edits.toneCurve);
    const HslMixer hsl(edits.hsl);
    const Presence presence{a.saturation / 100.0f, a.vibrance / 100.0f};

    const int bits = options.bitsPerChannel == 16 ? 16 : 8;
    const OutputTransform& transform = OutputTransform::sRGB(bits);
    EncodedImage output(input->width, input->height, bits);

#pragma omp parallel
    {
        std::vector<float> row(std::size_t(input->width) * 3);
        std::vector<std::vector<float>> coverage(locals.size(), std::vector<float>(input->width));
#pragma omp for schedule(static)
        for (int y = 0; y < input->height; ++y) {
            const float* in = input->row(y);
            for (std::size_t i = 0; i < locals.size(); ++i)
                locals[i].coverage.row(y, coverage[i].data());
            for (int x = 0; x < input->width; ++x) {
                const float* s = in + std::size_t(x) * 3;
                float* p = &row[std::size_t(x) * 3];
                p[0] = std::max(0.0f, m[0] * s[0] + m[1] * s[1] + m[2] * s[2]);
                p[1] = std::max(0.0f, m[3] * s[0] + m[4] * s[1] + m[5] * s[2]);
                p[2] = std::max(0.0f, m[6] * s[0] + m[7] * s[1] + m[8] * s[2]);

                float baseEv = 0;
                if (base) {
                    const float lum = luma[0] * s[0] + luma[1] * s[1] + luma[2] * s[2];
                    const float logY = std::log2(std::max(lum, kMinLuminance));
                    baseEv = base->at(x, y, logY) - kMiddleGreyLog2;
                }
                if (globalRegional) {
                    const float k = std::exp2(regional(baseEv));
                    p[0] *= k;
                    p[1] *= k;
                    p[2] *= k;
                }
                for (std::size_t i = 0; i < locals.size(); ++i)
                    if (const float w = coverage[i][x]; w > 0)
                        locals[i].apply(p, w, baseEv);

                curve.applyHuePreserving(p);
                if (hsl.active())
                    hsl.apply(p);
                if (presence.active())
                    presence.apply(p);
            }
            transform.apply(row.data(), output.row(y), std::size_t(input->width));
        }
    }
    return output;
}

} // namespace iris
