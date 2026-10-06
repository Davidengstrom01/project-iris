#include "core/Crop.h"

#include <algorithm>
#include <cmath>

namespace iris {

namespace {

constexpr double kPi = 3.14159265358979323846;
constexpr double kTolerance = 0.02; // pixels a crop corner may lie outside the photo
constexpr float kMinCropFraction = 0.01f;

// Straightened frame -> photo pixels after the quarter turns (the inverse rotation).
struct Straighten {
    double cx, cy, c, s;
    Straighten(int frameWidth, int frameHeight, float angle)
        : cx(frameWidth / 2.0), cy(frameHeight / 2.0), c(std::cos(angle * kPi / 180)), s(std::sin(angle * kPi / 180))
    {
    }
    // Rotates a frame point back (counter-clockwise by angle) onto the turned photo.
    void toPhoto(double x, double y, double& ox, double& oy) const
    {
        const double dx = x - cx, dy = y - cy;
        ox = cx + c * dx + s * dy;
        oy = cy - s * dx + c * dy;
    }
};

} // namespace

Affine Affine::inverted() const
{
    const double det = m11 * m22 - m12 * m21;
    Affine r;
    r.m11 = m22 / det;
    r.m12 = -m12 / det;
    r.m21 = -m21 / det;
    r.m22 = m11 / det;
    r.dx = -(r.m11 * dx + r.m12 * dy);
    r.dy = -(r.m21 * dx + r.m22 * dy);
    return r;
}

Affine operator*(const Affine& a, const Affine& b)
{
    Affine r;
    r.m11 = a.m11 * b.m11 + a.m12 * b.m21;
    r.m12 = a.m11 * b.m12 + a.m12 * b.m22;
    r.m21 = a.m21 * b.m11 + a.m22 * b.m21;
    r.m22 = a.m21 * b.m12 + a.m22 * b.m22;
    r.dx = a.m11 * b.dx + a.m12 * b.dy + a.dx;
    r.dy = a.m21 * b.dx + a.m22 * b.dy + a.dy;
    return r;
}

void frameSize(int photoWidth, int photoHeight, int quarterTurns, int& width, int& height)
{
    const bool swap = quarterTurns % 2 != 0;
    width = swap ? photoHeight : photoWidth;
    height = swap ? photoWidth : photoHeight;
}

CropGeometry cropGeometry(int photoWidth, int photoHeight, const Crop& crop, bool wholeFrame)
{
    const double w = photoWidth, h = photoHeight;
    Affine turn;
    switch (((crop.quarterTurns % 4) + 4) % 4) {
    case 1: turn = {0, -1, 1, 0, h, 0}; break;   // (x, y) -> (h - y, x)
    case 2: turn = {-1, 0, 0, -1, w, h}; break;  // (x, y) -> (w - x, h - y)
    case 3: turn = {0, 1, -1, 0, 0, w}; break;   // (x, y) -> (y, w - x)
    default: break;
    }
    int fw = 0, fh = 0;
    frameSize(photoWidth, photoHeight, crop.quarterTurns, fw, fh);

    // Clockwise rotation about the frame centre (y points down).
    const double a = crop.angle * kPi / 180, c = std::cos(a), s = std::sin(a);
    const double cx = fw / 2.0, cy = fh / 2.0;
    const Affine rotate{c, -s, s, c, cx - c * cx + s * cy, cy - s * cx - c * cy};

    CropGeometry g;
    double x0 = 0, y0 = 0, cw = fw, ch = fh;
    if (!wholeFrame) {
        x0 = crop.left * fw;
        y0 = crop.top * fh;
        cw = (crop.right - crop.left) * fw;
        ch = (crop.bottom - crop.top) * fh;
        if (crop.angle == 0) {
            const double x1 = std::round(x0 + cw), y1 = std::round(y0 + ch);
            x0 = std::round(x0);
            y0 = std::round(y0);
            cw = x1 - x0;
            ch = y1 - y0;
        }
    }
    g.width = std::max(1, int(std::lround(cw)));
    g.height = std::max(1, int(std::lround(ch)));
    g.photoToResult = Affine::translate(-x0, -y0) * rotate * turn;
    return g;
}

bool cropFitsPhoto(const Crop& crop, int photoWidth, int photoHeight)
{
    if (crop.left < -1e-6f || crop.top < -1e-6f || crop.right > 1 + 1e-6f || crop.bottom > 1 + 1e-6f)
        return false;
    int fw = 0, fh = 0;
    frameSize(photoWidth, photoHeight, crop.quarterTurns, fw, fh);
    const Straighten st(fw, fh, crop.angle);
    for (const float fx : {crop.left, crop.right})
        for (const float fy : {crop.top, crop.bottom}) {
            double x = 0, y = 0;
            st.toPhoto(fx * fw, fy * fh, x, y);
            if (x < -kTolerance || y < -kTolerance || x > fw + kTolerance || y > fh + kTolerance)
                return false;
        }
    return true;
}

Crop constrainedCrop(Crop crop, int photoWidth, int photoHeight)
{
    if (photoWidth <= 0 || photoHeight <= 0 || cropFitsPhoto(crop, photoWidth, photoHeight))
        return crop;
    // Scale about the centre (moved into the photo if needed) until it fits.
    double cx = (crop.left + crop.right) / 2, cy = (crop.top + crop.bottom) / 2;
    const double hw = (crop.right - crop.left) / 2, hh = (crop.bottom - crop.top) / 2;
    Crop centre = crop;
    centre.left = centre.right = float(cx);
    centre.top = centre.bottom = float(cy);
    if (!cropFitsPhoto(centre, photoWidth, photoHeight))
        cx = cy = 0.5;
    auto scaled = [&](double k) {
        Crop c = crop;
        c.left = float(cx - hw * k);
        c.right = float(cx + hw * k);
        c.top = float(cy - hh * k);
        c.bottom = float(cy + hh * k);
        return c;
    };
    double lo = 0, hi = 1;
    for (int i = 0; i < 40; ++i) {
        const double mid = (lo + hi) / 2;
        (cropFitsPhoto(scaled(mid), photoWidth, photoHeight) ? lo : hi) = mid;
    }
    return scaled(lo);
}

Crop withAspect(Crop crop, float aspect, int photoWidth, int photoHeight)
{
    int fw = 0, fh = 0;
    frameSize(photoWidth, photoHeight, crop.quarterTurns, fw, fh);
    crop.aspect = aspect;
    if (aspect <= 0)
        return crop;
    // The largest rectangle of this aspect in the frame, centred on the current crop,
    // then shrunk to fit the straightened photo.
    double w = fw, h = fw / aspect;
    if (h > fh) {
        h = fh;
        w = fh * aspect;
    }
    double cx = (crop.left + crop.right) / 2 * fw, cy = (crop.top + crop.bottom) / 2 * fh;
    cx = std::clamp(cx, w / 2, fw - w / 2);
    cy = std::clamp(cy, h / 2, fh - h / 2);
    crop.left = float((cx - w / 2) / fw);
    crop.right = float((cx + w / 2) / fw);
    crop.top = float((cy - h / 2) / fh);
    crop.bottom = float((cy + h / 2) / fh);
    return constrainedCrop(crop, photoWidth, photoHeight);
}

Crop rotatedQuarter(const Crop& crop, bool clockwise)
{
    Crop r = crop;
    r.quarterTurns = ((crop.quarterTurns + (clockwise ? 1 : -1)) % 4 + 4) % 4;
    if (clockwise) { // (u, v) -> (1 - v, u)
        r.left = 1 - crop.bottom;
        r.top = crop.left;
        r.right = 1 - crop.top;
        r.bottom = crop.right;
    } else { // (u, v) -> (v, 1 - u)
        r.left = crop.top;
        r.top = 1 - crop.right;
        r.right = crop.bottom;
        r.bottom = 1 - crop.left;
    }
    if (r.aspect > 0)
        r.aspect = 1 / r.aspect;
    return r;
}

Crop sanitized(Crop crop)
{
    auto finite = [](float v, float fallback) { return std::isfinite(v) ? v : fallback; };
    crop.quarterTurns = ((crop.quarterTurns % 4) + 4) % 4;
    crop.angle = std::clamp(finite(crop.angle, 0), -kMaxStraightenAngle, kMaxStraightenAngle);
    crop.left = std::clamp(finite(crop.left, 0), 0.0f, 1 - kMinCropFraction);
    crop.top = std::clamp(finite(crop.top, 0), 0.0f, 1 - kMinCropFraction);
    crop.right = std::clamp(finite(crop.right, 1), crop.left + kMinCropFraction, 1.0f);
    crop.bottom = std::clamp(finite(crop.bottom, 1), crop.top + kMinCropFraction, 1.0f);
    crop.aspect = std::clamp(finite(crop.aspect, 0), 0.0f, 100.0f);
    return crop;
}

} // namespace iris
