#pragma once

#include <QWidget>

#include <functional>

class QDoubleSpinBox;
class QLabel;
class QSlider;

namespace iris::ui {

// One develop control: a name, an editable value and a slider underneath.
// Double-clicking the name resets the value to its default.
class AdjustmentSlider : public QWidget {
    Q_OBJECT

public:
    AdjustmentSlider(const QString& name, double minimum, double maximum, int decimals, QWidget* parent = nullptr);

    // Non-linear slider response, e.g. for temperature. Maps value <-> position in [0, 1].
    void setResponse(std::function<double(double)> valueToPosition, std::function<double(double)> positionToValue);
    // Coloured groove, e.g. blue -> yellow for temperature.
    void setGradient(const QColor& left, const QColor& right);

    void setDefaultValue(double value) { m_default = value; }
    // Updates the control without emitting valueChanged.
    void setValue(double value);
    double value() const { return m_value; }

signals:
    // Emitted for user changes only.
    void valueChanged(double value);

protected:
    bool eventFilter(QObject* watched, QEvent* event) override;

private:
    int toSlider(double value) const;
    double fromSlider(int position) const;
    void showValue();
    void userChanged(double value);

    QLabel* m_label;
    QDoubleSpinBox* m_spin;
    QSlider* m_slider;
    double m_min;
    double m_max;
    double m_value = 0;
    double m_default = 0;
    int m_decimals;
    std::function<double(double)> m_valueToPosition;
    std::function<double(double)> m_positionToValue;
};

} // namespace iris::ui
