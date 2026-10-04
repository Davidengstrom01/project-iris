#include "ui/HistogramWidget.h"

#include <QPainter>
#include <QPainterPath>

#include <algorithm>
#include <cmath>

namespace iris::ui {

namespace {

// Square-root scaling keeps small populations visible next to large peaks; the scale
// ignores the two end bins so clipped pixels do not flatten everything else.
QPainterPath histogramPath(const std::array<std::uint32_t, 256>& bins, double peak, const QRectF& area)
{
    QPainterPath path(area.bottomLeft());
    for (int i = 0; i < 256; ++i) {
        const double x = area.left() + area.width() * i / 255.0;
        const double h = std::min(1.0, std::sqrt(bins[i] / peak));
        path.lineTo(x, area.bottom() - h * area.height());
    }
    path.lineTo(area.bottomRight());
    path.closeSubpath();
    return path;
}

} // namespace

HistogramWidget::HistogramWidget(QWidget* parent) : QWidget(parent)
{
    setMinimumHeight(80);
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
    setToolTip(tr("Histogram of the edited photo"));
}

void HistogramWidget::setHistogram(const Histogram& histogram)
{
    m_histogram = histogram;
    update();
}

void HistogramWidget::clear()
{
    m_histogram = {};
    update();
}

void HistogramWidget::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    const QRectF area = QRectF(rect()).adjusted(12, 6, -12, -6);
    p.fillRect(area, QColor(0x17, 0x18, 0x1a));
    if (!hasData())
        return;

    double peak = 1;
    for (int i = 1; i < 255; ++i)
        peak = std::max({peak, double(m_histogram.red[i]), double(m_histogram.green[i]), double(m_histogram.blue[i])});

    p.setRenderHint(QPainter::Antialiasing);
    p.setPen(Qt::NoPen);
    // Additive blending: overlapping channels mix towards white, like the photo itself.
    p.setCompositionMode(QPainter::CompositionMode_Plus);
    p.setBrush(QColor(170, 40, 40));
    p.drawPath(histogramPath(m_histogram.red, peak, area));
    p.setBrush(QColor(40, 150, 50));
    p.drawPath(histogramPath(m_histogram.green, peak, area));
    p.setBrush(QColor(40, 70, 190));
    p.drawPath(histogramPath(m_histogram.blue, peak, area));
}

} // namespace iris::ui
