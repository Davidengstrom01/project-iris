#pragma once

#include "presets/PresetLibrary.h"

#include <QWidget>

class QTreeWidget;
class QTreeWidgetItem;

namespace iris::ui {

// Lists presets by folder. Clicking a preset applies it; user presets can be renamed,
// moved to another folder and deleted from the context menu.
class PresetPanel : public QWidget {
    Q_OBJECT

public:
    PresetPanel(PresetLibrary* library, QWidget* parent = nullptr);

    void refresh();

signals:
    void presetActivated(const iris::Preset& preset);
    void savePresetRequested();

private:
    void showContextMenu(const QPoint& pos);
    const PresetEntry* entryFor(QTreeWidgetItem* item) const;
    void reportError(const QString& error);

    PresetLibrary* m_library;
    QTreeWidget* m_tree;
};

} // namespace iris::ui
