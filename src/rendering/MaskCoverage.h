#pragma once

#include "core/Mask.h"

#include <cstdint>
#include <vector>

namespace iris {

// How strongly a mask applies at each pixel of an image of a given size, 0..1.
//
// Gradients are evaluated analytically per pixel; brush strokes are rasterised once at
// the image's resolution (only over the area they cover). Because mask geometry is
// resolution-independent, the preview and the export get the same mask.
class MaskCoverage {
public:
    MaskCoverage(const Mask& mask, int width, int height);

    // Writes the coverage of row y to out[0 .. width).
    void row(int y, float* out) const;

private:
    // A rasterised brush layer covering [x0, x0 + w) x [y0, y0 + h); 0 elsewhere.
    struct Layer {
        int x0 = 0, y0 = 0, w = 0, h = 0;
        std::vector<float> values;

        bool empty() const { return values.empty(); }
        const float* row(int y) const
        {
            return y >= y0 && y < y0 + h ? values.data() + std::size_t(y - y0) * w : nullptr;
        }
    };

    void rasterizeStrokes(const Mask& mask);

    MaskType m_type;
    bool m_invert;
    int m_width;
    int m_height;
    float m_longEdge;
    LinearGradient m_linear;
    RadialGradient m_radial;
    Layer m_paint;    // Add strokes, minus Erase strokes
    Layer m_subtract; // Subtract strokes, minus Erase strokes
};

// The coverage of a mask as an 8-bit greyscale image (255 = full effect), for overlays.
std::vector<std::uint8_t> renderMaskCoverage(const Mask& mask, int width, int height);

} // namespace iris
