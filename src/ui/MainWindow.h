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
class HistogramWidget;
class HslPanel;
class InfoPanel;
class LibraryPanel;
class MaskPanel;
class PhotoSession;
class PresetPanel;
class ToneCurvePanel;

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
    // Shows edits in the panels (without emitting edit signals).
    void showEdits(const iris::EditState& edits);
    void setEditingEnabled(bool enabled);
    void applyWhiteBalance(const std::optional<iris::WhiteBalance>& wb);
    void applyPreset(const iris::Preset& preset);
    void setCompareMode(ImageView::CompareMode mode);
    void updateEditActions();
    void updateZoomLabel(double zoom, bool fit);

    // Masks: the selected mask is edited on the photo (-1 = none, normal viewing).
    void selectMask(int index);
    void updateMaskEditing();
    void addMask(iris::MaskType type);
    void deleteMask(int index);
    void toggleMaskEditing();
    // Replaces the selected mask (a slider, brush stroke or handle drag).
    void editSelectedMask(const iris::Mask& mask, const QString& label);

    PhotoSession* m_session;
    EditDocument* m_document;
    std::unique_ptr<PresetLibrary> m_presetLibrary;

    ImageView* m_view;
    LibraryPanel* m_library;
    PresetPanel* m_presets;
    DevelopPanel* m_develop;
    ToneCurvePanel* m_curvePanel;
    HslPanel* m_hslPanel;
    MaskPanel* m_maskPanel;
    HistogramWidget* m_histogram;
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
    QAction* m_maskAction;
    QAction* m_overlayAction;
    QAction* m_exitMaskAction;
    QAction* m_brushSmallerAction;
    QAction* m_brushLargerAction;

    int m_selectedMask = -1;
    int m_lastSelectedMask = 0;

    int m_exportsRunning = 0;
    // "\" toggles before/after on a tap and shows "before" only while held on a long press.
    QElapsedTimer m_beforeKeyTimer;
    ImageView::CompareMode m_modeBeforeKey = ImageView::CompareMode::Off;
};

} // namespace iris::ui
