#include "ui/MaskEditor.h"

#include <QObject>
#include <QPainter>
#include <QPainterPath>
#include <QtMath>

#include <algorithm>
#include <cmath>

namespace iris::ui {

namespace {

constexpr double kHandleRadius = 5;   // widget pixels
constexpr double kHitDistance = 10;   // widget pixels
constexpr double kRotationArm = 28;   // widget pixels beyond the ellipse
constexpr double kMinCreateDrag = 4;  // widget pixels before a drag creates a gradient
constexpr double kStrokeSpacing = 0.25; // of the brush radius, between recorded points (segments are exact)

double length(const QPointF& p)
{
    return std::hypot(p.x(), p.y());
}

// Direction perpendicular to a gradient, pointing to its full-effect side (image pixels,
// y down). Angles are counter-clockwise on screen; 0 points up.
QPointF normalFor(double degrees)
{
    const double a = qDegreesToRadians(degrees);
    return {-std::sin(a), -std::cos(a)};
}

double angleOf(const QPointF& normal)
{
    return qRadiansToDegrees(std::atan2(-normal.x(), -normal.y()));
}

void drawHandle(QPainter& p, const QPointF& at, bool filled = false)
{
    p.setPen(QPen(QColor(0, 0, 0, 160), 3));
    p.setBrush(Qt::NoBrush);
    p.drawEllipse(at, kHandleRadius, kHandleRadius);
    p.setPen(QPen(Qt::white, 1.5));
    p.setBrush(filled ? QBrush(QColor(0x4c, 0x8d, 0xf6)) : QBrush(QColor(0x1e, 0x1f, 0x22)));
    p.drawEllipse(at, kHandleRadius, kHandleRadius);
}

// A line or shape in white with a dark outline, visible on any photo.
void drawOutlined(QPainter& p, const QPainterPath& path, bool dashed = false)
{
    p.setBrush(Qt::NoBrush);
    p.setPen(QPen(QColor(0, 0, 0, 140), 3));
    p.drawPath(path);
    QPen pen(QColor(255, 255, 255, 230), 1.2);
    if (dashed)
        pen.setDashPattern({5, 4});
    p.setPen(pen);
    p.drawPath(path);
}

} // namespace

// --- Mapping ---------------------------------------------------------------------

QPointF MaskEditor::Mapping::toWidget(float x, float y) const
{
    return origin + QPointF(x * imageSize.width(), y * imageSize.height()) * scale;
}

MaskPoint MaskEditor::Mapping::toMask(const QPointF& widget) const
{
    const QPointF image = (widget - origin) / scale;
    return {float(image.x() / std::max(1, imageSize.width())), float(image.y() / std::max(1, imageSize.height()))};
}

// --- Geometry in image pixels ------------------------------------------------------

namespace {

struct Frame {
    double w, h, longEdge;
    explicit Frame(const QSize& size) : w(size.width()), h(size.height()), longEdge(std::max(w, h)) {}
    QPointF toPixels(float x, float y) const { return {x * w, y * h}; }
};

struct LinearHandles {
    QPointF center, start, end; // start = full effect, end = no effect
    QPointF along;              // unit direction of the lines
};

LinearHandles linearHandles(const LinearGradient& g, const Frame& f)
{
    const QPointF c = f.toPixels(g.x, g.y);
    const QPointF n = normalFor(g.angle);
    const double half = g.feather * f.longEdge / 2;
    return {c, c + n * half, c - n * half, QPointF(-n.y(), n.x())};
}

struct RadialHandles {
    QPointF center;
    QPointF u, v; // unit axes (width, height)
    double rx, ry;
};

RadialHandles radialHandles(const RadialGradient& g, const Frame& f)
{
    const double a = qDegreesToRadians(g.rotation);
    return {f.toPixels(g.x, g.y), QPointF(std::cos(a), -std::sin(a)), QPointF(std::sin(a), std::cos(a)),
            g.width * f.longEdge / 2, g.height * f.longEdge / 2};
}

} // namespace

// --- Editing ---------------------------------------------------------------------

void MaskEditor::setMask(const std::optional<Mask>& mask)
{
    // Keep an in-progress gesture going if the mask is merely updated.
    if (!mask || !m_mask || mask->type != m_mask->type)
        m_drag = Drag::None;
    m_mask = mask;
}

MaskEditor::Drag MaskEditor::hitTest(const QPointF& pos, const Mapping& m) const
{
    if (!m_mask || m_tool != Tool::Shape)
        return Drag::None;
    const Frame f(m.imageSize);
    auto near = [&](const QPointF& image) { return length(m.origin + image * m.scale - pos) <= kHitDistance; };
    if (m_mask->type == MaskType::Linear) {
        const LinearHandles h = linearHandles(m_mask->linear, f);
        if (near(h.center))
            return Drag::Center;
        if (near(h.start))
            return Drag::LinearStart;
        if (near(h.end))
            return Drag::LinearEnd;
    } else if (m_mask->type == MaskType::Radial) {
        const RadialHandles h = radialHandles(m_mask->radial, f);
        if (near(h.center))
            return Drag::Center;
        if (near(h.center - h.v * (h.ry + kRotationArm / m.scale)))
            return Drag::RadialRotation;
        if (near(h.center + h.u * h.rx) || near(h.center - h.u * h.rx))
            return Drag::RadialWidth;
        if (near(h.center + h.v * h.ry) || near(h.center - h.v * h.ry))
            return Drag::RadialHeight;
    }
    return Drag::None;
}

bool MaskEditor::isOverHandle(const QPointF& pos, const Mapping& mapping) const
{
    return hitTest(pos, mapping) != Drag::None;
}

bool MaskEditor::press(const QPointF& pos, const Mapping& m, bool eraseModifier)
{
    if (!m_mask || m.imageSize.isEmpty())
        return false;
    m_pressPos = pos;
    m_pressMask = *m_mask;

    if (m_tool == Tool::Brush) {
        if (m_mask->strokes.size() >= kMaxStrokesPerMask)
            return false;
        BrushStroke stroke;
        stroke.mode = eraseModifier ? BrushMode::Erase : m_brush.mode;
        stroke.radius = m_brush.radius;
        stroke.feather = m_brush.feather;
        stroke.opacity = m_brush.opacity;
        stroke.points = {m.toMask(pos)};
        m_mask->strokes.push_back(std::move(stroke));
        m_drag = Drag::Paint;
        return true;
    }
    if (m_mask->type == MaskType::Brush)
        return false;
    m_drag = hitTest(pos, m);
    if (m_drag == Drag::None)
        m_drag = Drag::Create; // drag out a new gradient
    return true;
}

bool MaskEditor::move(const QPointF& pos, const Mapping& m)
{
    if (!m_mask || m_drag == Drag::None)
        return false;
    if (m_drag == Drag::Paint) {
        BrushStroke& stroke = m_mask->strokes.back();
        const MaskPoint p = m.toMask(pos);
        const MaskPoint& last = stroke.points.back();
        const Frame f(m.imageSize);
        const double distance = std::hypot((p.x - last.x) * f.w, (p.y - last.y) * f.h) / f.longEdge;
        const double minimum = std::max(stroke.radius * kStrokeSpacing, 1.5 / m.longEdge());
        if (distance < minimum || stroke.points.size() >= kMaxPointsPerStroke)
            return false;
        stroke.points.push_back(p);
        return true;
    }
    if (m_drag == Drag::Create && length(pos - m_pressPos) < kMinCreateDrag)
        return false;
    const Mask before = *m_mask;
    dragTo(pos, m);
    return *m_mask != before;
}

void MaskEditor::dragTo(const QPointF& pos, const Mapping& m)
{
    const Frame f(m.imageSize);
    const QPointF p = (pos - m.origin) / m.scale;          // image pixels
    const QPointF p0 = (m_pressPos - m.origin) / m.scale;  // where the drag started
    auto toNormalized = [&](const QPointF& image, float& x, float& y) {
        x = float(image.x() / f.w);
        y = float(image.y() / f.h);
    };

    if (m_mask->type == MaskType::Linear) {
        LinearGradient& g = m_mask->linear;
        const LinearHandles h = linearHandles(m_pressMask.linear, f);
        QPointF start = h.start, end = h.end;
        switch (m_drag) {
        case Drag::Center: toNormalized(h.center + (p - p0), g.x, g.y); return;
        case Drag::LinearStart: start = p; break;
        case Drag::LinearEnd: end = p; break;
        case Drag::Create: start = p0; end = p; break;
        default: return;
        }
        // The gradient runs from start (full effect) to end (no effect).
        const QPointF d = start - end;
        if (length(d) < 1)
            return;
        toNormalized((start + end) / 2, g.x, g.y);
        g.angle = float(angleOf(d / length(d)));
        g.feather = float(length(d) / f.longEdge);
        return;
    }

    if (m_mask->type == MaskType::Radial) {
        RadialGradient& g = m_mask->radial;
        const RadialHandles h = radialHandles(m_pressMask.radial, f);
        const float minimum = 0.005f;
        switch (m_drag) {
        case Drag::Center: toNormalized(h.center + (p - p0), g.x, g.y); break;
        case Drag::RadialWidth: {
            const QPointF d = p - h.center;
            g.width = std::max(minimum, float(2 * std::abs(d.x() * h.u.x() + d.y() * h.u.y()) / f.longEdge));
            break;
        }
        case Drag::RadialHeight: {
            const QPointF d = p - h.center;
            g.height = std::max(minimum, float(2 * std::abs(d.x() * h.v.x() + d.y() * h.v.y()) / f.longEdge));
            break;
        }
        case Drag::RadialRotation: {
            const QPointF d = p - h.center;
            if (length(d) > 1)
                g.rotation = float(angleOf(d / length(d)));
            break;
        }
        case Drag::Create:
            // Drag from the centre outwards.
            toNormalized(p0, g.x, g.y);
            g.width = std::max(minimum, float(2 * std::abs(p.x() - p0.x()) / f.longEdge));
            g.height = std::max(minimum, float(2 * std::abs(p.y() - p0.y()) / f.longEdge));
            g.rotation = 0;
            break;
        default: break;
        }
    }
}

void MaskEditor::release()
{
    m_drag = Drag::None;
}

QString MaskEditor::gestureLabel() const
{
    switch (m_drag) {
    case Drag::Paint: return QObject::tr("Brush Stroke");
    case Drag::Create: return QObject::tr("Draw Gradient");
    case Drag::Center: return QObject::tr("Move Mask");
    default: return QObject::tr("Reshape Mask");
    }
}

// --- Drawing ---------------------------------------------------------------------

void MaskEditor::paint(QPainter& p, const Mapping& m, const std::optional<QPointF>& cursor) const
{
    if (!m_mask || m.imageSize.isEmpty())
        return;
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    const Frame f(m.imageSize);
    auto w = [&](const QPointF& image) { return m.origin + image * m.scale; };

    if (m_tool == Tool::Brush) {
        if (cursor) {
            const double r = m_brush.radius * m.longEdge();
            QPainterPath outer;
            outer.addEllipse(*cursor, r, r);
            drawOutlined(p, outer);
            if (m_brush.feather > 0.02f && r * (1 - m_brush.feather) > 2) {
                QPainterPath inner;
                inner.addEllipse(*cursor, r * (1 - m_brush.feather), r * (1 - m_brush.feather));
                drawOutlined(p, inner, true);
            }
        }
        p.restore();
        return;
    }

    const QRectF imageRect(m.origin, QSizeF(m.imageSize) * m.scale);
    if (m_mask->type == MaskType::Linear) {
        const LinearHandles h = linearHandles(m_mask->linear, f);
        const double reach = (f.w + f.h) * 2; // long enough to cross the photo
        auto line = [&](const QPointF& through) {
            QPainterPath path;
            path.moveTo(w(through - h.along * reach));
            path.lineTo(w(through + h.along * reach));
            return path;
        };
        p.setClipRect(imageRect);
        drawOutlined(p, line(h.start));
        drawOutlined(p, line(h.center), true);
        drawOutlined(p, line(h.end));
        p.setClipping(false);
        drawHandle(p, w(h.center), true);
        drawHandle(p, w(h.start));
        drawHandle(p, w(h.end));
    } else if (m_mask->type == MaskType::Radial) {
        const RadialHandles h = radialHandles(m_mask->radial, f);
        auto ellipse = [&](double scale) {
            QPainterPath path;
            const int steps = 96;
            for (int i = 0; i <= steps; ++i) {
                const double t = 2 * M_PI * i / steps;
                const QPointF pt = w(h.center + h.u * (h.rx * scale * std::cos(t)) + h.v * (h.ry * scale * std::sin(t)));
                i == 0 ? path.moveTo(pt) : path.lineTo(pt);
            }
            return path;
        };
        drawOutlined(p, ellipse(1));
        if (m_mask->radial.feather > 0.02f)
            drawOutlined(p, ellipse(1 - m_mask->radial.feather), true);
        const QPointF top = w(h.center - h.v * h.ry);
        const QPointF rotation = top - QPointF(h.v.x(), h.v.y()) * kRotationArm;
        QPainterPath arm;
        arm.moveTo(top);
        arm.lineTo(rotation);
        drawOutlined(p, arm);
        drawHandle(p, w(h.center), true);
        for (const QPointF& handle : {h.center + h.u * h.rx, h.center - h.u * h.rx, h.center + h.v * h.ry,
                                      h.center - h.v * h.ry})
            drawHandle(p, w(handle));
        drawHandle(p, rotation, true);
    }
    p.restore();
}

} // namespace iris::ui
