#pragma once

#include "core/EditState.h"
#include "core/Image.h"

#include <algorithm>
#include <vector>

namespace iris {

// Global tone curve built from Contrast, Whites and Blacks. Maps scene-linear values
// to display-linear [0, 1]; the shaping happens in a gamma 2.2 perceptual space.
// With all three sliders at zero it is the identity, clipped at 1.
class ToneCurve {
public:
    explicit ToneCurve(const BasicAdjustments& adjustments);

    float operator()(float v) const
    {
        const float position = std::max(v, 0.0f) * m_scale;
        if (position >= float(m_table.size() - 1))
            return m_table.back();
        const int i = int(position);
        const float f = position - float(i);
        return m_table[i] + (m_table[i + 1] - m_table[i]) * f;
    }

    // Applies the curve to the largest and smallest channel and interpolates the middle
    // one, which keeps hue stable (the technique used by Adobe's DNG reference renderer).
    void applyHuePreserving(float* rgb) const;

    // The curve evaluated directly (no table), for tests.
    static double evaluate(double v, const BasicAdjustments& adjustments);

private:
    std::vector<float> m_table;
    float m_scale = 1;
};

// Edge-aware, low-frequency log-luminance of an image, used to steer Highlights and
// Shadows so that they act on regions rather than on individual pixels (preserving
// local detail without halos). It is computed by a guided filter at a fixed small
// resolution, so the preview and the full-resolution export get the same result.
class ToneBaseLayer {
public:
    // luminanceWeights: weights that turn a source pixel into scene luminance after
    // white balance and exposure (row 2 of the luminance-from-working-RGB matrix).
    ToneBaseLayer(const ImageF& source, const float luminanceWeights[3]);

    // Base log2 luminance at pixel (x, y) of the source, given that pixel's own log2 luminance.
    float at(int x, int y, float log2Luminance) const
    {
        const Tap& tx = m_xTaps[x];
        const Tap& ty = m_yTaps[y];
        const std::size_t r0 = std::size_t(ty.i0) * m_width, r1 = std::size_t(ty.i1) * m_width;
        auto sample = [&](const std::vector<float>& g) {
            const float top = g[r0 + tx.i0] + (g[r0 + tx.i1] - g[r0 + tx.i0]) * tx.f;
            const float bottom = g[r1 + tx.i0] + (g[r1 + tx.i1] - g[r1 + tx.i0]) * tx.f;
            return top + (bottom - top) * ty.f;
        };
        return sample(m_a) * log2Luminance + sample(m_b);
    }

private:
    struct Tap {
        int i0, i1;
        float f;
    };

    int m_width = 0;
    int m_height = 0;
    std::vector<float> m_a; // guided filter coefficients (smoothed)
    std::vector<float> m_b;
    std::vector<Tap> m_xTaps;
    std::vector<Tap> m_yTaps;
};

} // namespace iris
