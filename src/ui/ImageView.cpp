#include "ui/ImageView.h"

#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QWheelEvent>

#include <algorithm>
#include <array>
#include <cmath>

namespace iris::ui {

namespace {

constexpr double kMaxZoom = 16.0;
constexpr double kFitMargin = 12.0; // logical pixels around the image in fit mode
constexpr std::array kZoomSteps = {0.0625, 0.125, 0.25, 1.0 / 3.0, 0.5, 2.0 / 3.0, 1.0, 2.0, 3.0, 4.0, 8.0, 16.0};
const QColor kBackground(0x14, 0x15, 0x17);

} // namespace

ImageView::ImageView(QWidget* parent) : QWidget(parent)
{
    setMinimumSize(200, 150);
    setMouseTracking(false);
    setFocusPolicy(Qt::StrongFocus);
    m_message = tr("Open a RAW photo to start  (Ctrl+O)");
}

void ImageView::beginLoading()
{
    m_loading = true;
    m_message.clear();
    update();
}

void ImageView::setLoadFailed(const QString& message)
{
    m_loading = false;
    m_preview = QImage();
    m_full = QImage();
    m_imageSize = QSize();
    m_message = message;
    updateCursor();
    update();
    notifyZoom();
}

void ImageView::setPreview(const QImage& preview, const QSize& fullSize)
{
    const bool newPhoto = m_loading || m_imageSize.isEmpty();
    m_preview = preview;
    if (newPhoto) {
        m_loading = false;
        m_full = QImage();
        m_imageSize = fullSize;
        fitToWindow();
    } else {
        update();
    }
}

void ImageView::setFullImage(const QImage& full)
{
    m_full = full;
    if (!full.isNull() && full.size() != m_imageSize) {
        // Keep the same relative position if the decoder's final size differs slightly.
        const double sx = double(full.width()) / std::max(1, m_imageSize.width());
        const double sy = double(full.height()) / std::max(1, m_imageSize.height());
        m_center = QPointF(m_center.x() * sx, m_center.y() * sy);
        m_imageSize = full.size();
        if (m_fit)
            fitToWindow();
    }
    update();
}

double ImageView::fitZoom() const
{
    if (m_imageSize.isEmpty())
        return 1.0;
    const double dpr = devicePixelRatioF();
    const double w = std::max(1.0, width() - 2 * kFitMargin) * dpr;
    const double h = std::max(1.0, height() - 2 * kFitMargin) * dpr;
    return std::min(w / m_imageSize.width(), h / m_imageSize.height());
}

double ImageView::logicalScale() const
{
    return m_zoom / devicePixelRatioF();
}

QPointF ImageView::viewCenter() const
{
    return QPointF(width() / 2.0, height() / 2.0);
}

QPointF ImageView::widgetToImage(const QPointF& pos) const
{
    return m_center + (pos - viewCenter()) / logicalScale();
}

void ImageView::fitToWindow()
{
    m_fit = true;
    m_zoom = fitZoom();
    m_center = QPointF(m_imageSize.width() / 2.0, m_imageSize.height() / 2.0);
    updateCursor();
    update();
    notifyZoom();
}

void ImageView::zoomToActualPixels()
{
    setZoom(1.0, viewCenter());
}

void ImageView::zoomIn()
{
    for (double step : kZoomSteps) {
        if (step > m_zoom * 1.01) {
            setZoom(step, viewCenter());
            return;
        }
    }
}

void ImageView::zoomOut()
{
    for (auto it = kZoomSteps.rbegin(); it != kZoomSteps.rend(); ++it) {
        if (*it < m_zoom / 1.01) {
            setZoom(*it, viewCenter());
            return;
        }
    }
    fitToWindow();
}

void ImageView::setZoom(double zoom, const QPointF& anchor)
{
    if (m_imageSize.isEmpty())
        return;
    // Zooming out never goes below "fit": the whole photo is always reachable.
    if (zoom <= fitZoom() * 1.0001) {
        fitToWindow();
        return;
    }
    const QPointF anchorImage = widgetToImage(anchor);
    m_zoom = std::min(zoom, kMaxZoom);
    m_fit = false;
    m_center = anchorImage - (anchor - viewCenter()) / logicalScale();
    clampCenter();
    updateCursor();
    update();
    notifyZoom();
}

void ImageView::clampCenter()
{
    const double s = logicalScale();
    auto clampAxis = [](double center, double imageLength, double viewLength) {
        if (imageLength <= viewLength)
            return imageLength / 2.0;
        return std::clamp(center, viewLength / 2.0, imageLength - viewLength / 2.0);
    };
    m_center.setX(clampAxis(m_center.x(), m_imageSize.width(), width() / s));
    m_center.setY(clampAxis(m_center.y(), m_imageSize.height(), height() / s));
}

void ImageView::updateCursor()
{
    if (m_panning)
        setCursor(Qt::ClosedHandCursor);
    else if (!m_imageSize.isEmpty() && !m_fit)
        setCursor(Qt::OpenHandCursor);
    else
        unsetCursor();
}

void ImageView::notifyZoom()
{
    emit zoomChanged(m_zoom, m_fit);
}

void ImageView::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.fillRect(rect(), kBackground);

