#pragma once

#include "core/EditState.h"

#include <QWidget>

class QPushButton;

namespace iris::ui {

class AdjustmentSlider;

// The "Basic" develop controls: white balance, tone and presence.
class DevelopPanel : public QWidget {
    Q_OBJECT

public:
    explicit DevelopPanel(QWidget* parent = nullptr);

    // Shows a photo's adjustments. Does not emit adjustmentsChanged.
    void setAdjustments(const iris::BasicAdjustments& adjustments, const iris::WhiteBalance& asShot);
    const iris::BasicAdjustments& adjustments() const { return m_adjustments; }
    void setEyedropperActive(bool active);

signals:
    // A slider moved.
    void adjustmentsChanged(const iris::BasicAdjustments& adjustments);
    void whiteBalanceChosen(const iris::WhiteBalance& whiteBalance); // "As Shot"
    void resetRequested();
    void autoWhiteBalanceRequested();
    void eyedropperToggled(bool active);

private:
    AdjustmentSlider* addSlider(const QString& name, double min, double max, int decimals, float BasicAdjustments::*field);
    void refresh();

    BasicAdjustments m_adjustments;
    WhiteBalance m_asShot;
    AdjustmentSlider* m_temperature;
    AdjustmentSlider* m_tint;
    QList<QPair<AdjustmentSlider*, float BasicAdjustments::*>> m_sliders;
    QPushButton* m_eyedropper;
};

} // namespace iris::ui
