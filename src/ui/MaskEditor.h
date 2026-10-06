#pragma once

#include "core/Mask.h"

#include <QPointF>
#include <QSize>
#include <QString>

#include <algorithm>
#include <optional>

class QPainter;

namespace iris::ui {

// Editing a mask directly on the photo: painting brush strokes, and dragging the handles of
// linear and radial gradients (or dragging out a new one). Used by ImageView, which passes
// in where the photo is on screen.
class MaskEditor {
public:
    enum class Tool {
        Shape, // move / reshape the gradient
        Brush, // paint strokes with the current brush
    };

    struct Brush {
        BrushMode mode = BrushMode::Add;
        float radius = 0.05f; // fraction of the long edge
        float feather = 0.5f;
        float opacity = 1.0f;
    };

    // Where the photo is drawn: widget position = origin + image pixel * scale.
    struct Mapping {
        QPointF origin;
        double scale = 1;
        QSize imageSize;

        QPointF toWidget(float x, float y) const;
        MaskPoint toMask(const QPointF& widget) const;
        double longEdge() const { return std::max(imageSize.width(), imageSize.height()) * scale; }
    };

    void setMask(const std::optional<Mask>& mask);
    const std::optional<Mask>& mask() const { return m_mask; }
    bool isActive() const { return m_mask.has_value(); }
    void setTool(Tool tool) { m_tool = tool; }
    Tool tool() const { return m_tool; }
    void setBrush(const Brush& brush) { m_brush = brush; }
    const Brush& brush() const { return m_brush; }

    // Mouse gestures. press() returns true if it starts one; move() returns true if it
    // changed the mask. eraseModifier temporarily switches the brush to Erase.
    bool press(const QPointF& pos, const Mapping& mapping, bool eraseModifier);
    bool move(const QPointF& pos, const Mapping& mapping);
    void release();
    bool isDragging() const { return m_drag != Drag::None; }
    // Undo label of the current gesture, e.g. "Brush Stroke".
    QString gestureLabel() const;

    bool usesHoverCursor() const { return m_tool == Tool::Brush; }
    bool isOverHandle(const QPointF& pos, const Mapping& mapping) const;
    // Draws the gradient guides and handles, or the brush outline at the cursor.
    void paint(QPainter& painter, const Mapping& mapping, const std::optional<QPointF>& cursor) const;

private:
    enum class Drag { None, Paint, Create, Center, LinearStart, LinearEnd, RadialWidth, RadialHeight, RadialRotation };

    Drag hitTest(const QPointF& pos, const Mapping& mapping) const;
    void dragTo(const QPointF& pos, const Mapping& mapping);

    std::optional<Mask> m_mask;
    Tool m_tool = Tool::Brush;
    Brush m_brush;
    Drag m_drag = Drag::None;
    QPointF m_pressPos;  // widget
    Mask m_pressMask;    // the mask when the gesture started
};

} // namespace iris::ui
