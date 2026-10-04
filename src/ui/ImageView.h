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

    double zoom() const { return m_zoom; }
    bool isFit() const { return m_fit; }
    bool hasImage() const { return !m_preview.isNull(); }

public slots:
    void fitToWindow();
    void zoomToActualPixels();
    void zoomIn();
    void zoomOut();

signals:
    void zoomChanged(double zoom, bool fit);

protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;

private:
    double fitZoom() const;
    double logicalScale() const; // widget (logical) pixels per image pixel
    QPointF viewCenter() const;
    QPointF widgetToImage(const QPointF& pos) const;
    void setZoom(double zoom, const QPointF& anchor);
    void clampCenter();
    void updateCursor();
    void notifyZoom();

    QImage m_preview;
    QImage m_full;
    QSize m_imageSize; // full-resolution size; defines the image coordinate system
    QString m_message;
    bool m_loading = false;

    bool m_fit = true;
    double m_zoom = 1.0;
    QPointF m_center; // image point shown at the centre of the view

    bool m_panning = false;
    QPointF m_lastPanPos;
};

} // namespace iris::ui
