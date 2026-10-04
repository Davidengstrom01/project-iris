#include "ui/PresetPanel.h"

#include <QHBoxLayout>
#include <QHeaderView>
#include <QInputDialog>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QPushButton>
#include <QSet>
#include <QTreeWidget>
#include <QVBoxLayout>

namespace iris::ui {

namespace {

constexpr int kEntryIndexRole = Qt::UserRole + 1;

} // namespace

PresetPanel::PresetPanel(PresetLibrary* library, QWidget* parent) : QWidget(parent), m_library(library)
{
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 6);
    layout->setSpacing(2);

    auto* titleRow = new QHBoxLayout;
    auto* title = new QLabel(tr("PRESETS"), this);
    title->setObjectName("panelTitle");
    auto* save = new QPushButton(tr("Save Preset…"), this);
    save->setObjectName("smallButton");
    save->setFocusPolicy(Qt::NoFocus);
    save->setToolTip(tr("Save the current settings as a preset"));
    titleRow->addWidget(title);
    titleRow->addStretch();
    titleRow->addWidget(save);
    titleRow->addSpacing(12);
    layout->addLayout(titleRow);

    m_tree = new QTreeWidget(this);
    m_tree->setObjectName("presetTree");
    m_tree->setHeaderHidden(true);
    m_tree->setRootIsDecorated(true);
    m_tree->setIndentation(14);
    m_tree->setContextMenuPolicy(Qt::CustomContextMenu);
    m_tree->setFixedHeight(220);
    layout->addWidget(m_tree);

    connect(save, &QPushButton::clicked, this, &PresetPanel::savePresetRequested);
    connect(m_tree, &QTreeWidget::itemClicked, this, [this](QTreeWidgetItem* item) {
        if (const PresetEntry* entry = entryFor(item))
            emit presetActivated(entry->preset);
    });
    connect(m_tree, &QTreeWidget::itemActivated, this, [this](QTreeWidgetItem* item) {
        if (const PresetEntry* entry = entryFor(item))
            emit presetActivated(entry->preset);
    });
    connect(m_tree, &QTreeWidget::customContextMenuRequested, this, &PresetPanel::showContextMenu);
    refresh();
}

void PresetPanel::refresh()
{
    QSet<QString> collapsed;
    for (int i = 0; i < m_tree->topLevelItemCount(); ++i)
        if (!m_tree->topLevelItem(i)->isExpanded())
            collapsed.insert(m_tree->topLevelItem(i)->text(0));

    m_tree->clear();
    QTreeWidgetItem* folder = nullptr;
    const QList<PresetEntry>& presets = m_library->presets();
    for (int i = 0; i < presets.size(); ++i) {
        const PresetEntry& entry = presets[i];
        if (!folder || folder->text(0) != entry.folder) {
            folder = new QTreeWidgetItem(m_tree, {entry.folder});
            folder->setFlags(Qt::ItemIsEnabled);
            QFont font = folder->font(0);
            font.setBold(true);
            folder->setFont(0, font);
        }
        auto* item = new QTreeWidgetItem(folder, {QString::fromStdString(entry.preset.name)});
        item->setData(0, kEntryIndexRole, i);
        item->setToolTip(0, entry.builtIn ? tr("Built-in preset") : entry.filePath);
    }
    for (int i = 0; i < m_tree->topLevelItemCount(); ++i)
        m_tree->topLevelItem(i)->setExpanded(!collapsed.contains(m_tree->topLevelItem(i)->text(0)));
}

const PresetEntry* PresetPanel::entryFor(QTreeWidgetItem* item) const
{
    if (!item || !item->data(0, kEntryIndexRole).isValid())
        return nullptr;
    const int index = item->data(0, kEntryIndexRole).toInt();
    return index >= 0 && index < m_library->presets().size() ? &m_library->presets()[index] : nullptr;
}

void PresetPanel::showContextMenu(const QPoint& pos)
{
    const PresetEntry* found = entryFor(m_tree->itemAt(pos));
    if (!found)
        return;
    const PresetEntry entry = *found; // the library is reloaded by the actions below

    QMenu menu(this);
    QAction* apply = menu.addAction(tr("Apply"));
    menu.addSeparator();
    QAction* rename = menu.addAction(tr("Rename…"));
    QAction* move = menu.addAction(tr("Move to Folder…"));
    QAction* remove = menu.addAction(tr("Delete"));
    for (QAction* action : {rename, move, remove})
        action->setEnabled(!entry.builtIn);

    QAction* chosen = menu.exec(m_tree->viewport()->mapToGlobal(pos));
    const QString name = QString::fromStdString(entry.preset.name);
    if (chosen == apply) {
        emit presetActivated(entry.preset);
    } else if (chosen == rename) {
        bool ok = false;
        const QString newName = QInputDialog::getText(this, tr("Rename Preset"), tr("Name:"), QLineEdit::Normal, name, &ok);
        if (ok && newName.trimmed() != name)
            reportError(m_library->rename(entry, newName));
    } else if (chosen == move) {
        bool ok = false;
        const QString folder = QInputDialog::getItem(this, tr("Move Preset"), tr("Folder:"), m_library->userFolders(),
                                                     m_library->userFolders().indexOf(entry.folder), true, &ok);
        if (ok)
            reportError(m_library->move(entry, folder));
    } else if (chosen == remove) {
        if (QMessageBox::question(this, tr("Delete Preset"), tr("Delete the preset “%1”?").arg(name)) ==
            QMessageBox::Yes)
            reportError(m_library->remove(entry));
    }
    refresh();
}

void PresetPanel::reportError(const QString& error)
{
    if (!error.isEmpty())
        QMessageBox::warning(this, tr("Presets"), error);
}

} // namespace iris::ui
