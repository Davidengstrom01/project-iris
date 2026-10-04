#include "core/Curve.h"

#include <algorithm>
#include <cmath>

namespace iris {

CurvePoints linearCurve()
{
    return {{0, 0}, {1, 1}};
}

CurvePoints sCurve()
{
    return {{0, 0}, {0.25f, 0.20f}, {0.5f, 0.5f}, {0.75f, 0.80f}, {1, 1}};
}

CurvePoints inverseSCurve()
{
    return {{0, 0}, {0.25f, 0.30f}, {0.5f, 0.5f}, {0.75f, 0.70f}, {1, 1}};
}

bool isLinear(const CurvePoints& points)
{
    for (const CurvePoint& p : points)
        if (std::abs(p.x - p.y) > 1e-6f)
            return false;
    return points.size() >= 2 && points.front().x <= 1e-6f && points.back().x >= 1 - 1e-6f;
}

CurvePoints normalizedCurve(CurvePoints points)
{
    points.erase(std::remove_if(points.begin(), points.end(),
                                [](const CurvePoint& p) { return !std::isfinite(p.x) || !std::isfinite(p.y); }),
                 points.end());
    for (CurvePoint& p : points) {
        p.x = std::clamp(p.x, 0.0f, 1.0f);
        p.y = std::clamp(p.y, 0.0f, 1.0f);
    }
    std::stable_sort(points.begin(), points.end(), [](const CurvePoint& a, const CurvePoint& b) { return a.x < b.x; });
    CurvePoints result;
    for (const CurvePoint& p : points)
        if (result.empty() || p.x - result.back().x >= kMinCurveGap)
            result.push_back(p);
    if (result.size() < 2 || result.size() > kMaxCurvePoints)
        return linearCurve();
    return result;
}

CurveSpline::CurveSpline(const CurvePoints& input)
{
    const CurvePoints points = normalizedCurve(input);
    const std::size_t n = points.size();
    for (const CurvePoint& p : points) {
        m_x.push_back(p.x);
        m_y.push_back(p.y);
    }

    // Secant slopes, then tangents limited so the interpolant cannot overshoot.
    std::vector<float> secant(n - 1);
    for (std::size_t k = 0; k + 1 < n; ++k)
        secant[k] = (m_y[k + 1] - m_y[k]) / (m_x[k + 1] - m_x[k]);
    m_slope.assign(n, 0.0f);
    m_slope[0] = secant[0];
    m_slope[n - 1] = secant[n - 2];
    for (std::size_t k = 1; k + 1 < n; ++k)
        m_slope[k] = secant[k - 1] * secant[k] <= 0 ? 0.0f : (secant[k - 1] + secant[k]) / 2;
    for (std::size_t k = 0; k + 1 < n; ++k) {
        if (secant[k] == 0) {
            m_slope[k] = m_slope[k + 1] = 0;
            continue;
        }
        const float a = m_slope[k] / secant[k];
        const float b = m_slope[k + 1] / secant[k];
        const float h = a * a + b * b;
        if (h > 9) {
            const float t = 3 / std::sqrt(h);
            m_slope[k] = t * a * secant[k];
            m_slope[k + 1] = t * b * secant[k];
        }
    }
}

float CurveSpline::operator()(float x) const
{
    if (x <= m_x.front())
        return m_y.front();
    if (x >= m_x.back())
        return m_y.back();
    const std::size_t k = std::size_t(std::upper_bound(m_x.begin(), m_x.end(), x) - m_x.begin()) - 1;
    const float h = m_x[k + 1] - m_x[k];
    const float t = (x - m_x[k]) / h;
    const float t2 = t * t, t3 = t2 * t;
    const float y = (2 * t3 - 3 * t2 + 1) * m_y[k] + (t3 - 2 * t2 + t) * h * m_slope[k] +
                    (-2 * t3 + 3 * t2) * m_y[k + 1] + (t3 - t2) * h * m_slope[k + 1];
    return std::clamp(y, 0.0f, 1.0f);
}

} // namespace iris