    if (!m_preview.isNull() && !m_imageSize.isEmpty()) {
        const double s = logicalScale();
        const QRectF imageRect(viewCenter() - m_center * s, QSizeF(m_imageSize) * s);
        const QRectF visible = imageRect.intersected(QRectF(rect()));

        // Use the full-resolution rendering once the preview would be magnified.
        const double previewZoom = double(m_preview.width()) / m_imageSize.width();
        const bool useFull = !m_full.isNull() && m_zoom > previewZoom * 1.001;
        const QImage& image = useFull ? m_full : m_preview;

        if (!visible.isEmpty()) {
            const double sx = image.width() / imageRect.width();
            const double sy = image.height() / imageRect.height();
            const QRectF source((visible.x() - imageRect.x()) * sx, (visible.y() - imageRect.y()) * sy,
                                visible.width() * sx, visible.height() * sy);
            // Smooth when shrinking or mildly enlarging; show crisp pixels when inspecting detail.
            p.setRenderHint(QPainter::SmoothPixmapTransform, m_zoom < 2.0);
            p.drawImage(visible, image, source);
        }
    } else if (!m_message.isEmpty() && !m_loading) {
        p.setPen(QColor(0x7d, 0x80, 0x86));
        p.drawText(rect(), Qt::AlignCenter | Qt::TextWordWrap, m_message);
    }

    if (m_loading) {
        const QString text = tr("Loading…");
        QFont font = p.font();
        font.setPointSizeF(font.pointSizeF() * 1.05);
        p.setFont(font);
        const QRectF textRect = p.fontMetrics().boundingRect(text).adjusted(-14, -7, 14, 7);
        const QRectF pill(QPointF(width() / 2.0 - textRect.width() / 2, height() - textRect.height() - 24), textRect.size());
        p.setRenderHint(QPainter::Antialiasing);
        QPainterPath path;
        path.addRoundedRect(pill, pill.height() / 2, pill.height() / 2);
        p.fillPath(path, QColor(0, 0, 0, 170));
        p.setPen(QColor(0xe6, 0xe7, 0xea));
        p.drawText(pill, Qt::AlignCenter, text);
    }
}

void ImageView::resizeEvent(QResizeEvent*)
{
    if (m_fit) {
        fitToWindow();
    } else {
        clampCenter();
        update();
    }
}

void ImageView::wheelEvent(QWheelEvent* event)
{
    const double steps = event->angleDelta().y() / 120.0;
    if (steps == 0 || m_imageSize.isEmpty())
        return;
    setZoom(m_zoom * std::pow(1.25, steps), event->position());
    event->accept();
}

void ImageView::mousePressEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton && !m_fit && !m_imageSize.isEmpty()) {
        m_panning = true;
        m_lastPanPos = event->position();
        updateCursor();
    }
}

void ImageView::mouseMoveEvent(QMouseEvent* event)
{
    if (!m_panning)
        return;
    const QPointF delta = event->position() - m_lastPanPos;
    m_lastPanPos = event->position();
    m_center -= delta / logicalScale();
    clampCenter();
    update();
}

void ImageView::mouseReleaseEvent(QMouseEvent* event)
{
    if (event->button() == Qt::LeftButton && m_panning) {
        m_panning = false;
        updateCursor();
    }
}

void ImageView::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (event->button() != Qt::LeftButton || m_imageSize.isEmpty())
        return;
    if (m_fit)
        setZoom(1.0, event->position());
    else
        fitToWindow();
}

} // namespace iris::ui
