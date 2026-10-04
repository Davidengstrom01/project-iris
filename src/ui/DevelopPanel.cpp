#include "ui/DevelopPanel.h"

#include "core/ColorScience.h"
#include "ui/AdjustmentSlider.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QVBoxLayout>

namespace iris::ui {

namespace {

QLabel* sectionLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setObjectName("sectionLabel");
    return label;
}

QPushButton* smallButton(const QString& text, const QString& tip, QWidget* parent)
{
    auto* button = new QPushButton(text, parent);
    button->setObjectName("smallButton");
    button->setToolTip(tip);
    button->setFocusPolicy(Qt::NoFocus);
    return button;
}

// Temperature slider moves evenly in mired (1/K), which matches perceived change.
double kelvinToPosition(double kelvin)
{
    const double lo = 1e6 / kMaxTemperature, hi = 1e6 / kMinTemperature;
    return (hi - 1e6 / kelvin) / (hi - lo);
}

double positionToKelvin(double position)
{
    const double lo = 1e6 / kMaxTemperature, hi = 1e6 / kMinTemperature;
    return 1e6 / (hi - position * (hi - lo));
}

} // namespace

DevelopPanel::DevelopPanel(QWidget* parent) : QWidget(parent)
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 8);
    layout->setSpacing(2);

    auto* titleRow = new QHBoxLayout;
    auto* title = new QLabel(tr("BASIC"), this);
    title->setObjectName("panelTitle");
    auto* reset = smallButton(tr("Reset"), tr("Reset all basic adjustments"), this);
    titleRow->addWidget(title);
    titleRow->addStretch();
    titleRow->addWidget(reset);
    titleRow->addSpacing(12);
    layout->addLayout(titleRow);

    // White balance
    auto* wbRow = new QHBoxLayout;
    wbRow->setContentsMargins(0, 0, 12, 0);
    wbRow->addWidget(sectionLabel(tr("White Balance"), this));
    wbRow->addStretch();
    auto* asShot = smallButton(tr("As Shot"), tr("Use the camera's white balance"), this);
    auto* autoWb = smallButton(tr("Auto"), tr("Estimate white balance from the photo"), this);
    m_eyedropper = smallButton(tr("Pick"), tr("Click a neutral grey or white area in the photo (Esc to cancel)"), this);
    m_eyedropper->setCheckable(true);
    wbRow->addWidget(asShot);
    wbRow->addWidget(autoWb);
    wbRow->addWidget(m_eyedropper);
    layout->addLayout(wbRow);

    m_temperature = addSlider(tr("Temperature"), kMinTemperature, kMaxTemperature, 0, nullptr);
    m_temperature->setResponse(kelvinToPosition, positionToKelvin);
    m_temperature->setGradient(QColor(0x4a, 0x7c, 0xd6), QColor(0xe0, 0xbe, 0x48));
    connect(m_temperature, &AdjustmentSlider::valueChanged, this, [this](double v) {
        m_adjustments.whiteBalance.temperature = float(v);
        emit adjustmentsChanged(m_adjustments);
    });
    m_tint = addSlider(tr("Tint"), -kMaxTint, kMaxTint, 0, nullptr);
    m_tint->setGradient(QColor(0x4c, 0xb0, 0x52), QColor(0xc8, 0x4c, 0xc0));
    connect(m_tint, &AdjustmentSlider::valueChanged, this, [this](double v) {
        m_adjustments.whiteBalance.tint = float(v);
        emit adjustmentsChanged(m_adjustments);
    });

    layout->addSpacing(6);
    layout->addWidget(sectionLabel(tr("Tone"), this));
    addSlider(tr("Exposure"), -5, 5, 2, &BasicAdjustments::exposure);
    addSlider(tr("Contrast"), -100, 100, 0, &BasicAdjustments::contrast);
    addSlider(tr("Highlights"), -100, 100, 0, &BasicAdjustments::highlights);
    addSlider(tr("Shadows"), -100, 100, 0, &BasicAdjustments::shadows);
    addSlider(tr("Whites"), -100, 100, 0, &BasicAdjustments::whites);
    addSlider(tr("Blacks"), -100, 100, 0, &BasicAdjustments::blacks);

    layout->addSpacing(6);
    layout->addWidget(sectionLabel(tr("Presence"), this));
    addSlider(tr("Vibrance"), -100, 100, 0, &BasicAdjustments::vibrance);
    addSlider(tr("Saturation"), -100, 100, 0, &BasicAdjustments::saturation);

    connect(asShot, &QPushButton::clicked, this, [this] { emit whiteBalanceChosen(m_asShot); });
    connect(autoWb, &QPushButton::clicked, this, &DevelopPanel::autoWhiteBalanceRequested);
    connect(m_eyedropper, &QPushButton::toggled, this, &DevelopPanel::eyedropperToggled);
    connect(reset, &QPushButton::clicked, this, &DevelopPanel::resetRequested);
}

AdjustmentSlider* DevelopPanel::addSlider(const QString& name, double min, double max, int decimals,
                                          float BasicAdjustments::*field)
{
    auto* slider = new AdjustmentSlider(name, min, max, decimals, this);
    layout()->addWidget(slider);
    if (field) {
        m_sliders.append({slider, field});
        connect(slider, &AdjustmentSlider::valueChanged, this, [this, field](double v) {
            m_adjustments.*field = float(v);
            emit adjustmentsChanged(m_adjustments);
        });
    }
    return slider;
}

void DevelopPanel::setAdjustments(const BasicAdjustments& adjustments, const WhiteBalance& asShot)
{
    m_adjustments = adjustments;
    m_asShot = asShot;
    m_temperature->setDefaultValue(asShot.temperature);
    m_tint->setDefaultValue(asShot.tint);
    refresh();
}

void DevelopPanel::setEyedropperActive(bool active)
{
    m_eyedropper->setChecked(active);
}

void DevelopPanel::refresh()
{
    m_temperature->setValue(m_adjustments.whiteBalance.temperature);
    m_tint->setValue(m_adjustments.whiteBalance.tint);
    for (const auto& [slider, field] : m_sliders)
        slider->setValue(m_adjustments.*field);
}

} // namespace iris::ui
