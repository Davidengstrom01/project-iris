#pragma once

#include "core/Curve.h"

#include <QWidget>

#include <array>
#include <cstdint>

class QComboBox;

namespace iris::ui {

class CurveEditor;

// "Tone Curve" section: the curve editor, curve presets and reset.
class ToneCurvePanel : public QWidget {
    Q_OBJECT

public:
    explicit ToneCurvePanel(QWidget* parent = nullptr);

    // Shows a photo's curve. Does not emit signals.
    void setCurve(const iris::ToneCurve& curve);
    const iris::ToneCurve& curve() const { return m_curve; }
    void setHistogram(const std::array<std::uint32_t, 256>& luminance);

signals:
    // The user is dragging, adding or removing points.
    void curveEdited(const iris::ToneCurve& curve);
    // The user picked a preset curve or reset it.
    void curveChosen(const iris::ToneCurve& curve);

private:
    void updatePresetSelection();

    ToneCurve m_curve;
    CurveEditor* m_editor;
    QComboBox* m_presets;
};

} // namespace iris::ui
