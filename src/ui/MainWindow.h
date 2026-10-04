#pragma once

#include "core/EditState.h"
#include "ui/ImageView.h"

#include <QElapsedTimer>
#include <QMainWindow>

#include <memory>
#include <optional>

class QAction;
class QLabel;

namespace iris {
class PresetLibrary;
struct Preset;
}

namespace iris::ui {

class DevelopPanel;
class EditDocument;
class InfoPanel;
class LibraryPanel;
class PhotoSession;
class PresetPanel;

class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(QWidget* parent = nullptr);
    ~MainWindow() override;

    void openPhoto(const QString& path);

protected:
    bool eventFilter(QObject* watched, QEvent* event) override;
    void dragEnterEvent(QDragEnterEvent* event) override;
    void dropEvent(QDropEvent* event) override;
    void closeEvent(QCloseEvent* event) override;

private:
    void createActions();
    void createLayout();
    void connectSession();
    void connectEditing();

    void showOpenDialog();
    void showExportDialog();
    void save();
    void saveAs();
    void showSavePresetDialog();
    // Asks to save unsaved edits. Returns false if the user cancelled.
    bool maybeSaveChanges();

    // Records an edit made by anything but a slider drag (presets, white balance, reset).
    void commitEdit(const iris::EditState& state, const QString& label);
    void applyWhiteBalance(const std::optional<iris::WhiteBalance>& wb);
    void applyPreset(const iris::Preset& preset);
    void setCompareMode(ImageView::CompareMode mode);
    void updateEditActions();
    void updateZoomLabel(double zoom, bool fit);

    PhotoSession* m_session;
    EditDocument* m_document;
    std::unique_ptr<PresetLibrary> m_presetLibrary;

    ImageView* m_view;
    LibraryPanel* m_library;
    PresetPanel* m_presets;
    DevelopPanel* m_develop;
    InfoPanel* m_info;
    QLabel* m_zoomLabel;
    QLabel* m_sizeLabel;

    QAction* m_openAction;
    QAction* m_saveAction;
    QAction* m_saveAsAction;
    QAction* m_exportAction;
    QAction* m_undoAction;
    QAction* m_redoAction;
    QAction* m_resetAction;
    QAction* m_beforeAfterAction;
    QAction* m_splitAction;
    QAction* m_fitAction;
    QAction* m_actualSizeAction;
    QAction* m_zoomInAction;
    QAction* m_zoomOutAction;

    int m_exportsRunning = 0;
    // "\" toggles before/after on a tap and shows "before" only while held on a long press.
    QElapsedTimer m_beforeKeyTimer;
    ImageView::CompareMode m_modeBeforeKey = ImageView::CompareMode::Off;
};

} // namespace iris::ui
