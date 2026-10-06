#include "ui/ImageView.h"

#include <QKeyEvent>
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
    m_beforePreview = QImage();
    m_beforeFull = QImage();
    m_maskOverlay = QImage();
    update();
}

void ImageView::setCompareMode(CompareMode mode)
{
    m_compare = mode;
    updateMouseTracking();
    updateCursor();
    update();
}

void ImageView::setBeforePreview(const QImage& preview)
{
    m_beforePreview = preview;
    update();
}

void ImageView::setBeforeFullImage(const QImage& full)
{
    m_beforeFull = full;
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

bool ImageView::needsFullResolution() const
{
    if (m_preview.isNull() || m_imageSize.isEmpty() || m_fit)
        return false;
    return m_zoom > double(m_preview.width()) / m_imageSize.width() * 1.001;
}

void ImageView::setPickMode(bool enabled)
{
    m_pickMode = enabled;
    updateCursor();
}

void ImageView::setMask(const std::optional<iris::Mask>& mask)
{
    const bool wasEditing = m_maskEditor.isActive();
    m_maskEditor.setMask(mask);
    if (!mask) {
        m_maskOverlay = QImage();
        if (wasEditing && m_panning)
            m_panning = false;
    }
    updateMouseTracking();
    updateCursor();
    update();
}

void ImageView::setMaskTool(MaskEditor::Tool tool)
{
    m_maskEditor.setTool(tool);
    updateCursor();
    update();
}

void ImageView::setBrush(const MaskEditor::Brush& brush)
{
    m_maskEditor.setBrush(brush);
    update();
}

void ImageView::setMaskOverlay(const QImage& overlay)
{
    m_maskOverlay = overlay;
    update();
}

void ImageView::updateMouseTracking()
{
    // Tracking shows the split divider's cursor and the brush outline.
    setMouseTracking(m_compare == CompareMode::Split || m_maskEditor.isActive());
}

MaskEditor::Mapping ImageView::maskMapping() const
{
    const double s = logicalScale();
    return {viewCenter() - m_center * s, s, m_imageSize};
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
    if (m_pickMode)
        setCursor(Qt::CrossCursor);
    else if (m_panning)
        setCursor(Qt::ClosedHandCursor);
    else if (m_maskEditor.isActive() && !m_imageSize.isEmpty() && !m_spaceHeld)
        setCursor(m_cursorPos && m_maskEditor.isOverHandle(*m_cursorPos, maskMapping()) ? Qt::SizeAllCursor
                                                                                         : Qt::CrossCursor);
    else if (!m_imageSize.isEmpty() && !m_fit)
        setCursor(Qt::OpenHandCursor);
    else
        unsetCursor();
}

void ImageView::notifyZoom()
{
    emit zoomChanged(m_zoom, m_fit);
}

void ImageView::drawPhoto(QPainter& p, const QImage& preview, const QImage& full, const QRectF& clip)
{
    if (preview.isNull())
        return;
    const double s = logicalScale();
    const QRectF imageRect(viewCenter() - m_center * s, QSizeF(m_imageSize) * s);
    const QRectF visible = imageRect.intersected(clip);
    if (visible.isEmpty())
        return;

    // Use the full-resolution rendering once the preview would be magnified.
    const double previewZoom = double(preview.width()) / m_imageSize.width();
    const bool useFull = !full.isNull() && m_zoom > previewZoom * 1.001;
    const QImage& image = useFull ? full : preview;

    const double sx = image.width() / imageRect.width();
    const double sy = image.height() / imageRect.height();
    const QRectF source((visible.x() - imageRect.x()) * sx, (visible.y() - imageRect.y()) * sy, visible.width() * sx,
                        visible.height() * sy);
    // Smooth when shrinking or mildly enlarging; show crisp pixels when inspecting detail.
    p.setRenderHint(QPainter::SmoothPixmapTransform, m_zoom < 2.0);
    p.drawImage(visible, image, source);
}

void ImageView::drawLabel(QPainter& p, const QString& text, const QPointF& anchor, Qt::Alignment side)
{
    const QRectF textRect = p.fontMetrics().boundingRect(text).adjusted(-10, -4, 10, 4);
    QRectF pill(anchor, textRect.size());
    if (side & Qt::AlignRight)
        pill.moveRight(anchor.x());
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    QPainterPath path;
    path.addRoundedRect(pill, pill.height() / 2, pill.height() / 2);
    p.fillPath(path, QColor(0, 0, 0, 160));
    p.setPen(QColor(0xe6, 0xe7, 0xea));
    p.drawText(pill, Qt::AlignCenter, text);
    p.restore();
}

void ImageView::paintEvent(QPaintEvent*)
{
    QPainter p(this);
    p.fillRect(rect(), kBackground);

    if (!m_preview.isNull() && !m_imageSize.isEmpty()) {
        // While the "before" rendering is not ready yet, show the edited one.
        const bool haveBefore = !m_beforePreview.isNull();
        const QImage& beforePreview = haveBefore ? m_beforePreview : m_preview;
        const QImage& beforeFull = haveBefore ? m_beforeFull : m_full;

        switch (m_compare) {
        case CompareMode::Off:
            drawPhoto(p, m_preview, m_full, rect());
            if (m_maskEditor.isActive() && !m_maskOverlay.isNull())
                drawPhoto(p, m_maskOverlay, QImage(), rect());
            break;
        case CompareMode::Before:
            drawPhoto(p, beforePreview, beforeFull, rect());
            drawLabel(p, tr("Before"), QPointF(12, 12), Qt::AlignLeft);
            break;
        case CompareMode::Split: {
            const double x = splitX();
            drawPhoto(p, beforePreview, beforeFull, QRectF(0, 0, x, height()));
            drawPhoto(p, m_preview, m_full, QRectF(x, 0, width() - x, height()));
            p.setPen(QPen(QColor(255, 255, 255, 200), 1));
            p.drawLine(QPointF(x, 0), QPointF(x, height()));
            drawLabel(p, tr("Before"), QPointF(x - 10, 12), Qt::AlignRight);
            drawLabel(p, tr("After"), QPointF(x + 10, 12), Qt::AlignLeft);
            break;
        }
        }
        const bool brushOutside = m_maskEditor.tool() == MaskEditor::Tool::Brush && m_spaceHeld;
        m_maskEditor.paint(p, maskMapping(), brushOutside ? std::nullopt : m_cursorPos);
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
    if (m_pickMode && event->button() == Qt::LeftButton && !m_imageSize.isEmpty()) {
        const QPointF p = widgetToImage(event->position());
        if (p.x() >= 0 && p.y() >= 0 && p.x() < m_imageSize.width() && p.y() < m_imageSize.height())
            emit pointPicked(QPointF(p.x() / m_imageSize.width(), p.y() / m_imageSize.height()));
        return;
    }
    if (m_compare == CompareMode::Split && event->button() == Qt::LeftButton &&
        std::abs(event->position().x() - splitX()) <= 8) {
        m_draggingSplit = true;
        return;
    }
    // Middle button, or Space + drag, pans; while editing a mask a plain drag edits it.
    const bool panGesture = event->button() == Qt::MiddleButton || (event->button() == Qt::LeftButton && m_spaceHeld);
    if (!panGesture && m_maskEditor.isActive()) {
        if (event->button() == Qt::LeftButton &&
            m_maskEditor.press(event->position(), maskMapping(), event->modifiers() & Qt::AltModifier)) {
            emit maskGestureStarted();
            if (m_maskEditor.tool() == MaskEditor::Tool::Brush) // a click paints a dab
                emit maskEdited(*m_maskEditor.mask(), m_maskEditor.gestureLabel());
            update();
        }
        return;
    }
    if ((event->button() == Qt::LeftButton || panGesture) && !m_fit && !m_imageSize.isEmpty()) {
        m_panning = true;
        m_lastPanPos = event->position();
        updateCursor();
    }
}

void ImageView::mouseMoveEvent(QMouseEvent* event)
{
    if (m_maskEditor.isActive()) {
        m_cursorPos = event->position();
        if (m_maskEditor.isDragging()) {
            if (m_maskEditor.move(event->position(), maskMapping()))
                emit maskEdited(*m_maskEditor.mask(), m_maskEditor.gestureLabel());
            update();
            return;
        }
        if (!m_panning)
            updateCursor();
        update(); // brush outline follows the cursor
    }
    if (m_draggingSplit) {
        m_split = std::clamp(event->position().x() / std::max(1, width()), 0.02, 0.98);
        update();
        return;
    }
    if (m_compare == CompareMode::Split && !m_panning && !m_pickMode && !m_maskEditor.isActive()) {
        if (std::abs(event->position().x() - splitX()) <= 8)
            setCursor(Qt::SplitHCursor);
        else
            updateCursor();
    }
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
    if (m_draggingSplit) {
        m_draggingSplit = false;
        return;
    }
    if (m_maskEditor.isDragging() && event->button() == Qt::LeftButton) {
        m_maskEditor.release();
        emit maskGestureFinished();
        update();
        return;
    }
    if ((event->button() == Qt::LeftButton || event->button() == Qt::MiddleButton) && m_panning) {
        m_panning = false;
        updateCursor();
    }
}

void ImageView::keyPressEvent(QKeyEvent* event)
{
    if (m_pickMode && event->key() == Qt::Key_Escape) {
        emit pickCancelled();
        return;
    }
    if (event->key() == Qt::Key_Space && !event->isAutoRepeat()) {
        m_spaceHeld = true;
        updateCursor();
        update();
        return;
    }
    QWidget::keyPressEvent(event);
}

void ImageView::keyReleaseEvent(QKeyEvent* event)
{
    if (event->key() == Qt::Key_Space && !event->isAutoRepeat()) {
        m_spaceHeld = false;
        updateCursor();
        update();
        return;
    }
    QWidget::keyReleaseEvent(event);
}

void ImageView::leaveEvent(QEvent* event)
{
    m_cursorPos.reset();
    update();
    QWidget::leaveEvent(event);
}

void ImageView::mouseDoubleClickEvent(QMouseEvent* event)
{
    if (m_pickMode || m_maskEditor.isActive() || event->button() != Qt::LeftButton || m_imageSize.isEmpty())
        return;
    if (m_fit)
        setZoom(1.0, event->position());
    else
        fitToWindow();
}

} // namespace iris::ui
