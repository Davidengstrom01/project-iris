#include "rendering/Tone.h"

#include "rendering/Resample.h"

#include <algorithm>
#include <cmath>

namespace iris {

namespace {

constexpr double kGamma = 2.2;
constexpr float kMaxInput = 4.0f; // 2 EV above sensor white; brighter values clip anyway
constexpr int kTableSize = 1 << 16;

double smoothstep(double edge0, double edge1, double x)
{
    const double t = std::clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// --- Guided filter helpers -------------------------------------------------------

// Mean over a (2r+1)^2 window, shrinking the window at the borders.
std::vector<float> boxMean(const std::vector<float>& src, int w, int h, int r)
{
    std::vector<float> tmp(src.size());
    std::vector<float> out(src.size());
#pragma omp parallel for schedule(static)
    for (int y = 0; y < h; ++y) {
        const float* in = &src[std::size_t(y) * w];
        float* o = &tmp[std::size_t(y) * w];
        double sum = 0;
        for (int x = 0; x <= std::min(r, w - 1); ++x)
            sum += in[x];
        for (int x = 0; x < w; ++x) {
            const int lo = std::max(0, x - r), hi = std::min(w - 1, x + r);
            o[x] = float(sum / (hi - lo + 1));
            if (x + r + 1 < w)
                sum += in[x + r + 1];
            if (x - r >= 0)
                sum -= in[x - r];
        }
    }
#pragma omp parallel for schedule(static)
    for (int x = 0; x < w; ++x) {
        double sum = 0;
        for (int y = 0; y <= std::min(r, h - 1); ++y)
            sum += tmp[std::size_t(y) * w + x];
        for (int y = 0; y < h; ++y) {
            const int lo = std::max(0, y - r), hi = std::min(h - 1, y + r);
            out[std::size_t(y) * w + x] = float(sum / (hi - lo + 1));
            if (y + r + 1 < h)
                sum += tmp[std::size_t(y + r + 1) * w + x];
            if (y - r >= 0)
                sum -= tmp[std::size_t(y - r) * w + x];
        }
    }
    return out;
}

constexpr int kBaseLongEdge = 1024;     // resolution the base layer is computed at
constexpr double kBaseRadius = 0.025;   // filter radius as a fraction of the long edge
constexpr float kBaseEpsilon = 0.25f;   // edge threshold, in (log2 units)^2
constexpr float kMinLuminance = 1.0f / 65536.0f;

} // namespace

// --- ToneLut ---------------------------------------------------------------------

double ToneLut::evaluateBasic(double v, const BasicAdjustments& a)
{
    double p = std::pow(std::max(v, 0.0), 1.0 / kGamma);

    // Whites: move the white point; only the upper part of the range is affected.
    const double whitePoint = std::pow(std::exp2(-a.whites / 100.0), 1.0 / kGamma);
    const double t = smoothstep(0.25, 1.0, p);
    p *= 1.0 - t + t / whitePoint;

    // Blacks: lift or crush the black point; only the lower part is affected.
    if (p < 0.5)
        p += a.blacks / 100.0 * 0.1 * std::pow(1.0 - p / 0.5, 2.0);

    // Contrast: a power curve on each side of middle grey, meeting with equal slope.
    p = std::clamp(p, 0.0, 1.0);
    const double exponent = 1.0 + a.contrast / 100.0 * 0.6;
    const double pivot = std::pow(0.18, 1.0 / kGamma);
    if (p < pivot)
        p = pivot * std::pow(p / pivot, exponent);
    else
        p = 1.0 - (1.0 - pivot) * std::pow((1.0 - p) / (1.0 - pivot), exponent);

    return std::pow(std::clamp(p, 0.0, 1.0), kGamma);
}

ToneLut::ToneLut(const BasicAdjustments& adjustments, const ToneCurve& curve) : m_table(kTableSize + 1)
{
    m_scale = float(kTableSize) / kMaxInput;
    const bool basicIdentity = adjustments.whites == 0 && adjustments.blacks == 0 && adjustments.contrast == 0;
    const bool curveIdentity = curve.isIdentity();
    const CurveSpline spline(curve.rgb);
#pragma omp parallel for schedule(static)
    for (int i = 0; i <= kTableSize; ++i) {
        const double v = double(i) / m_scale;
        double out = basicIdentity ? std::min(v, 1.0) : evaluateBasic(v, adjustments);
        if (!curveIdentity)
            out = std::pow(double(spline(float(std::pow(out, 1.0 / kGamma)))), kGamma);
        m_table[i] = float(out);
    }
}

void ToneLut::applyHuePreserving(float* rgb) const
{
    const float hi = std::max({rgb[0], rgb[1], rgb[2]});
    const float lo = std::min({rgb[0], rgb[1], rgb[2]});
    const float hiOut = (*this)(hi);
    if (hi - lo < 1e-9f) {
        rgb[0] = rgb[1] = rgb[2] = hiOut;
        return;
    }
    const float loOut = (*this)(lo);
    const float k = (hiOut - loOut) / (hi - lo);
    for (int c = 0; c < 3; ++c)
        rgb[c] = loOut + (rgb[c] - lo) * k;
}

// --- ToneBaseLayer ---------------------------------------------------------------

ToneBaseLayer::ToneBaseLayer(const ImageF& source, const float luminanceWeights[3])
{
    const ImageF small = downscaleToFit(source, kBaseLongEdge);
    m_width = small.width;
    m_height = small.height;
    const std::size_t count = std::size_t(m_width) * m_height;

    std::vector<float> logY(count);
#pragma omp parallel for schedule(static)
    for (std::ptrdiff_t i = 0; i < std::ptrdiff_t(count); ++i) {
        const float* px = &small.pixels[std::size_t(i) * 3];
        const float y = luminanceWeights[0] * px[0] + luminanceWeights[1] * px[1] + luminanceWeights[2] * px[2];
        logY[i] = std::log2(std::max(y, kMinLuminance));
    }

    // Self-guided filter (He et al.): flat regions are smoothed, strong edges kept.
    const int r = std::max(1, int(std::lround(std::max(m_width, m_height) * kBaseRadius)));
    std::vector<float> sq(count);
    for (std::size_t i = 0; i < count; ++i)
        sq[i] = logY[i] * logY[i];
    const std::vector<float> mean = boxMean(logY, m_width, m_height, r);
    const std::vector<float> meanSq = boxMean(sq, m_width, m_height, r);
    std::vector<float> a(count), b(count);
    for (std::size_t i = 0; i < count; ++i) {
        const float variance = std::max(0.0f, meanSq[i] - mean[i] * mean[i]);
        a[i] = variance / (variance + kBaseEpsilon);
        b[i] = mean[i] - a[i] * mean[i];
    }
    m_a = boxMean(a, m_width, m_height, r);
    m_b = boxMean(b, m_width, m_height, r);

    // Bilinear lookup from source pixels into the small grid (pixel-centre aligned).
    auto taps = [](int sourceLength, int gridLength) {
        std::vector<Tap> t(sourceLength);
        const double scale = double(gridLength) / sourceLength;
        for (int i = 0; i < sourceLength; ++i) {
            const double pos = std::clamp((i + 0.5) * scale - 0.5, 0.0, double(gridLength - 1));
            const int i0 = int(pos);
            t[i] = {i0, std::min(i0 + 1, gridLength - 1), float(pos - i0)};
        }
        return t;
    };
    m_xTaps = taps(source.width, m_width);
    m_yTaps = taps(source.height, m_height);
}

} // namespace iris
