#include "ui/MaskPanel.h"

#include "ui/AdjustmentSlider.h"

#include <QButtonGroup>
#include <QCheckBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QListWidget>
#include <QPushButton>
#include <QSignalBlocker>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>

namespace iris::ui {

namespace {

constexpr float kRadiusPerSize = 0.002f; // brush Size 1..100 -> radius 0.2%..20% of the long edge
enum ToolButton { ShapeButton, AddButton, SubtractButton, EraseButton };

QPushButton* smallButton(const QString& text, const QString& tip, QWidget* parent)
{
    auto* button = new QPushButton(text, parent);
    button->setObjectName("smallButton");
    button->setToolTip(tip);
    button->setFocusPolicy(Qt::NoFocus);
    return button;
}

QLabel* sectionLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setObjectName("sectionLabel");
    return label;
}

QWidget* section(QWidget* parent, QVBoxLayout* into)
{
    auto* widget = new QWidget(parent);
    auto* layout = new QVBoxLayout(widget);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(2);
    into->addWidget(widget);
    return widget;
}

} // namespace

MaskPanel::MaskPanel(QWidget* parent) : QWidget(parent)
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 8);
    layout->setSpacing(2);

    auto* titleRow = new QHBoxLayout;
    auto* title = new QLabel(tr("MASKS"), this);
    title->setObjectName("panelTitle");
    titleRow->addWidget(title);
    titleRow->addStretch();
    for (const MaskType type : {MaskType::Brush, MaskType::Linear, MaskType::Radial}) {
        const QString name = type == MaskType::Brush ? tr("Brush") : type == MaskType::Linear ? tr("Linear") : tr("Radial");
        auto* add = smallButton("+ " + name, tr("Add a %1 mask").arg(tr(maskTypeName(type)).toLower()), this);
        connect(add, &QPushButton::clicked, this, [this, type] { emit addRequested(type); });
        titleRow->addWidget(add);
    }
    titleRow->addSpacing(12);
    layout->addLayout(titleRow);

    m_list = new QListWidget(this);
    m_list->setObjectName("maskList");
    m_list->setFocusPolicy(Qt::NoFocus);
    m_list->setFixedHeight(96);
    m_hint = new QLabel(tr("Add a mask to adjust part of the photo (M)."), this);
    m_hint->setObjectName("hintLabel");
    m_hint->setWordWrap(true);
    auto* listRow = new QVBoxLayout;
    listRow->setContentsMargins(12, 0, 12, 0);
    listRow->addWidget(m_list);
    listRow->addWidget(m_hint);
    layout->addLayout(listRow);

    auto* optionsRow = new QHBoxLayout;
    optionsRow->setContentsMargins(12, 2, 12, 0);
    m_overlay = new QCheckBox(tr("Show overlay"), this);
    m_overlay->setToolTip(tr("Tint the selected mask's area (O)"));
    m_overlay->setChecked(true);
    m_overlay->setFocusPolicy(Qt::NoFocus);
    m_delete = smallButton(tr("Delete"), tr("Delete the selected mask"), this);
    optionsRow->addWidget(m_overlay);
    optionsRow->addStretch();
    optionsRow->addWidget(m_delete);
    layout->addLayout(optionsRow);

    // Everything below applies to the selected mask.
    m_details = new QWidget(this);
    auto* details = new QVBoxLayout(m_details);
    details->setContentsMargins(0, 4, 0, 0);
    details->setSpacing(2);
    layout->addWidget(m_details);

    auto* toolRow = new QHBoxLayout;
    toolRow->setContentsMargins(0, 0, 12, 0);
    toolRow->addWidget(sectionLabel(tr("Edit"), m_details));
    toolRow->addStretch();
    m_tools = new QButtonGroup(this);
    m_shapeTool = smallButton(tr("Shape"), tr("Drag the handles, or drag on the photo to draw the gradient again"),
                              m_details);
    const struct {
        ToolButton id;
        QPushButton* button;
    } tools[] = {
        {ShapeButton, m_shapeTool},
        {AddButton, smallButton(tr("Add"), tr("Paint the mask in (Alt+drag erases)"), m_details)},
        {SubtractButton, smallButton(tr("Subtract"), tr("Paint to remove the effect, gradient included"), m_details)},
        {EraseButton, smallButton(tr("Erase"), tr("Paint to remove earlier brush strokes"), m_details)},
    };
    for (const auto& [id, button] : tools) {
        button->setCheckable(true);
        m_tools->addButton(button, id);
        toolRow->addWidget(button);
    }
    details->addLayout(toolRow);

    m_brushSection = section(m_details, details);
    m_brushSize = addSlider(m_brushSection, tr("Brush Size"), 1, 100, 0, 25);
    m_brushSize->setToolTip(tr("[ and ] change the size"));
    m_brushFeather = addSlider(m_brushSection, tr("Brush Feather"), 0, 100, 0, 50);
    m_brushOpacity = addSlider(m_brushSection, tr("Brush Opacity"), 1, 100, 0, 100);
    m_brushSize->setValue(m_brush.radius / kRadiusPerSize);
    m_brushFeather->setValue(m_brush.feather * 100);
    m_brushOpacity->setValue(m_brush.opacity * 100);
    connect(m_brushSize, &AdjustmentSlider::valueChanged, this, [this](double v) {
        m_brush.radius = float(v) * kRadiusPerSize;
        emitTool();
    });
    connect(m_brushFeather, &AdjustmentSlider::valueChanged, this, [this](double v) {
        m_brush.feather = float(v / 100);
        emitTool();
    });
    connect(m_brushOpacity, &AdjustmentSlider::valueChanged, this, [this](double v) {
        m_brush.opacity = float(v / 100);
        emitTool();
    });

    // Shape sliders: lengths are percentages of the photo's long edge.
    m_linearSection = section(m_details, details);
    m_linearAngle = addSlider(m_linearSection, tr("Angle"), -180, 180, 0, 0);
    m_linearFeather = addSlider(m_linearSection, tr("Feather"), 0, 100, 0, 30);
    connect(m_linearAngle, &AdjustmentSlider::valueChanged, this, [this](double v) {
        editShape(tr("Angle"), [](Mask& m, float x) { m.linear.angle = x; }, v);
    });
    connect(m_linearFeather, &AdjustmentSlider::valueChanged, this, [this](double v) {
        editShape(tr("Feather"), [](Mask& m, float x) { m.linear.feather = x / 100; }, v);
    });

    m_radialSection = section(m_details, details);
    m_radialWidth = addSlider(m_radialSection, tr("Width"), 1, 200, 0, 40);
    m_radialHeight = addSlider(m_radialSection, tr("Height"), 1, 200, 0, 30);
    m_radialRotation = addSlider(m_radialSection, tr("Rotation"), -180, 180, 0, 0);
    m_radialFeather = addSlider(m_radialSection, tr("Feather"), 0, 100, 0, 50);
    connect(m_radialWidth, &AdjustmentSlider::valueChanged, this, [this](double v) {
        editShape(tr("Width"), [](Mask& m, float x) { m.radial.width = x / 100; }, v);
    });
    connect(m_radialHeight, &AdjustmentSlider::valueChanged, this, [this](double v) {
        editShape(tr("Height"), [](Mask& m, float x) { m.radial.height = x / 100; }, v);
    });
    connect(m_radialRotation, &AdjustmentSlider::valueChanged, this, [this](double v) {
        editShape(tr("Rotation"), [](Mask& m, float x) { m.radial.rotation = x; }, v);
    });
    connect(m_radialFeather, &AdjustmentSlider::valueChanged, this, [this](double v) {
        editShape(tr("Feather"), [](Mask& m, float x) { m.radial.feather = x / 100; }, v);
    });

    auto* invertRow = new QHBoxLayout;
    invertRow->setContentsMargins(12, 4, 12, 2);
    m_invert = new QCheckBox(tr("Invert"), m_details);
    m_invert->setToolTip(tr("Apply the adjustments outside the mask instead"));
    m_invert->setFocusPolicy(Qt::NoFocus);
    invertRow->addWidget(m_invert);
    details->addLayout(invertRow);
    connect(m_invert, &QCheckBox::toggled, this, [this](bool on) {
        if (m_selected < 0)
            return;
        m_masks[m_selected].invert = on;
        emit maskEdited(m_masks[m_selected], tr("%1 Invert").arg(QString::fromStdString(m_masks[m_selected].name)));
    });

    QWidget* adjustments = section(m_details, details);
    adjustments->layout()->addWidget(sectionLabel(tr("Adjustments"), adjustments));
    for (const LocalAdjustmentField& field : localAdjustmentFields()) {
        const int decimals = std::string(field.key) == "exposure" ? 2 : 0;
        AdjustmentSlider* slider = addSlider(adjustments, tr(field.label), field.minimum, field.maximum, decimals, 0);
        if (std::string(field.key) == "temperature")
            slider->setGradient(QColor(0x4a, 0x7c, 0xd6), QColor(0xe0, 0xbe, 0x48));
        m_adjustments.push_back(slider);
        connect(slider, &AdjustmentSlider::valueChanged, this, [this, &field](double v) {
            if (m_selected < 0)
                return;
            Mask& mask = m_masks[m_selected];
            field.value(mask.adjustments) = float(v);
            emit maskEdited(mask, QString("%1 %2").arg(QString::fromStdString(mask.name), tr(field.label)));
        });
    }

    connect(m_list, &QListWidget::currentRowChanged, this, [this](int row) {
        if (row != m_selected)
            emit selectionRequested(row);
    });
    connect(m_delete, &QPushButton::clicked, this, [this] {
        if (m_selected >= 0)
            emit deleteRequested(m_selected);
    });
    connect(m_overlay, &QCheckBox::toggled, this, &MaskPanel::overlayToggled);
    connect(m_tools, &QButtonGroup::idClicked, this, [this](int id) {
        if (id == ShapeButton) {
            setTool(MaskEditor::Tool::Shape);
        } else {
            m_brush.mode = id == SubtractButton ? BrushMode::Subtract : id == EraseButton ? BrushMode::Erase : BrushMode::Add;
            setTool(MaskEditor::Tool::Brush);
        }
        emitTool();
    });

    m_tools->button(AddButton)->setChecked(true);
    showSelected();
}

