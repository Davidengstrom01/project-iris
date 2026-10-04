#include "rendering/Resample.h"

#include <algorithm>
#include <cmath>

namespace iris {

namespace {

struct Taps {
    int first = 0;
    std::vector<float> weights;
};

// For each destination sample, the overlapping source samples and their coverage weights.
std::vector<Taps> boxTaps(int sourceLength, int destLength)
{
    std::vector<Taps> taps(destLength);
    const double scale = double(sourceLength) / destLength;
    for (int d = 0; d < destLength; ++d) {
        const double start = d * scale;
        const double end = (d + 1) * scale;
        const int first = int(std::floor(start));
        const int last = std::min(int(std::ceil(end)), sourceLength) - 1;
        Taps& t = taps[d];
        t.first = first;
        double total = 0;
        for (int s = first; s <= last; ++s) {
            const double coverage = std::min(end, s + 1.0) - std::max(start, double(s));
            t.weights.push_back(float(coverage));
            total += coverage;
        }
        for (float& w : t.weights)
            w = float(w / total);
    }
    return taps;
}

} // namespace

void fitSize(int width, int height, int maxLongEdge, int& outWidth, int& outHeight)
{
    const int longEdge = std::max(width, height);
    if (maxLongEdge <= 0 || longEdge <= maxLongEdge) {
        outWidth = width;
        outHeight = height;
        return;
    }
    const double factor = double(maxLongEdge) / longEdge;
    outWidth = std::max(1, int(std::lround(width * factor)));
    outHeight = std::max(1, int(std::lround(height * factor)));
}

ImageF resizeArea(const ImageF& source, int width, int height)
{
    width = std::clamp(width, 1, source.width);
    height = std::clamp(height, 1, source.height);
    if (width == source.width && height == source.height)
        return source;

    const std::vector<Taps> xTaps = boxTaps(source.width, width);
    const std::vector<Taps> yTaps = boxTaps(source.height, height);

    // Horizontal pass: source.height rows of `width` pixels.
    ImageF horizontal(width, source.height);
#pragma omp parallel for schedule(static)
    for (int y = 0; y < source.height; ++y) {
        const float* in = source.row(y);
        float* out = horizontal.row(y);
        for (int x = 0; x < width; ++x) {
            const Taps& t = xTaps[x];
            float r = 0, g = 0, b = 0;
            for (std::size_t i = 0; i < t.weights.size(); ++i) {
                const float* px = in + std::size_t(t.first + int(i)) * 3;
                const float w = t.weights[i];
                r += px[0] * w;
                g += px[1] * w;
                b += px[2] * w;
            }
            out[x * 3 + 0] = r;
            out[x * 3 + 1] = g;
            out[x * 3 + 2] = b;
        }
    }

    // Vertical pass.
    ImageF result(width, height);
    const std::size_t rowSamples = std::size_t(width) * 3;
#pragma omp parallel for schedule(static)
    for (int y = 0; y < height; ++y) {
        const Taps& t = yTaps[y];
        float* out = result.row(y);
        std::fill(out, out + rowSamples, 0.0f);
        for (std::size_t i = 0; i < t.weights.size(); ++i) {
            const float* in = horizontal.row(t.first + int(i));
            const float w = t.weights[i];
            for (std::size_t s = 0; s < rowSamples; ++s)
                out[s] += in[s] * w;
        }
    }
    return result;
}

ImageF downscaleToFit(const ImageF& source, int maxLongEdge)
{
    int width = 0, height = 0;
    fitSize(source.width, source.height, maxLongEdge, width, height);
    return resizeArea(source, width, height);
}

} // namespace iris
