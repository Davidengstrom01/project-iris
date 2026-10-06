#pragma once

#include "core/Mask.h"
#include "ui/MaskEditor.h"

#include <QWidget>

#include <vector>

class QButtonGroup;
class QCheckBox;
class QLabel;
class QListWidget;
class QPushButton;

namespace iris::ui {

class AdjustmentSlider;

// "Masks" section: the photo's masks, the brush, the selected mask's shape and its local
// adjustments. Which mask is selected is owned by the caller (MainWindow).
class MaskPanel : public QWidget {
    Q_OBJECT

public:
    explicit MaskPanel(QWidget* parent = nullptr);

    // Shows the masks with one selected (-1 = none). Does not emit signals.
    void setMasks(const std::vector<iris::Mask>& masks, int selected);
    int selected() const { return m_selected; }

    MaskEditor::Tool tool() const { return m_tool; }
    MaskEditor::Brush brush() const { return m_brush; }
    // Changes the brush size by a number of steps ([ and ] keys).
    void stepBrushSize(int steps);
    bool overlayVisible() const;
    void setOverlayVisible(bool visible);

signals:
    void addRequested(iris::MaskType type);
    void deleteRequested(int index);
    void selectionRequested(int index);
    // A slider or Invert changed the selected mask; label names it for undo.
    void maskEdited(const iris::Mask& mask, const QString& label);
    void toolChanged(iris::ui::MaskEditor::Tool tool, const iris::ui::MaskEditor::Brush& brush);
    void overlayToggled(bool visible);

private:
    AdjustmentSlider* addSlider(QWidget* section, const QString& name, double min, double max, int decimals,
                                double defaultValue);
    void editShape(const QString& property, void (*apply)(Mask&, float), double value);
    void showSelected();
    void setTool(MaskEditor::Tool tool);
    void emitTool();

    std::vector<Mask> m_masks;
    int m_selected = -1;
    MaskEditor::Tool m_tool = MaskEditor::Tool::Brush;
    MaskEditor::Brush m_brush;

    QListWidget* m_list;
    QLabel* m_hint;
    QPushButton* m_delete;
    QCheckBox* m_overlay;
    QWidget* m_details;
    QButtonGroup* m_tools;
    QPushButton* m_shapeTool;
    QWidget* m_brushSection;
    AdjustmentSlider* m_brushSize;
    AdjustmentSlider* m_brushFeather;
    AdjustmentSlider* m_brushOpacity;
    QWidget* m_linearSection;
    AdjustmentSlider* m_linearAngle;
    AdjustmentSlider* m_linearFeather;
    QWidget* m_radialSection;
    AdjustmentSlider* m_radialWidth;
    AdjustmentSlider* m_radialHeight;
    AdjustmentSlider* m_radialRotation;
    AdjustmentSlider* m_radialFeather;
    QCheckBox* m_invert;
    std::vector<AdjustmentSlider*> m_adjustments; // in localAdjustmentFields() order
};

} // namespace iris::ui
