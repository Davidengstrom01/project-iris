#include "ui/HslPanel.h"

#include "ui/AdjustmentSlider.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QTabBar>
#include <QVBoxLayout>

namespace iris::ui {

namespace {

// Approximate display hue (HSV degrees) of each colour range, for the slider grooves.
constexpr int kDisplayHue[kHslColorCount] = {0, 30, 55, 120, 180, 220, 270, 310};

QColor rangeColor(int index, int hueOffset = 0, int saturation = 200, int value = 210)
{
    return QColor::fromHsv((kDisplayHue[index] + hueOffset + 360) % 360, saturation, value);
}

} // namespace

HslPanel::HslPanel(QWidget* parent) : QWidget(parent)
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 8);
    layout->setSpacing(2);

    auto* titleRow = new QHBoxLayout;
    auto* title = new QLabel(tr("COLOR"), this);
    title->setObjectName("panelTitle");
    auto* reset = new QPushButton(tr("Reset"), this);
    reset->setObjectName("smallButton");
    reset->setFocusPolicy(Qt::NoFocus);
    reset->setToolTip(tr("Reset all HSL adjustments"));
    titleRow->addWidget(title);
    titleRow->addStretch();
    titleRow->addWidget(reset);
    titleRow->addSpacing(12);
    layout->addLayout(titleRow);

    m_tabs = new QTabBar(this);
    m_tabs->setObjectName("hslTabs");
    m_tabs->addTab(tr("Hue"));
    m_tabs->addTab(tr("Saturation"));
    m_tabs->addTab(tr("Luminance"));
    m_tabs->setExpanding(true);
    m_tabs->setDrawBase(false);
    m_tabs->setFocusPolicy(Qt::NoFocus);
    auto* tabRow = new QHBoxLayout;
    tabRow->setContentsMargins(12, 0, 12, 4);
    tabRow->addWidget(m_tabs);
    layout->addLayout(tabRow);

    for (int i = 0; i < kHslColorCount; ++i) {
        auto* slider = new AdjustmentSlider(tr(hslColorName(HslColor(i))), -100, 100, 0, this);
        layout->addWidget(slider);
        m_sliders[i] = slider;
        connect(slider, &AdjustmentSlider::valueChanged, this, [this, i](double v) {
            value(m_hsl.bands[i]) = float(v);
            emit hslEdited(m_hsl, QString("%1 %2").arg(tr(hslColorName(HslColor(i))), m_tabs->tabText(m_property)));
        });
    }

    connect(m_tabs, &QTabBar::currentChanged, this, &HslPanel::showProperty);
    connect(reset, &QPushButton::clicked, this, &HslPanel::resetRequested);
    showProperty(Hue);
}

float& HslPanel::value(HslBand& band) const
{
    switch (m_property) {
    case Saturation: return band.saturation;
    case Luminance: return band.luminance;
    default: return band.hue;
    }
}

void HslPanel::showProperty(int property)
{
    m_property = Property(property);
    for (int i = 0; i < kHslColorCount; ++i) {
        // The groove previews the effect: towards the neighbouring hues, from grey to
        // vivid, or from dark to light.
        switch (m_property) {
        case Hue: m_sliders[i]->setGradient(rangeColor(i, -30), rangeColor(i, 30)); break;
        case Saturation: m_sliders[i]->setGradient(rangeColor(i, 0, 0, 150), rangeColor(i, 0, 230)); break;
        case Luminance: m_sliders[i]->setGradient(rangeColor(i, 0, 220, 70), rangeColor(i, 0, 110, 245)); break;
        }
        m_sliders[i]->setValue(value(m_hsl.bands[i]));
    }
}

void HslPanel::setHsl(const HslAdjustments& hsl)
{
    m_hsl = hsl;
    showProperty(m_property);
}

} // namespace iris::ui
