#pragma once

#include <cstddef>
#include <vector>

namespace iris {

// A point on a tone curve; x = input, y = output, both 0..1 in perceptual (gamma) units.
struct CurvePoint {
    float x = 0;
    float y = 0;

    bool operator==(const CurvePoint&) const = default;
};

using CurvePoints = std::vector<CurvePoint>;

inline constexpr float kMinCurveGap = 0.01f;   // minimum horizontal distance between points
inline constexpr std::size_t kMaxCurvePoints = 16;

CurvePoints linearCurve();
CurvePoints sCurve();        // more contrast: slightly lower shadows, raised highlights
CurvePoints inverseSCurve(); // less contrast
bool isLinear(const CurvePoints& points);

// Sorted by x, clamped to [0, 1], points at least kMinCurveGap apart, at least two points
// and at most kMaxCurvePoints. Anything unusable becomes the linear curve.
CurvePoints normalizedCurve(CurvePoints points);

// A smooth curve through the points that never overshoots between them (monotone cubic
// Hermite interpolation, Fritsch-Carlson). Flat beyond the first and last point.
class CurveSpline {
public:
    explicit CurveSpline(const CurvePoints& points);
    float operator()(float x) const;

private:
    std::vector<float> m_x;
    std::vector<float> m_y;
    std::vector<float> m_slope;
};

// The tone curves of a photo. Only the RGB (master) curve exists for now; separate red,
// green and blue curves can be added as further members.
struct ToneCurve {
    CurvePoints rgb = linearCurve();

    bool isIdentity() const { return isLinear(rgb); }
    bool operator==(const ToneCurve&) const = default;
};

} // namespace iris
