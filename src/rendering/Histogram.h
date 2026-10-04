#pragma once

#include "core/Image.h"

#include <array>
#include <cstdint>

namespace iris {

// 256-bin histograms of an output-encoded (sRGB) image.
struct Histogram {
    std::array<std::uint32_t, 256> red{};
    std::array<std::uint32_t, 256> green{};
    std::array<std::uint32_t, 256> blue{};
    std::array<std::uint32_t, 256> luminance{};
    std::uint64_t pixels = 0;
};

Histogram computeHistogram(const EncodedImage& image);

} // namespace iris
