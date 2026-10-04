#pragma once

#include <cstddef>
#include <cstdint>
#include <vector>

namespace iris {

// Scene-referred, linear-light RGB image in the working colour space
// (linear Rec.2020 primaries, D65 white). Interleaved RGB, row-major, no padding.
struct ImageF {
    int width = 0;
    int height = 0;
    std::vector<float> pixels;

    ImageF() = default;
    ImageF(int w, int h) : width(w), height(h), pixels(std::size_t(w) * std::size_t(h) * 3) {}

    bool empty() const { return width <= 0 || height <= 0; }
    float* row(int y) { return pixels.data() + std::size_t(y) * std::size_t(width) * 3; }
    const float* row(int y) const { return pixels.data() + std::size_t(y) * std::size_t(width) * 3; }
};

// Output-referred image, already encoded in its output colour space (e.g. sRGB).
// Interleaved RGB. Exactly one of data8 / data16 is populated, depending on bitsPerChannel.
struct EncodedImage {
    int width = 0;
    int height = 0;
    int bitsPerChannel = 8;  // 8 or 16
    std::vector<std::uint8_t> data8;
    std::vector<std::uint16_t> data16;

    EncodedImage() = default;
    EncodedImage(int w, int h, int bits) : width(w), height(h), bitsPerChannel(bits)
    {
        const std::size_t samples = std::size_t(w) * std::size_t(h) * 3;
        if (bits == 16)
            data16.resize(samples);
        else
            data8.resize(samples);
    }

    void* row(int y)
    {
        const std::size_t offset = std::size_t(y) * std::size_t(width) * 3;
        return bitsPerChannel == 16 ? static_cast<void*>(data16.data() + offset)
                                    : static_cast<void*>(data8.data() + offset);
    }
};

} // namespace iris
