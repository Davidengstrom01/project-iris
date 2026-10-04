#pragma once

#include "core/Curve.h"

#include <QWidget>

#include <array>
#include <cstdint>

namespace iris::ui {

// Interactive tone curve: input on the x axis, output on the y axis, with the photo's
// luminance histogram behind it.
//   click        add a point (and drag it)
//   drag         move a point
//   double-click / right-click / Delete   remove a point (not the end points)
class CurveEditor : public QWidget {
    Q_OBJECT

public:
    explicit CurveEditor(QWidget* parent = nullptr);

    // Shows a curve without emitting pointsEdited.
    void setPoints(const iris::CurvePoints& points);
    const iris::CurvePoints& points() const { return m_points; }
    void setHistogram(const std::array<std::uint32_t, 256>& luminance);

    bool hasHeightForWidth() const override { return true; }
    int heightForWidth(int width) const override { return width; }
    QSize sizeHint() const override { return {260, 260}; }

signals:
    void pointsEdited(const iris::CurvePoints& points);

protected:
    void paintEvent(QPaintEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;

private:
    QRectF plotRect() const;
    QPointF toWidget(const CurvePoint& p) const;
    CurvePoint fromWidget(const QPointF& pos) const;
    int hitTest(const QPointF& pos) const;
    void movePoint(int index, CurvePoint target);
    void removePoint(int index);

    CurvePoints m_points = linearCurve();
    std::array<float, 256> m_histogram{}; // normalised to 0..1
    bool m_hasHistogram = false;
    int m_dragIndex = -1;
    int m_selected = -1;
};

} // namespace iris::ui
