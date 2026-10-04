#include "ui/CurveEditor.h"

#include <QKeyEvent>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <cmath>

namespace iris::ui {

namespace {

constexpr double kMargin = 12;
constexpr double kHitRadius = 9;

} // namespace

CurveEditor::CurveEditor(QWidget* parent) : QWidget(parent)
{
    setFocusPolicy(Qt::ClickFocus);
    setMouseTracking(true);
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Preferred);
    setMinimumSize(160, 160);
    setToolTip(tr("Click to add a point, drag to move it, double-click to remove it"));
}

void CurveEditor::setPoints(const CurvePoints& points)
{
    m_points = normalizedCurve(points);
    if (m_dragIndex >= int(m_points.size()))
        m_dragIndex = -1;
    if (m_selected >= int(m_points.size()))
        m_selected = -1;
    update();
}

void CurveEditor::setHistogram(const std::array<std::uint32_t, 256>& luminance)
{
    double peak = 1;
    for (int i = 1; i < 255; ++i)
        peak = std::max(peak, double(luminance[i]));
    for (int i = 0; i < 256; ++i)
        m_histogram[i] = float(std::min(1.0, std::sqrt(luminance[i] / peak)));
    m_hasHistogram = true;
    update();
}

QRectF CurveEditor::plotRect() const
{
    const double side = std::min(width(), height()) - 2 * kMargin;
    return QRectF((width() - side) / 2, (height() - side) / 2, side, side);
}

QPointF CurveEditor::toWidget(const CurvePoint& p) const
{
    const QRectF r = plotRect();
    return {r.left() + p.x * r.width(), r.bottom() - p.y * r.height()};
}

CurvePoint CurveEditor::fromWidget(const QPointF& pos) const
{
    const QRectF r = plotRect();
    return {float(std::clamp((pos.x() - r.left()) / r.width(), 0.0, 1.0)),
            float(std::clamp((r.bottom() - pos.y()) / r.height(), 0.0, 1.0))};
}

int CurveEditor::hitTest(const QPointF& pos) const
{
    int best = -1;
    double bestDistance = kHitRadius;
    for (int i = 0; i < int(m_points.size()); ++i) {
        const double d = QLineF(pos, toWidget(m_points[i])).length();
        if (d <= bestDistance) {
            bestDistance = d;
            best = i;
        }
    }
    return best;
}

void CurveEditor::movePoint(int index, CurvePoint target)
{
    const int last = int(m_points.size()) - 1;
    // End points stay at the edges; inner points stay between their neighbours.
    if (index == 0)
        target.x = 0;
    else if (index == last)
        target.x = 1;
    else
        target.x = std::clamp(target.x, m_points[index - 1].x + kMinCurveGap, m_points[index + 1].x - kMinCurveGap);
    target.y = std::clamp(target.y, 0.0f, 1.0f);
    if (m_points[index] == target)
        return;
    m_points[index] = target;
    update();
    emit pointsEdited(m_points);
}

void CurveEditor::removePoint(int index)
{
    if (index <= 0 || index >= int(m_points.size()) - 1)
        return;
    m_points.erase(m_points.begin() + index);
    m_selected = -1;
    m_dragIndex = -1;
    update();
    emit pointsEdited(m_points);
}

void CurveEditor::mousePressEvent(QMouseEvent* event)
{
    if (!isEnabled())
        return;
    const int hit = hitTest(event->position());
    if (event->button() == Qt::RightButton) {
        removePoint(hit);
        return;
    }
    if (event->button() != Qt::LeftButton)
        return;
    if (hit >= 0) {
        m_dragIndex = m_selected = hit;
        update();
        return;
    }
    // Add a point where the user clicked, if there is room between its neighbours.
    const CurvePoint p = fromWidget(event->position());
    if (m_points.size() >= kMaxCurvePoints)
        return;
    const auto it = std::upper_bound(m_points.begin(), m_points.end(), p.x,
                                     [](float x, const CurvePoint& q) { return x < q.x; });
    if (it == m_points.begin() || it == m_points.end() || p.x - (it - 1)->x < kMinCurveGap || it->x - p.x < kMinCurveGap)
        return;
    const int index = int(it - m_points.begin());
    m_points.insert(it, p);
    m_dragIndex = m_selected = index;
    update();
    emit pointsEdited(m_points);
}

