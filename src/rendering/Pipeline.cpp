#include "rendering/Pipeline.h"

#include "core/ColorScience.h"
#include "rendering/ColorTransform.h"
#include "rendering/HslMixer.h"
#include "rendering/Resample.h"
#include "rendering/Tone.h"

#include <algorithm>
#include <cmath>
#include <optional>
#include <vector>

namespace iris {

namespace {

constexpr float kMiddleGreyLog2 = -2.4739312f; // log2(0.18)
constexpr float kMinLuminance = 1.0f / 65536.0f;
constexpr float kMaxShadowsEv = 2.0f;    // lift of the darkest regions at Shadows +100
constexpr float kMaxHighlightsEv = 1.5f; // change of the brightest regions at Highlights ±100

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
    std::optional<ToneBaseLayer> base;
    if (a.shadows != 0 || a.highlights != 0)
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
#pragma omp for schedule(static)
        for (int y = 0; y < input->height; ++y) {
            const float* in = input->row(y);
            for (int x = 0; x < input->width; ++x) {
                const float* s = in + std::size_t(x) * 3;
                float* p = &row[std::size_t(x) * 3];
                p[0] = std::max(0.0f, m[0] * s[0] + m[1] * s[1] + m[2] * s[2]);
                p[1] = std::max(0.0f, m[3] * s[0] + m[4] * s[1] + m[5] * s[2]);
                p[2] = std::max(0.0f, m[6] * s[0] + m[7] * s[1] + m[8] * s[2]);

                if (base) {
                    const float lum = luma[0] * s[0] + luma[1] * s[1] + luma[2] * s[2];
                    const float logY = std::log2(std::max(lum, kMinLuminance));
                    const float k = std::exp2(regional(base->at(x, y, logY) - kMiddleGreyLog2));
                    p[0] *= k;
                    p[1] *= k;
                    p[2] *= k;
                }

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
