#include "rendering/WhiteBalanceTools.h"

#include "core/ColorScience.h"

#include <algorithm>

namespace iris {

namespace {

constexpr float kClipLevel = 0.97f;
constexpr float kMinLevel = 0.002f;

bool usable(const float* px)
{
    const float hi = std::max({px[0], px[1], px[2]});
    const float lo = std::min({px[0], px[1], px[2]});
    return hi < kClipLevel && lo > kMinLevel;
}

} // namespace

WhiteBalance estimateWhiteBalance(const ImageF& source, const WhiteBalance& asShot)
{
    double sum[3] = {0, 0, 0};
    std::size_t count = 0;
    for (std::size_t i = 0; i + 2 < source.pixels.size(); i += 3) {
        const float* px = &source.pixels[i];
        if (!usable(px))
            continue;
        for (int c = 0; c < 3; ++c)
            sum[c] += px[c];
        ++count;
    }
    if (count == 0)
        return asShot;
    return whiteBalanceForNeutral({sum[0] / count, sum[1] / count, sum[2] / count}, asShot);
}

std::optional<WhiteBalance> sampleWhiteBalance(const ImageF& source, double x, double y, const WhiteBalance& asShot)
{
    if (source.empty() || x < 0 || x > 1 || y < 0 || y > 1)
        return std::nullopt;
    // Average a small area (about 0.3% of the long edge) to reduce noise.
    const int radius = std::max(1, std::max(source.width, source.height) / 300);
    const int cx = std::clamp(int(x * source.width), 0, source.width - 1);
    const int cy = std::clamp(int(y * source.height), 0, source.height - 1);
    double sum[3] = {0, 0, 0};
    int count = 0;
    for (int py = std::max(0, cy - radius); py <= std::min(source.height - 1, cy + radius); ++py) {
        for (int px = std::max(0, cx - radius); px <= std::min(source.width - 1, cx + radius); ++px) {
            const float* p = source.row(py) + std::size_t(px) * 3;
            if (!usable(p))
                return std::nullopt;
            for (int c = 0; c < 3; ++c)
                sum[c] += p[c];
            ++count;
        }
    }
    return whiteBalanceForNeutral({sum[0] / count, sum[1] / count, sum[2] / count}, asShot);
}

} // namespace iris
