#include "rendering/Histogram.h"

#include <vector>

namespace iris {

Histogram computeHistogram(const EncodedImage& image)
{
    Histogram total;
    const std::size_t count = std::size_t(image.width) * std::size_t(image.height);
    const int shift = image.bitsPerChannel == 16 ? 8 : 0;
#pragma omp parallel
    {
        Histogram local;
#pragma omp for schedule(static) nowait
        for (std::ptrdiff_t i = 0; i < std::ptrdiff_t(count); ++i) {
            const std::size_t o = std::size_t(i) * 3;
            const unsigned r = (image.bitsPerChannel == 16 ? image.data16[o] : image.data8[o]) >> shift;
            const unsigned g = (image.bitsPerChannel == 16 ? image.data16[o + 1] : image.data8[o + 1]) >> shift;
            const unsigned b = (image.bitsPerChannel == 16 ? image.data16[o + 2] : image.data8[o + 2]) >> shift;
            ++local.red[r];
            ++local.green[g];
            ++local.blue[b];
            ++local.luminance[(54 * r + 183 * g + 19 * b) >> 8]; // Rec.709 luma weights
        }
#pragma omp critical
        for (int k = 0; k < 256; ++k) {
            total.red[k] += local.red[k];
            total.green[k] += local.green[k];
            total.blue[k] += local.blue[k];
            total.luminance[k] += local.luminance[k];
        }
    }
    total.pixels = count;
    return total;
}

} // namespace iris
