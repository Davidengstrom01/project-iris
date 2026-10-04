#include "ui/AdjustmentSlider.h"

#include <QDoubleSpinBox>
#include <QEvent>
#include <QGridLayout>
#include <QLabel>
#include <QSignalBlocker>
#include <QSlider>

#include <algorithm>
#include <cmath>

namespace iris::ui {

namespace {

constexpr int kSliderSteps = 1000;

} // namespace

AdjustmentSlider::AdjustmentSlider(const QString& name, double minimum, double maximum, int decimals, QWidget* parent)
    : QWidget(parent), m_min(minimum), m_max(maximum), m_decimals(decimals)
{
    m_label = new QLabel(name, this);
    m_label->setObjectName("sliderLabel");
    m_label->setToolTip(tr("Double-click to reset"));
    m_label->installEventFilter(this);

    m_spin = new QDoubleSpinBox(this);
    m_spin->setObjectName("sliderValue");
    m_spin->setRange(minimum, maximum);
    m_spin->setDecimals(decimals);
    m_spin->setButtonSymbols(QAbstractSpinBox::NoButtons);
    m_spin->setAlignment(Qt::AlignRight);
    m_spin->setKeyboardTracking(false);
    m_spin->setFrame(false);
    m_spin->setFixedWidth(64);

    m_slider = new QSlider(Qt::Horizontal, this);
    m_slider->setRange(0, kSliderSteps);
    m_slider->setFocusPolicy(Qt::StrongFocus);
    m_slider->installEventFilter(this);

    auto* layout = new QGridLayout(this);
    layout->setContentsMargins(12, 2, 12, 2);
    layout->setHorizontalSpacing(6);
    layout->setVerticalSpacing(0);
    layout->addWidget(m_label, 0, 0);
    layout->addWidget(m_spin, 0, 1);
    layout->addWidget(m_slider, 1, 0, 1, 2);
    layout->setColumnStretch(0, 1);

    connect(m_slider, &QSlider::valueChanged, this, [this](int position) { userChanged(fromSlider(position)); });
    connect(m_spin, &QDoubleSpinBox::valueChanged, this, [this](double value) { userChanged(value); });

    showValue();
}

void AdjustmentSlider::setResponse(std::function<double(double)> valueToPosition,
                                   std::function<double(double)> positionToValue)
{
    m_valueToPosition = std::move(valueToPosition);
    m_positionToValue = std::move(positionToValue);
    showValue();
}

void AdjustmentSlider::setGradient(const QColor& left, const QColor& right)
{
    m_slider->setStyleSheet(QString("QSlider::groove:horizontal { background: qlineargradient(x1:0, y1:0, x2:1, y2:0, "
                                    "stop:0 %1, stop:1 %2); }")
                                .arg(left.name(), right.name()));
}

void AdjustmentSlider::setValue(double value)
{
    m_value = std::clamp(value, m_min, m_max);
    showValue();
}

int AdjustmentSlider::toSlider(double value) const
{
    const double position = m_valueToPosition ? m_valueToPosition(value) : (value - m_min) / (m_max - m_min);
    return int(std::lround(std::clamp(position, 0.0, 1.0) * kSliderSteps));
}

double AdjustmentSlider::fromSlider(int position) const
{
    const double t = double(position) / kSliderSteps;
    const double value = m_positionToValue ? m_positionToValue(t) : m_min + t * (m_max - m_min);
    const double scale = std::pow(10.0, m_decimals);
    return std::round(value * scale) / scale;
}

void AdjustmentSlider::showValue()
{
    const QSignalBlocker blockSlider(m_slider);
    const QSignalBlocker blockSpin(m_spin);
    m_slider->setValue(toSlider(m_value));
    m_spin->setValue(m_value);
}

void AdjustmentSlider::userChanged(double value)
{
    value = std::clamp(value, m_min, m_max);
    if (value == m_value)
        return;
    m_value = value;
    showValue();
    emit valueChanged(m_value);
}

bool AdjustmentSlider::eventFilter(QObject* watched, QEvent* event)
{
    if (watched == m_label && event->type() == QEvent::MouseButtonDblClick) {
        userChanged(m_default);
        return true;
    }
    // Let the wheel scroll the panel unless the slider has been clicked/focused.
    if (watched == m_slider && event->type() == QEvent::Wheel && !m_slider->hasFocus()) {
        event->ignore();
        return true;
    }
    return QWidget::eventFilter(watched, event);
}

} // namespace iris::ui