void CurveEditor::mouseMoveEvent(QMouseEvent* event)
{
    if (m_dragIndex >= 0) {
        movePoint(m_dragIndex, fromWidget(event->position()));
        return;
    }
    setCursor(hitTest(event->position()) >= 0 ? Qt::PointingHandCursor : Qt::CrossCursor);
}

void CurveEditor::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton)
        m_dragIndex = -1;
}

void CurveEditor::mouseDoubleClickEvent(QMouseEvent* event)
{
    removePoint(hitTest(event->position()));
}

void CurveEditor::keyPressEvent(QKeyEvent* event)
{
    if ((event->key() == Qt::Key_Delete || event->key() == Qt::Key_Backspace) && m_selected >= 0) {
        removePoint(m_selected);
        return;
    }
    QWidget::keyPressEvent(event);
}

void CurveEditor::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.setRenderHint(QPainter::Antialiasing);
    const QRectF r = plotRect();
    p.fillRect(r, QColor(0x17, 0x18, 0x1a));

    // Histogram of the photo behind the curve.
    if (m_hasHistogram) {
        QPainterPath hist(r.bottomLeft());
        for (int i = 0; i < 256; ++i)
            hist.lineTo(r.left() + r.width() * i / 255.0, r.bottom() - m_histogram[i] * r.height() * 0.9);
        hist.lineTo(r.bottomRight());
        hist.closeSubpath();
        p.fillPath(hist, QColor(0x3a, 0x3c, 0x42));
    }

    // Grid in quarters and the identity diagonal.
    p.setPen(QPen(QColor(0x2e, 0x30, 0x35), 1));
    for (int i = 1; i < 4; ++i) {
        const double x = r.left() + r.width() * i / 4, y = r.top() + r.height() * i / 4;
        p.drawLine(QPointF(x, r.top()), QPointF(x, r.bottom()));
        p.drawLine(QPointF(r.left(), y), QPointF(r.right(), y));
    }
    p.setPen(QPen(QColor(0x4a, 0x4c, 0x52), 1, Qt::DashLine));
    p.drawLine(r.bottomLeft(), r.topRight());
    p.setPen(QPen(QColor(0x3a, 0x3c, 0x42), 1));
    p.drawRect(r);

    // The curve.
    const CurveSpline spline(m_points);
    QPainterPath curve;
    constexpr int steps = 160;
    for (int i = 0; i <= steps; ++i) {
        const float x = float(i) / steps;
        const QPointF pt = toWidget({x, spline(x)});
        if (i == 0)
            curve.moveTo(pt);
        else
            curve.lineTo(pt);
    }
    p.setPen(QPen(isEnabled() ? QColor(0xe6, 0xe7, 0xea) : QColor(0x60, 0x62, 0x68), 1.6));
    p.setBrush(Qt::NoBrush);
    p.drawPath(curve);

    // Control points.
    for (int i = 0; i < int(m_points.size()); ++i) {
        const bool selected = i == m_selected;
        p.setPen(QPen(QColor(0xe6, 0xe7, 0xea), 1.4));
        p.setBrush(selected ? QColor(0x4c, 0x8d, 0xf6) : QColor(0x17, 0x18, 0x1a));
        p.drawEllipse(toWidget(m_points[i]), selected ? 5.0 : 4.0, selected ? 5.0 : 4.0);
    }

    // Read-out of the selected point.
    if (m_selected >= 0 && m_selected < int(m_points.size())) {
        const CurvePoint& sp = m_points[m_selected];
        p.setPen(QColor(0x9e, 0xa1, 0xa7));
        p.drawText(r.adjusted(6, 4, -6, -4), Qt::AlignLeft | Qt::AlignTop,
                   tr("In %1  ·  Out %2").arg(std::lround(sp.x * 100)).arg(std::lround(sp.y * 100)));
    }
}

} // namespace iris::ui