AdjustmentSlider* MaskPanel::addSlider(QWidget* parent, const QString& name, double min, double max, int decimals,
                                       double defaultValue)
{
    auto* slider = new AdjustmentSlider(name, min, max, decimals, parent);
    slider->setDefaultValue(defaultValue);
    parent->layout()->addWidget(slider);
    return slider;
}

void MaskPanel::editShape(const QString& property, void (*apply)(Mask&, float), double value)
{
    if (m_selected < 0)
        return;
    Mask& mask = m_masks[m_selected];
    apply(mask, float(value));
    mask = sanitized(mask);
    emit maskEdited(mask, QString("%1 %2").arg(QString::fromStdString(mask.name), property));
}

void MaskPanel::setMasks(const std::vector<Mask>& masks, int selected)
{
    const bool valid = selected >= 0 && selected < int(masks.size()) && selected < int(m_masks.size());
    const bool newSelection = selected != m_selected || !valid || masks[selected].type != m_masks[selected].type ||
                              masks[selected].name != m_masks[selected].name;
    m_masks = masks;
    m_selected = selected >= 0 && selected < int(masks.size()) ? selected : -1;
    {
        const QSignalBlocker block(m_list);
        if (m_list->count() != int(masks.size())) {
            m_list->clear();
            for (std::size_t i = 0; i < masks.size(); ++i)
                m_list->addItem(QString());
        }
        for (std::size_t i = 0; i < masks.size(); ++i) {
            QListWidgetItem* item = m_list->item(int(i));
            item->setText(QString::fromStdString(masks[i].name));
            item->setToolTip(tr(maskTypeName(masks[i].type)));
        }
        m_list->setCurrentRow(m_selected);
    }
    // A newly selected gradient starts with its handles; a brush mask always paints.
    if (newSelection && m_selected >= 0)
        setTool(m_masks[m_selected].type == MaskType::Brush ? MaskEditor::Tool::Brush : MaskEditor::Tool::Shape);
    showSelected();
}

