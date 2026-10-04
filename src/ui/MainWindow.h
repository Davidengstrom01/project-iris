#pragma once

#include <QMainWindow>

class QAction;
class QLabel;

namespace iris::ui {

class ImageView;
class InfoPanel;
class LibraryPanel;
class PhotoSession;

class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(QWidget* parent = nullptr);

    void openPhoto(const QString& path);

protected:
    void dragEnterEvent(QDragEnterEvent* event) override;
    void dropEvent(QDropEvent* event) override;
    void closeEvent(QCloseEvent* event) override;

private:
    void createActions();
    void createLayout();
    void connectSession();
    void showOpenDialog();
    void showExportDialog();
    void updateZoomLabel(double zoom, bool fit);

    PhotoSession* m_session;
    ImageView* m_view;
    LibraryPanel* m_library;
    InfoPanel* m_info;
    QLabel* m_zoomLabel;
    QLabel* m_sizeLabel;

    QAction* m_openAction;
    QAction* m_exportAction;
    QAction* m_fitAction;
    QAction* m_actualSizeAction;
    QAction* m_zoomInAction;
    QAction* m_zoomOutAction;
    int m_exportsRunning = 0;
};

} // namespace iris::ui
