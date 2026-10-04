#pragma once

#include <QImage>
#include <QPointF>
#include <QWidget>

namespace iris::ui {

// Displays the rendered photo with fit / 100% / free zoom and panning.
//
// Holds two renderings of the same photo: a screen-sized preview that is used while
// the whole image is visible, and an optional full-resolution image that is used once
// the zoom level exceeds the preview's resolution. Zoom is expressed in device pixels
// per full-resolution image pixel, so 1.0 is a true 100% view on HiDPI screens too.
class ImageView : public QWidget {
    Q_OBJECT

public:
    explicit ImageView(QWidget* parent = nullptr);

    // Shows a loading indicator; the next setPreview() starts a new photo (resets to fit).
    void beginLoading();
    void setLoadFailed(const QString& message);
    void setPreview(const QImage& preview, const QSize& fullSize);
    void setFullImage(const QImage& full);

    // Before/after comparison. The "before" images are the unedited rendering.
    enum class CompareMode { Off, Before, Split };
    void setCompareMode(CompareMode mode);
    CompareMode compareMode() const { return m_compare; }
    void setBeforePreview(const QImage& preview);
    void setBeforeFullImage(const QImage& full);

    double zoom() const { return m_zoom; }
    bool isFit() const { return m_fit; }
    bool hasImage() const { return !m_preview.isNull(); }
    // True when the current zoom shows more detail than the preview rendering has.
    bool needsFullResolution() const;

    // Eyedropper mode: the next click reports an image position instead of panning.
    void setPickMode(bool enabled);

public slots:
    void fitToWindow();
    void zoomToActualPixels();
    void zoomIn();
    void zoomOut();

signals:
    void zoomChanged(double zoom, bool fit);
    void pointPicked(const QPointF& normalizedPosition);
    void pickCancelled();

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

private:
    double fitZoom() const;
    double logicalScale() const; // widget (logical) pixels per image pixel
    QPointF viewCenter() const;
    QPointF widgetToImage(const QPointF& pos) const;
    void setZoom(double zoom, const QPointF& anchor);
    void clampCenter();
    void updateCursor();
    void notifyZoom();
    void drawPhoto(QPainter& painter, const QImage& preview, const QImage& full, const QRectF& clip);
    void drawLabel(QPainter& painter, const QString& text, const QPointF& anchor, Qt::Alignment side);
    double splitX() const { return width() * m_split; }

    QImage m_preview;
    QImage m_full;
    QSize m_imageSize; // full-resolution size; defines the image coordinate system
    QString m_message;
    bool m_loading = false;

    bool m_fit = true;
    double m_zoom = 1.0;
    QPointF m_center; // image point shown at the centre of the view

    QImage m_beforePreview;
    QImage m_beforeFull;
    CompareMode m_compare = CompareMode::Off;
    double m_split = 0.5; // divider position as a fraction of the view width
    bool m_draggingSplit = false;

    bool m_pickMode = false;
    bool m_panning = false;
    QPointF m_lastPanPos;
};

} // namespace iris::ui
