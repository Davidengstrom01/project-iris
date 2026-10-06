#include "rendering/MaskCoverage.h"

#include <algorithm>
#include <cmath>

namespace iris {

namespace {

constexpr float kPi = 3.14159265358979f;

float smoothstep(float edge0, float edge1, float x)
{
    const float t = std::clamp((x - edge0) / (edge1 - edge0), 0.0f, 1.0f);
    return t * t * (3.0f - 2.0f * t);
}

// Distance from p to the segment a-b.
float segmentDistance(float px, float py, float ax, float ay, float bx, float by)
{
    const float dx = bx - ax, dy = by - ay;
    const float lengthSq = dx * dx + dy * dy;
    float t = lengthSq > 0 ? ((px - ax) * dx + (py - ay) * dy) / lengthSq : 0.0f;
    t = std::clamp(t, 0.0f, 1.0f);
    return std::hypot(px - (ax + t * dx), py - (ay + t * dy));
}

} // namespace

MaskCoverage::MaskCoverage(const Mask& mask, int width, int height)
    : m_type(mask.type),
      m_invert(mask.invert),
      m_width(width),
      m_height(height),
      m_longEdge(float(std::max(width, height))),
      m_linear(mask.linear),
      m_radial(mask.radial)
{
    rasterizeStrokes(mask);
}

void MaskCoverage::rasterizeStrokes(const Mask& mask)
{
    if (mask.strokes.empty() || m_width <= 0 || m_height <= 0)
        return;

    struct Rect {
        int x0, y0, x1, y1; // half-open
    };
    auto strokeBounds = [&](const BrushStroke& s) {
        const float r = s.radius * m_longEdge + 1;
        float minX = s.points[0].x * m_width, maxX = minX, minY = s.points[0].y * m_height, maxY = minY;
        for (const MaskPoint& p : s.points) {
            minX = std::min(minX, p.x * m_width);
            maxX = std::max(maxX, p.x * m_width);
            minY = std::min(minY, p.y * m_height);
            maxY = std::max(maxY, p.y * m_height);
        }
        return Rect{std::max(0, int(std::floor(minX - r))), std::max(0, int(std::floor(minY - r))),
                    std::min(m_width, int(std::ceil(maxX + r))), std::min(m_height, int(std::ceil(maxY + r)))};
    };

    // Each layer covers the union of the strokes that can change it.
    auto allocate = [&](Layer& layer, bool (*uses)(BrushMode)) {
        Rect u{m_width, m_height, 0, 0};
        for (const BrushStroke& s : mask.strokes) {
            if (!uses(s.mode))
                continue;
            const Rect b = strokeBounds(s);
            u = {std::min(u.x0, b.x0), std::min(u.y0, b.y0), std::max(u.x1, b.x1), std::max(u.y1, b.y1)};
        }
        if (u.x1 <= u.x0 || u.y1 <= u.y0)
            return;
        layer.x0 = u.x0;
        layer.y0 = u.y0;
        layer.w = u.x1 - u.x0;
        layer.h = u.y1 - u.y0;
        layer.values.assign(std::size_t(layer.w) * layer.h, 0.0f);
    };
    allocate(m_paint, [](BrushMode m) { return m == BrushMode::Add; });
    allocate(m_subtract, [](BrushMode m) { return m == BrushMode::Subtract; });

    std::vector<float> stroke;
    for (const BrushStroke& s : mask.strokes) {
        Layer* target = s.mode == BrushMode::Add ? &m_paint : s.mode == BrushMode::Subtract ? &m_subtract : nullptr;
        if (target && target->empty())
            continue;
        if (!target && m_paint.empty() && m_subtract.empty())
            continue; // nothing to erase
        const Rect b = strokeBounds(s);
        const int w = b.x1 - b.x0, h = b.y1 - b.y0;
        if (w <= 0 || h <= 0)
            continue;

        // Coverage of this stroke alone: the strongest of its segments, so a stroke does
        // not build up where it crosses itself.
        const float radius = s.radius * m_longEdge;
        const float inner = std::max(0.0f, std::min(radius * (1 - s.feather), radius - 1.0f));
        std::vector<float> px(s.points.size()), py(s.points.size());
        for (std::size_t i = 0; i < s.points.size(); ++i) {
            px[i] = s.points[i].x * m_width;
            py[i] = s.points[i].y * m_height;
        }
        const std::size_t segments = std::max<std::size_t>(1, s.points.size() - 1);
        stroke.assign(std::size_t(w) * h, 0.0f);
#pragma omp parallel for schedule(dynamic, 16)
        for (int y = b.y0; y < b.y1; ++y) {
            float* out = &stroke[std::size_t(y - b.y0) * w];
            const float cy = y + 0.5f;
            for (std::size_t i = 0; i < segments; ++i) {
                const std::size_t j = std::min(i + 1, s.points.size() - 1);
                if (cy < std::min(py[i], py[j]) - radius || cy > std::max(py[i], py[j]) + radius)
                    continue;
                const int xa = std::max(b.x0, int(std::floor(std::min(px[i], px[j]) - radius)));
                const int xb = std::min(b.x1, int(std::ceil(std::max(px[i], px[j]) + radius)));
                for (int x = xa; x < xb; ++x) {
                    const float d = segmentDistance(x + 0.5f, cy, px[i], py[i], px[j], py[j]);
                    if (d < radius)
                        out[x - b.x0] = std::max(out[x - b.x0], 1.0f - smoothstep(inner, radius, d));
                }
            }
        }

        // Composite the stroke into its layer(s).
        auto composite = [&](Layer& layer, bool erase) {
            if (layer.empty())
                return;
            const int y0 = std::max(b.y0, layer.y0), y1 = std::min(b.y1, layer.y0 + layer.h);
            const int x0 = std::max(b.x0, layer.x0), x1 = std::min(b.x1, layer.x0 + layer.w);
            for (int y = y0; y < y1; ++y) {
                const float* in = &stroke[std::size_t(y - b.y0) * w];
                float* out = &layer.values[std::size_t(y - layer.y0) * layer.w];
                for (int x = x0; x < x1; ++x) {
                    const float v = in[x - b.x0] * s.opacity;
                    float& o = out[x - layer.x0];
                    o = erase ? o * (1 - v) : o + v * (1 - o);
                }
            }
        };
        if (target) {
            composite(*target, false);
        } else {
            composite(m_paint, true);
            composite(m_subtract, true);
        }
    }
}

void MaskCoverage::row(int y, float* out) const
{
    const float cy = y + 0.5f;
    switch (m_type) {
    case MaskType::Brush:
        std::fill(out, out + m_width, 0.0f);
        break;
    case MaskType::Linear: {
        // Signed distance from the centre line, positive towards the affected side.
        const float a = m_linear.angle * kPi / 180;
        const float nx = -std::sin(a), ny = -std::cos(a);
        const float half = std::max(0.5f, m_linear.feather * m_longEdge / 2);
        const float cx0 = m_linear.x * m_width, cy0 = m_linear.y * m_height;
        for (int x = 0; x < m_width; ++x) {
            const float d = (x + 0.5f - cx0) * nx + (cy - cy0) * ny;
            out[x] = smoothstep(-half, half, d);
        }
        break;
    }
    case MaskType::Radial: {
        const float a = m_radial.rotation * kPi / 180;
        const float ux = std::cos(a), uy = -std::sin(a); // ellipse axes on screen
        const float vx = std::sin(a), vy = std::cos(a);
        const float rx = std::max(0.5f, m_radial.width * m_longEdge / 2);
        const float ry = std::max(0.5f, m_radial.height * m_longEdge / 2);
        const float edge0 = std::min(1 - m_radial.feather, 1 - 1 / std::min(rx, ry));
        const float cx0 = m_radial.x * m_width, cy0 = m_radial.y * m_height;
        for (int x = 0; x < m_width; ++x) {
            const float dx = x + 0.5f - cx0, dy = cy - cy0;
            const float lx = (dx * ux + dy * uy) / rx, ly = (dx * vx + dy * vy) / ry;
            out[x] = 1 - smoothstep(edge0, 1, std::sqrt(lx * lx + ly * ly));
        }
        break;
    }
    }

    // Shape + paint (screen), inverted, minus Subtract strokes.
    if (const float* paint = m_paint.row(y)) {
        for (int x = m_paint.x0; x < m_paint.x0 + m_paint.w; ++x) {
            const float p = paint[x - m_paint.x0];
            out[x] += p * (1 - out[x]);
        }
    }
    if (m_invert)
        for (int x = 0; x < m_width; ++x)
            out[x] = 1 - out[x];
    if (const float* subtract = m_subtract.row(y))
        for (int x = m_subtract.x0; x < m_subtract.x0 + m_subtract.w; ++x)
            out[x] *= 1 - subtract[x - m_subtract.x0];
}

std::vector<std::uint8_t> renderMaskCoverage(const Mask& mask, int width, int height)
{
    std::vector<std::uint8_t> result(std::size_t(width) * std::max(0, height));
    if (width <= 0 || height <= 0)
        return result;
    const MaskCoverage coverage(mask, width, height);
#pragma omp parallel
    {
        std::vector<float> row(width);
#pragma omp for schedule(static)
        for (int y = 0; y < height; ++y) {
            coverage.row(y, row.data());
            std::uint8_t* out = &result[std::size_t(y) * width];
            for (int x = 0; x < width; ++x)
                out[x] = std::uint8_t(std::lround(std::clamp(row[x], 0.0f, 1.0f) * 255));
        }
    }
    return result;
}

} // namespace iris
