#pragma once

#include "core/Hsl.h"

#include <QWidget>

#include <array>

class QTabBar;

namespace iris::ui {

class AdjustmentSlider;

// "Color" section: Hue / Saturation / Luminance tabs, each with a slider per colour range.
class HslPanel : public QWidget {
    Q_OBJECT

public:
    explicit HslPanel(QWidget* parent = nullptr);

    // Shows a photo's HSL adjustments. Does not emit signals.
    void setHsl(const iris::HslAdjustments& hsl);
    const iris::HslAdjustments& hsl() const { return m_hsl; }

signals:
    // A slider moved; label names it, e.g. "Blue Saturation".
    void hslEdited(const iris::HslAdjustments& hsl, const QString& label);
    void resetRequested();

private:
    enum Property { Hue, Saturation, Luminance };
    float& value(HslBand& band) const;
    void showProperty(int property);

    HslAdjustments m_hsl;
    Property m_property = Hue;
    QTabBar* m_tabs;
    std::array<AdjustmentSlider*, kHslColorCount> m_sliders{};
};

} // namespace iris::ui
