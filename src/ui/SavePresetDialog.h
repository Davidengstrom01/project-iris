#pragma once

#include "core/EditState.h"
#include "presets/Preset.h"

#include <QDialog>
#include <QList>
#include <QPair>

class QCheckBox;
class QComboBox;
class QPushButton;
class QLineEdit;

namespace iris::ui {

// Name, folder and which settings to include in a new preset.
class SavePresetDialog : public QDialog {
    Q_OBJECT

public:
    SavePresetDialog(const iris::BasicAdjustments& adjustments, const QStringList& folders, QWidget* parent = nullptr);

    Preset preset() const;
    QString folder() const;

private:
    void updateOkButton();

    BasicAdjustments m_adjustments;
    QLineEdit* m_name;
    QComboBox* m_folder;
    QList<QPair<QCheckBox*, std::vector<std::string>>> m_groups; // checkbox -> adjustment keys
    QPushButton* m_ok = nullptr;
};

} // namespace iris::ui
