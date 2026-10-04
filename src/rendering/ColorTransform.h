#pragma once

#include <cstddef>

namespace iris {

// Converts linear Rec.2020 working-space pixels to an encoded output colour space
// using LittleCMS. Instances are immutable and safe to use from several threads.
class OutputTransform {
public:
    // Shared sRGB transform producing 8- or 16-bit samples.
    static const OutputTransform& sRGB(int bitsPerChannel);

    ~OutputTransform();
    OutputTransform(const OutputTransform&) = delete;
    OutputTransform& operator=(const OutputTransform&) = delete;

    // in: pixelCount RGB float triplets; out: pixelCount RGB triplets of uint8 or uint16.
    void apply(const float* in, void* out, std::size_t pixelCount) const;

private:
    explicit OutputTransform(int bitsPerChannel);
    void* m_transform = nullptr; // cmsHTRANSFORM
};

} // namespace iris
