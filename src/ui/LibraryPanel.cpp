#include "ui/LibraryPanel.h"

#include "raw/RawDecoder.h"

#include <QDir>
#include <QFileInfo>
#include <QLabel>
#include <QListWidget>
#include <QSignalBlocker>
#include <QVBoxLayout>

namespace iris::ui {

namespace {

QStringList rawNameFilters()
{
    QStringList filters;
    for (const std::string& ext : rawFileExtensions()) {
        const QString e = QString::fromStdString(ext);
        filters << "*." + e << "*." + e.toUpper();
    }
    return filters;
}

} // namespace

LibraryPanel::LibraryPanel(QWidget* parent) : QWidget(parent)
{
    setObjectName("sidePanel");
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);

    auto* title = new QLabel(tr("LIBRARY"), this);
    title->setObjectName("panelTitle");
    layout->addWidget(title);

    m_folderLabel = new QLabel(tr("No folder"), this);
    m_folderLabel->setObjectName("folderLabel");
    m_folderLabel->setWordWrap(true);
    layout->addWidget(m_folderLabel);

    m_list = new QListWidget(this);
    m_list->setObjectName("photoList");
    m_list->setUniformItemSizes(true);
    layout->addWidget(m_list, 1);

    connect(m_list, &QListWidget::currentItemChanged, this, [this](QListWidgetItem* item) {
        if (item)
            emit photoActivated(item->data(Qt::UserRole).toString());
    });
}

void LibraryPanel::showFolderOf(const QString& filePath)
{
    const QFileInfo info(filePath);
    const QString folder = info.absolutePath();
    const QSignalBlocker blocker(m_list);

    if (folder != m_folder) {
        m_folder = folder;
        m_list->clear();
        m_folderLabel->setText(QDir(folder).dirName());
        m_folderLabel->setToolTip(folder);
        const QFileInfoList files =
            QDir(folder).entryInfoList(rawNameFilters(), QDir::Files | QDir::Readable, QDir::Name | QDir::IgnoreCase);
        for (const QFileInfo& file : files) {
            auto* item = new QListWidgetItem(file.fileName(), m_list);
            item->setData(Qt::UserRole, file.absoluteFilePath());
        }
    }

    const QString absolute = info.absoluteFilePath();
    for (int i = 0; i < m_list->count(); ++i) {
        if (m_list->item(i)->data(Qt::UserRole).toString() == absolute) {
            // setCurrentIndex (unlike setCurrentRow) marks the view's current index as set, so
            // Qt does not jump back to the first row when the list later gains focus.
            m_list->setCurrentIndex(m_list->model()->index(i, 0));
            m_list->scrollToItem(m_list->item(i));
            break;
        }
    }
}

} // namespace iris::ui
