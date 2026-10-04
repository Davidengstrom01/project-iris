#include "ui/ToneCurvePanel.h"

#include "ui/CurveEditor.h"

#include <QComboBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QSignalBlocker>
#include <QVBoxLayout>

namespace iris::ui {

namespace {

enum PresetIndex { Custom, Linear, SCurve, InverseS };

CurvePoints presetPoints(int index)
{
    switch (index) {
    case SCurve: return sCurve();
    case InverseS: return inverseSCurve();
    default: return linearCurve();
    }
}

} // namespace

ToneCurvePanel::ToneCurvePanel(QWidget* parent) : QWidget(parent)
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 8);
    layout->setSpacing(4);

    auto* titleRow = new QHBoxLayout;
    auto* title = new QLabel(tr("TONE CURVE"), this);
    title->setObjectName("panelTitle");
    auto* reset = new QPushButton(tr("Reset"), this);
    reset->setObjectName("smallButton");
    reset->setFocusPolicy(Qt::NoFocus);
    reset->setToolTip(tr("Reset to a straight line"));
    titleRow->addWidget(title);
    titleRow->addStretch();
    titleRow->addWidget(reset);
    titleRow->addSpacing(12);
    layout->addLayout(titleRow);

    auto* presetRow = new QHBoxLayout;
    presetRow->setContentsMargins(12, 0, 12, 0);
    auto* presetLabel = new QLabel(tr("Curve"), this);
    presetLabel->setObjectName("sliderLabel");
    m_presets = new QComboBox(this);
    m_presets->addItems({tr("Custom"), tr("Linear"), tr("S-Curve (more contrast)"), tr("Inverse S (less contrast)")});
    m_presets->setFocusPolicy(Qt::NoFocus);
    presetRow->addWidget(presetLabel);
    presetRow->addWidget(m_presets, 1);
    layout->addLayout(presetRow);

    m_editor = new CurveEditor(this);
    layout->addWidget(m_editor);

    connect(m_editor, &CurveEditor::pointsEdited, this, [this](const CurvePoints& points) {
        m_curve.rgb = points;
        updatePresetSelection();
        emit curveEdited(m_curve);
    });
    connect(m_presets, &QComboBox::activated, this, [this](int index) {
        if (index == Custom)
            return;
        m_curve.rgb = presetPoints(index);
        m_editor->setPoints(m_curve.rgb);
        emit curveChosen(m_curve);
    });
    connect(reset, &QPushButton::clicked, this, [this] {
        m_curve = ToneCurve{};
        m_editor->setPoints(m_curve.rgb);
        updatePresetSelection();
        emit curveChosen(m_curve);
    });
    updatePresetSelection();
}

void ToneCurvePanel::setCurve(const ToneCurve& curve)
{
    m_curve = curve;
    m_editor->setPoints(curve.rgb);
    updatePresetSelection();
}

void ToneCurvePanel::setHistogram(const std::array<std::uint32_t, 256>& luminance)
{
    m_editor->setHistogram(luminance);
}

void ToneCurvePanel::updatePresetSelection()
{
    int index = Custom;
    for (int i : {Linear, SCurve, InverseS})
        if (m_curve.rgb == presetPoints(i))
            index = i;
    const QSignalBlocker blocker(m_presets);
    m_presets->setCurrentIndex(index);
}

} // namespace iris::ui
