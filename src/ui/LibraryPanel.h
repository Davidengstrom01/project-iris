#pragma once

#include <QWidget>

class QLabel;
class QListWidget;

namespace iris::ui {

// Lists the RAW files in the folder of the current photo. Deliberately simple:
// no catalog or database, just the files on disk.
class LibraryPanel : public QWidget {
    Q_OBJECT

public:
    explicit LibraryPanel(QWidget* parent = nullptr);

    // Shows the folder containing filePath and selects that file.
    void showFolderOf(const QString& filePath);
    // Marks a photo as having saved edits (a sidecar file).
    void setEdited(const QString& filePath, bool edited);

signals:
    void photoActivated(const QString& path);

private:
    QLabel* m_folderLabel;
    QListWidget* m_list;
    QString m_folder;
};

} // namespace iris::ui
