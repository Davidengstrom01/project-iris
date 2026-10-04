#include "core/ColorScience.h"

#include <algorithm>
#include <cmath>

namespace iris {

const Mat3 kRec2020ToXyz = {
    0.6369580, 0.1446169, 0.1688810,
    0.2627002, 0.6779981, 0.0593017,
    0.0000000, 0.0280727, 1.0609851,
};
const Mat3 kXyzToRec2020 = inverse(kRec2020ToXyz);

namespace {

const Mat3 kBradford = {
    0.8951, 0.2664, -0.1614,
    -0.7502, 1.7135, 0.0367,
    0.0389, -0.0685, 1.0296,
};

constexpr double kDuvPerTint = 1.0 / 3000.0;

struct Uv {
    double u = 0;
    double v = 0;
};

// Planckian locus in CIE 1960 uv (Krystek 1985, valid 1000-15000 K).
Uv planckian(double t)
{
    const double u = (0.860117757 + 1.54118254e-4 * t + 1.28641212e-7 * t * t) /
                     (1.0 + 8.42420235e-4 * t + 7.08145163e-7 * t * t);
    const double v = (0.317398726 + 4.22806245e-5 * t + 4.20481691e-8 * t * t) /
                     (1.0 - 2.89741816e-5 * t + 1.61456053e-7 * t * t);
    return {u, v};
}

// Unit normal to the locus at temperature t, pointing towards green (+v).
Uv locusNormal(double t)
{
    const Uv a = planckian(t - 1.0);
    const Uv b = planckian(t + 1.0);
    double nu = -(b.v - a.v);
    double nv = b.u - a.u;
    const double length = std::hypot(nu, nv);
    nu /= length;
    nv /= length;
    if (nv < 0) {
        nu = -nu;
        nv = -nv;
    }
    return {nu, nv};
}

Uv toUv(Chromaticity c)
{
    const double d = -2.0 * c.x + 12.0 * c.y + 3.0;
    return {4.0 * c.x / d, 6.0 * c.y / d};
}

Chromaticity fromUv(Uv p)
{
    const double d = 2.0 * p.u - 8.0 * p.v + 4.0;
    return {3.0 * p.u / d, 2.0 * p.v / d};
}

Vec3 toXyz(Chromaticity c)
{
    return {c.x / c.y, 1.0, (1.0 - c.x - c.y) / c.y};
}

double distanceToLocus(Uv p, double t)
{
    const Uv l = planckian(t);
    return std::hypot(p.u - l.u, p.v - l.v);
}

} // namespace

Mat3 operator*(const Mat3& a, const Mat3& b)
{
    Mat3 r{};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j)
            for (int k = 0; k < 3; ++k)
                r[i * 3 + j] += a[i * 3 + k] * b[k * 3 + j];
    return r;
}

Vec3 operator*(const Mat3& m, const Vec3& v)
{
    return {m[0] * v[0] + m[1] * v[1] + m[2] * v[2],
            m[3] * v[0] + m[4] * v[1] + m[5] * v[2],
            m[6] * v[0] + m[7] * v[1] + m[8] * v[2]};
}

Mat3 inverse(const Mat3& m)
{
    const double c00 = m[4] * m[8] - m[5] * m[7];
    const double c01 = m[5] * m[6] - m[3] * m[8];
    const double c02 = m[3] * m[7] - m[4] * m[6];
    const double det = m[0] * c00 + m[1] * c01 + m[2] * c02;
    const double s = 1.0 / det;
    return {
        c00 * s, (m[2] * m[7] - m[1] * m[8]) * s, (m[1] * m[5] - m[2] * m[4]) * s,
        c01 * s, (m[0] * m[8] - m[2] * m[6]) * s, (m[2] * m[3] - m[0] * m[5]) * s,
        c02 * s, (m[1] * m[6] - m[0] * m[7]) * s, (m[0] * m[4] - m[1] * m[3]) * s,
    };
}

Chromaticity chromaticity(const Vec3& xyz)
{
    const double sum = xyz[0] + xyz[1] + xyz[2];
    return {xyz[0] / sum, xyz[1] / sum};
}

Chromaticity whitePoint(const WhiteBalance& wb)
{
    const double t = std::clamp<double>(wb.temperature, 1000.0, 15000.0);
    const Uv l = planckian(t);
    const Uv n = locusNormal(t);
    const double duv = wb.tint * kDuvPerTint;
    return fromUv({l.u + n.u * duv, l.v + n.v * duv});
}

WhiteBalance whiteBalanceFromWhitePoint(Chromaticity white)
{
    const Uv p = toUv(white);
    // Search in mired (perceptually even) for the closest locus point, then refine.
    constexpr double minMired = 1e6 / 15000.0, maxMired = 1e6 / 1000.0;
    constexpr int steps = 2000;
    double best = minMired;
    double bestDistance = 1e9;
    for (int i = 0; i <= steps; ++i) {
        const double mired = minMired + (maxMired - minMired) * i / steps;
        const double d = distanceToLocus(p, 1e6 / mired);
        if (d < bestDistance) {
            bestDistance = d;
            best = mired;
        }
    }
    double lo = std::max(minMired, best - (maxMired - minMired) / steps);
    double hi = std::min(maxMired, best + (maxMired - minMired) / steps);
    for (int i = 0; i < 60; ++i) {
        const double m1 = lo + (hi - lo) / 3, m2 = hi - (hi - lo) / 3;
        if (distanceToLocus(p, 1e6 / m1) < distanceToLocus(p, 1e6 / m2))
            hi = m2;
        else
            lo = m1;
    }
    const double t = 1e6 / ((lo + hi) / 2);
    const Uv l = planckian(t);
    const Uv n = locusNormal(t);
    const double duv = (p.u - l.u) * n.u + (p.v - l.v) * n.v;

    WhiteBalance wb;
    wb.temperature = std::clamp(float(t), kMinTemperature, kMaxTemperature);
    wb.tint = std::clamp(float(duv / kDuvPerTint), -kMaxTint, kMaxTint);
    return wb;
}

Mat3 bradfordAdaptation(Chromaticity from, Chromaticity to)
{
    const Vec3 src = kBradford * toXyz(from);
    const Vec3 dst = kBradford * toXyz(to);
    const Mat3 scale = {dst[0] / src[0], 0, 0, 0, dst[1] / src[1], 0, 0, 0, dst[2] / src[2]};
    return inverse(kBradford) * scale * kBradford;
}

Mat3 whiteBalanceMatrix(const WhiteBalance& asShot, const WhiteBalance& target)
{
    // The decoder maps the as-shot illuminant to the working white (D65). Undo that,
    // then adapt from the target illuminant instead.
    const Mat3 undoAsShot = bradfordAdaptation(kD65, whitePoint(asShot));
    const Mat3 applyTarget = bradfordAdaptation(whitePoint(target), kD65);
    return kXyzToRec2020 * applyTarget * undoAsShot * kRec2020ToXyz;
}

WhiteBalance whiteBalanceForNeutral(const Vec3& workingRgb, const WhiteBalance& asShot)
{
    // In scene terms the sample was lit by this illuminant; choosing it as the target
    // white balance maps the sample to the working white.
    const Vec3 scene = bradfordAdaptation(kD65, whitePoint(asShot)) * (kRec2020ToXyz * workingRgb);
    return whiteBalanceFromWhitePoint(chromaticity(scene));
}

} // namespace iris