void MaskPanel::showSelected()
{
    const bool any = !m_masks.empty();
    m_list->setVisible(any);
    m_hint->setVisible(!any);
    m_delete->setEnabled(m_selected >= 0);
    m_details->setVisible(m_selected >= 0);
    if (m_selected < 0)
        return;

    const Mask& mask = m_masks[m_selected];
    const bool gradient = mask.type != MaskType::Brush;
    m_shapeTool->setVisible(gradient);
    m_brushSection->setVisible(m_tool == MaskEditor::Tool::Brush);
    m_linearSection->setVisible(mask.type == MaskType::Linear);
    m_radialSection->setVisible(mask.type == MaskType::Radial);

    m_linearAngle->setValue(mask.linear.angle);
    m_linearFeather->setValue(mask.linear.feather * 100);
    m_radialWidth->setValue(mask.radial.width * 100);
    m_radialHeight->setValue(mask.radial.height * 100);
    m_radialRotation->setValue(mask.radial.rotation);
    m_radialFeather->setValue(mask.radial.feather * 100);
    {
        const QSignalBlocker block(m_invert);
        m_invert->setChecked(mask.invert);
    }
    LocalAdjustments values = mask.adjustments;
    for (std::size_t i = 0; i < m_adjustments.size(); ++i)
        m_adjustments[i]->setValue(localAdjustmentFields()[i].value(values));
}

void MaskPanel::setTool(MaskEditor::Tool tool)
{
    m_tool = tool;
    int id = ShapeButton;
    if (tool == MaskEditor::Tool::Brush)
        id = m_brush.mode == BrushMode::Subtract ? SubtractButton : m_brush.mode == BrushMode::Erase ? EraseButton : AddButton;
    m_tools->button(id)->setChecked(true);
    m_brushSection->setVisible(tool == MaskEditor::Tool::Brush);
}

void MaskPanel::emitTool()
{
    emit toolChanged(m_tool, m_brush);
}

void MaskPanel::stepBrushSize(int steps)
{
    const double size = m_brushSize->value();
    double next = std::round(size * std::pow(1.15, steps));
    if (next == size)
        next += steps > 0 ? 1 : -1;
    next = std::clamp(next, 1.0, 100.0);
    m_brushSize->setValue(next);
    m_brush.radius = float(next) * kRadiusPerSize;
    emitTool();
}

bool MaskPanel::overlayVisible() const
{
    return m_overlay->isChecked();
}

void MaskPanel::setOverlayVisible(bool visible)
{
    m_overlay->setChecked(visible); // emits overlayToggled
}

} // namespace iris::ui
