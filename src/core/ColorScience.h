#pragma once

#include "core/EditState.h"

#include <array>

namespace iris {

using Vec3 = std::array<double, 3>;
using Mat3 = std::array<double, 9>; // row-major

Mat3 operator*(const Mat3& a, const Mat3& b);
Vec3 operator*(const Mat3& m, const Vec3& v);
Mat3 inverse(const Mat3& m);

struct Chromaticity {
    double x = 0;
    double y = 0;
};

inline constexpr Chromaticity kD65{0.3127, 0.3290};

// Linear Rec.2020 (D65) <-> CIE XYZ.
extern const Mat3 kRec2020ToXyz;
extern const Mat3 kXyzToRec2020;
// Luminance weights of linear Rec.2020.
inline constexpr double kLumaR = 0.2627, kLumaG = 0.6780, kLumaB = 0.0593;

inline constexpr float kMinTemperature = 2000;
inline constexpr float kMaxTemperature = 15000;
inline constexpr float kMaxTint = 150;

Chromaticity chromaticity(const Vec3& xyz);

// White point of a temperature/tint pair: a point on the Planckian locus, offset
// perpendicular to it by the tint (1 tint unit = 1/3000 Duv).
Chromaticity whitePoint(const WhiteBalance& wb);

// Inverse of whitePoint(), clamped to the supported range.
WhiteBalance whiteBalanceFromWhitePoint(Chromaticity white);

// Bradford chromatic adaptation in XYZ.
Mat3 bradfordAdaptation(Chromaticity from, Chromaticity to);

// Working-space (linear Rec.2020) matrix that re-balances an image decoded with the
// as-shot white balance so that it renders with the target white balance.
Mat3 whiteBalanceMatrix(const WhiteBalance& asShot, const WhiteBalance& target);

// White balance that makes the given working-space colour (from an image decoded with
// the as-shot white balance) render as neutral grey.
WhiteBalance whiteBalanceForNeutral(const Vec3& workingRgb, const WhiteBalance& asShot);

} // namespace iris
