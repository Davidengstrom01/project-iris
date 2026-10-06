#include "ui/MainWindow.h"

#include "persistence/Sidecar.h"
#include "presets/PresetLibrary.h"
#include "raw/RawDecoder.h"
#include "ui/DevelopPanel.h"
#include "ui/EditDocument.h"
#include "ui/ExportDialog.h"
#include "ui/HistogramWidget.h"
#include "ui/HslPanel.h"
#include "ui/InfoPanel.h"
#include "ui/LibraryPanel.h"
#include "ui/MaskPanel.h"
#include "ui/PhotoSession.h"
#include "ui/PresetPanel.h"
#include "ui/SavePresetDialog.h"
#include "ui/ToneCurvePanel.h"

#include <QAbstractSpinBox>
#include <QAction>
#include <QApplication>
#include <QCloseEvent>
#include <QDragEnterEvent>
#include <QFileDialog>
#include <QFileInfo>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QMimeData>
#include <QScrollArea>
#include <QSettings>
#include <QSplitter>
#include <QStatusBar>
#include <QToolBar>
#include <QVBoxLayout>

#include <cmath>

namespace iris::ui {

namespace {

constexpr qint64 kHoldThresholdMs = 350;

bool isRawFile(const QString& path)
{
    const std::string suffix = QFileInfo(path).suffix().toLower().toStdString();
    for (const std::string& ext : rawFileExtensions())
        if (suffix == ext)
            return true;
    return false;
}

QString rawFileFilter()
{
    QStringList patterns;
    for (const std::string& ext : rawFileExtensions()) {
        const QString e = QString::fromStdString(ext);
        patterns << "*." + e << "*." + e.toUpper();
    }
    return QObject::tr("RAW photos (%1);;All files (*)").arg(patterns.join(' '));
}

bool isBeforeKey(const QKeyEvent* event)
{
    // Some layouts (e.g. Swedish) produce "\" with AltGr, so also match on the text.
    return event->key() == Qt::Key_Backslash || event->text() == QLatin1String("\\");
}

} // namespace

MainWindow::MainWindow(QWidget* parent)
    : QMainWindow(parent),
      m_session(new PhotoSession(this)),
      m_document(new EditDocument(this)),
      m_presetLibrary(std::make_unique<PresetLibrary>(":/presets/builtin", PresetLibrary::defaultUserDirectory()))
{
    setWindowTitle("Project Iris");
    setAcceptDrops(true);
    createActions();
    createLayout();
    connectSession();
    connectEditing();
    updateEditActions();
    qApp->installEventFilter(this);

    QSettings s;
    if (!restoreGeometry(s.value("window/geometry").toByteArray()))
        resize(1500, 950);
}

MainWindow::~MainWindow()
{
    qApp->removeEventFilter(this);
}

void MainWindow::createActions()
{
    auto action = [this](const QString& text, const QKeySequence& shortcut, const QString& tip) {
        auto* a = new QAction(text, this);
        if (!shortcut.isEmpty())
            a->setShortcut(shortcut);
        if (!tip.isEmpty())
            a->setToolTip(tip);
        addAction(a); // shortcuts work window-wide, including actions without a button
        return a;
    };

    m_openAction = action(tr("Open"), QKeySequence::Open, tr("Open a RAW photo (Ctrl+O)"));
    m_saveAction = action(tr("Save"), QKeySequence::Save, tr("Save edits next to the photo (Ctrl+S)"));
    m_saveAsAction = action(tr("Save As…"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_S),
                            tr("Save edits to another file (Ctrl+Shift+S)"));
    m_exportAction = action(tr("Export"), QKeySequence(Qt::CTRL | Qt::Key_E), tr("Export to JPEG, PNG or TIFF (Ctrl+E)"));
    m_undoAction = action(tr("Undo"), QKeySequence::Undo, {});
    m_redoAction = action(tr("Redo"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Z), {});
    m_redoAction->setShortcuts({QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_Z), QKeySequence(Qt::CTRL | Qt::Key_Y)});
    m_resetAction = action(tr("Reset"), QKeySequence(Qt::CTRL | Qt::SHIFT | Qt::Key_R),
                           tr("Reset all edits (Ctrl+Shift+R)"));
    // "\" is handled in eventFilter() so that holding it works too.
    m_beforeAfterAction = action(tr("Before / After"), {}, tr("Show the original photo (\\ toggles, hold to peek)"));
    m_beforeAfterAction->setCheckable(true);
    m_splitAction = action(tr("Split"), QKeySequence(Qt::Key_Y), tr("Side-by-side before/after (Y)"));
    m_splitAction->setCheckable(true);
    m_fitAction = action(tr("Fit"), QKeySequence(Qt::Key_1), tr("Fit image to window (1)"));
    m_actualSizeAction = action(tr("100%"), QKeySequence(Qt::Key_2), tr("View at 100% (2)"));
    m_zoomInAction = action(tr("Zoom In"), QKeySequence::ZoomIn, {});
    m_zoomInAction->setShortcuts({QKeySequence::ZoomIn, QKeySequence(Qt::CTRL | Qt::Key_Equal)});
    m_zoomOutAction = action(tr("Zoom Out"), QKeySequence::ZoomOut, {});
    m_maskAction = action(tr("Masks"), QKeySequence(Qt::Key_M), tr("Edit masks (M)"));
    m_maskAction->setCheckable(true);
    m_overlayAction = action(tr("Mask Overlay"), QKeySequence(Qt::Key_O), {});
    m_exitMaskAction = action(tr("Done"), QKeySequence(Qt::Key_Escape), {});
    m_brushSmallerAction = action(tr("Smaller Brush"), QKeySequence(Qt::Key_BracketLeft), {});
    m_brushLargerAction = action(tr("Larger Brush"), QKeySequence(Qt::Key_BracketRight), {});
    QAction* quit = action(tr("Quit"), QKeySequence::Quit, {});

    connect(m_openAction, &QAction::triggered, this, &MainWindow::showOpenDialog);
    connect(m_saveAction, &QAction::triggered, this, &MainWindow::save);
    connect(m_saveAsAction, &QAction::triggered, this, &MainWindow::saveAs);
    connect(m_exportAction, &QAction::triggered, this, &MainWindow::showExportDialog);
    connect(m_undoAction, &QAction::triggered, m_document, &EditDocument::undo);
    connect(m_redoAction, &QAction::triggered, m_document, &EditDocument::redo);
    connect(m_resetAction, &QAction::triggered, this, [this] { commitEdit(m_document->defaults(), tr("Reset")); });
    connect(m_beforeAfterAction, &QAction::triggered, this, [this](bool on) {
        setCompareMode(on ? ImageView::CompareMode::Before : ImageView::CompareMode::Off);
    });
    connect(m_splitAction, &QAction::triggered, this, [this](bool on) {
        setCompareMode(on ? ImageView::CompareMode::Split : ImageView::CompareMode::Off);
    });
    connect(m_maskAction, &QAction::triggered, this, &MainWindow::toggleMaskEditing);
    connect(m_overlayAction, &QAction::triggered, this,
            [this] { m_maskPanel->setOverlayVisible(!m_maskPanel->overlayVisible()); });
    connect(m_exitMaskAction, &QAction::triggered, this, [this] {
        if (m_view->isPicking()) // Esc cancels the eyedropper first
            m_develop->setEyedropperActive(false);
        else
            selectMask(-1);
    });
    connect(m_brushSmallerAction, &QAction::triggered, this, [this] { m_maskPanel->stepBrushSize(-1); });
    connect(m_brushLargerAction, &QAction::triggered, this, [this] { m_maskPanel->stepBrushSize(1); });
    connect(quit, &QAction::triggered, this, &QWidget::close);
}

void MainWindow::createLayout()
{
    auto* toolbar = new QToolBar(this);
    toolbar->setObjectName("mainToolBar");
    toolbar->setMovable(false);
    toolbar->setFloatable(false);
    toolbar->setContextMenuPolicy(Qt::PreventContextMenu);
    toolbar->setToolButtonStyle(Qt::ToolButtonTextOnly);
    auto* brand = new QLabel("Project Iris", toolbar);
    brand->setObjectName("brandLabel");
    toolbar->addWidget(brand);
    toolbar->addAction(m_openAction);
    toolbar->addAction(m_saveAction);
    toolbar->addAction(m_exportAction);
    toolbar->addSeparator();
    toolbar->addAction(m_undoAction);
    toolbar->addAction(m_redoAction);
    toolbar->addSeparator();
    toolbar->addAction(m_beforeAfterAction);
    toolbar->addAction(m_splitAction);
    toolbar->addSeparator();
    toolbar->addAction(m_maskAction);
    auto* spacer = new QWidget(toolbar);
    spacer->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
    toolbar->addWidget(spacer);
    toolbar->addAction(m_fitAction);
    toolbar->addAction(m_actualSizeAction);
    addToolBar(Qt::TopToolBarArea, toolbar);

    m_library = new LibraryPanel(this);
    m_view = new ImageView(this);

    // Right panel: histogram, presets, develop controls, then photo info.
    auto* rightContent = new QWidget;
    rightContent->setObjectName("sidePanel");
    auto* rightLayout = new QVBoxLayout(rightContent);
    rightLayout->setContentsMargins(0, 6, 0, 0);
    m_histogram = new HistogramWidget(rightContent);
    rightLayout->addWidget(m_histogram);
    m_presets = new PresetPanel(m_presetLibrary.get(), rightContent);
    rightLayout->addWidget(m_presets);
    m_develop = new DevelopPanel(rightContent);
    rightLayout->addWidget(m_develop);
    m_curvePanel = new ToneCurvePanel(rightContent);
    rightLayout->addWidget(m_curvePanel);
    m_hslPanel = new HslPanel(rightContent);
    rightLayout->addWidget(m_hslPanel);
    m_maskPanel = new MaskPanel(rightContent);
    rightLayout->addWidget(m_maskPanel);
    setEditingEnabled(false);
    m_info = new InfoPanel(rightContent);
    rightLayout->addWidget(m_info);
    rightLayout->addStretch();
    auto* rightPanel = new QScrollArea(this);
    rightPanel->setWidget(rightContent);
    rightPanel->setWidgetResizable(true);
    rightPanel->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);

    auto* splitter = new QSplitter(Qt::Horizontal, this);
    splitter->addWidget(m_library);
    splitter->addWidget(m_view);
    splitter->addWidget(rightPanel);
    splitter->setStretchFactor(0, 0);
    splitter->setStretchFactor(1, 1);
    splitter->setStretchFactor(2, 0);
    splitter->setCollapsible(1, false);
    splitter->setHandleWidth(1);
    m_library->setMinimumWidth(150);
    rightPanel->setMinimumWidth(310);
    splitter->setSizes({190, 1000, 320});
    setCentralWidget(splitter);

    // Tab hides both side panels to give the photo the whole window.
    auto* togglePanels = new QAction(tr("Toggle Panels"), this);
    togglePanels->setShortcut(QKeySequence(Qt::Key_Tab));
    connect(togglePanels, &QAction::triggered, this, [this, rightPanel] {
        const bool show = m_library->isHidden() && rightPanel->isHidden();
        m_library->setVisible(show);
        rightPanel->setVisible(show);
    });
    addAction(togglePanels);

    m_zoomLabel = new QLabel(this);
    m_sizeLabel = new QLabel(this);
    statusBar()->addWidget(m_zoomLabel);
    statusBar()->addPermanentWidget(m_sizeLabel);
    statusBar()->setSizeGripEnabled(false);

    connect(m_fitAction, &QAction::triggered, m_view, &ImageView::fitToWindow);
    connect(m_actualSizeAction, &QAction::triggered, m_view, &ImageView::zoomToActualPixels);
    connect(m_zoomInAction, &QAction::triggered, m_view, &ImageView::zoomIn);
    connect(m_zoomOutAction, &QAction::triggered, m_view, &ImageView::zoomOut);
    connect(m_view, &ImageView::zoomChanged, this, &MainWindow::updateZoomLabel);
    connect(m_view, &ImageView::zoomChanged, this,
            [this] { m_session->setFullResolutionNeeded(m_view->needsFullResolution()); });
    connect(m_library, &LibraryPanel::photoActivated, this, &MainWindow::openPhoto);
}

void MainWindow::connectSession()
{
    connect(m_session, &PhotoSession::loadingStarted, this, [this](const QString& path) {
        m_view->beginLoading();
        m_develop->setEyedropperActive(false);
        setEditingEnabled(false);
        m_histogram->clear();
        m_exportAction->setEnabled(false);
        m_info->clear();
        m_sizeLabel->clear();
        statusBar()->showMessage(tr("Decoding %1…").arg(QFileInfo(path).fileName()));
    });
    connect(m_session, &PhotoSession::metadataReady, this, [this](const iris::PhotoMetadata& m) {
        m_info->setMetadata(m, QFileInfo(m_session->path()).fileName());
        // Restore saved edits before anything is rendered.
        const QString warning = m_document->load(m_session->path(), m.asShot);
        setEditingEnabled(true);
        if (!warning.isEmpty())
            QMessageBox::warning(this, tr("Saved edits"),
                                 tr("The saved edits for this photo could not be read and were ignored.\n%1")
                                     .arg(warning));
    });
    connect(m_session, &PhotoSession::fullImageInvalidated, this, [this] { m_view->setFullImage(QImage()); });
    connect(m_session, &PhotoSession::previewReady, this, [this](const QImage& preview, const QSize& fullSize) {
        const bool firstPreview = !m_exportAction->isEnabled();
        m_view->setPreview(preview, fullSize);
        m_exportAction->setEnabled(true);
        m_sizeLabel->setText(QString("%1 × %2").arg(fullSize.width()).arg(fullSize.height()));
        if (firstPreview)
            statusBar()->clearMessage();
    });
    connect(m_session, &PhotoSession::fullImageReady, m_view, &ImageView::setFullImage);
    connect(m_session, &PhotoSession::histogramReady, this, [this](const iris::Histogram& histogram) {
        m_histogram->setHistogram(histogram);
        m_curvePanel->setHistogram(histogram.luminance);
    });
    connect(m_session, &PhotoSession::beforePreviewReady, m_view, &ImageView::setBeforePreview);
    connect(m_session, &PhotoSession::beforeFullReady, m_view, &ImageView::setBeforeFullImage);
    connect(m_session, &PhotoSession::loadFailed, this, [this](const QString& path, const QString& message) {
        m_view->setLoadFailed(tr("Cannot open %1\n%2").arg(QFileInfo(path).fileName(), message));
        m_exportAction->setEnabled(false);
        statusBar()->showMessage(tr("Cannot open %1").arg(QFileInfo(path).fileName()), 8000);
    });
    connect(m_session, &PhotoSession::exportFinished, this, [this](const QString& output, const QString& error) {
        --m_exportsRunning;
        if (error.isEmpty()) {
            statusBar()->showMessage(tr("Exported %1").arg(output), 8000);
        } else {
            statusBar()->clearMessage();
            QMessageBox::warning(this, tr("Export failed"), error);
        }
    });
}

void MainWindow::connectEditing()
{
    // Load, undo and redo replace the edits wholesale.
    connect(m_document, &EditDocument::editsReplaced, this, [this](const iris::EditState& edits) {
        showEdits(edits);
        m_session->setEdits(edits, PhotoSession::Update::Immediate);
    });
    connect(m_document, &EditDocument::stateChanged, this, &MainWindow::updateEditActions);

    // A slider drag becomes one undo step; the preview updates continuously.
    connect(m_develop, &DevelopPanel::adjustmentsChanged, this, [this](const iris::BasicAdjustments& adjustments) {
        EditState state = m_document->edits();
        const QString label = QString::fromStdString(describeChange(state.basic, adjustments));
        state.basic = adjustments;
        m_document->edit(state, label, true);
        m_session->setEdits(m_document->edits(), PhotoSession::Update::Interactive);
    });
    connect(m_develop, &DevelopPanel::resetRequested, this, [this] {
        EditState state = m_document->edits();
        state.basic = m_document->defaults().basic;
        commitEdit(state, tr("Reset Basic"));
    });
    connect(m_develop, &DevelopPanel::whiteBalanceChosen, this,
            [this](const iris::WhiteBalance& wb) { applyWhiteBalance(wb); });
    connect(m_develop, &DevelopPanel::autoWhiteBalanceRequested, this,
            [this] { applyWhiteBalance(m_session->autoWhiteBalance()); });

    connect(m_develop, &DevelopPanel::eyedropperToggled, this, [this](bool active) {
        m_view->setPickMode(active);
        if (active) {
            m_view->setFocus();
            statusBar()->showMessage(tr("Click a neutral grey or white area (Esc to cancel)"));
        } else {
            statusBar()->clearMessage();
        }
    });
    connect(m_view, &ImageView::pointPicked, this, [this](const QPointF& position) {
        const auto wb = m_session->whiteBalanceAt(position);
        if (!wb) {
            statusBar()->showMessage(tr("That area is too dark or clipped. Pick a neutral, well-exposed area."));
            return;
        }
        m_develop->setEyedropperActive(false);
        applyWhiteBalance(wb);
    });
    connect(m_view, &ImageView::pickCancelled, this, [this] { m_develop->setEyedropperActive(false); });

    // Dragging curve points is interactive (one undo step per gesture); presets and reset
    // are separate steps.
    connect(m_curvePanel, &ToneCurvePanel::curveEdited, this, [this](const iris::ToneCurve& curve) {
        EditState state = m_document->edits();
        state.toneCurve = curve;
        m_document->edit(state, tr("Tone Curve"), true);
        m_session->setEdits(m_document->edits(), PhotoSession::Update::Interactive);
    });
    connect(m_curvePanel, &ToneCurvePanel::curveChosen, this, [this](const iris::ToneCurve& curve) {
        EditState state = m_document->edits();
        state.toneCurve = curve;
        commitEdit(state, tr("Tone Curve Preset"));
    });

    connect(m_hslPanel, &HslPanel::hslEdited, this, [this](const iris::HslAdjustments& hsl, const QString& label) {
        EditState state = m_document->edits();
        state.hsl = hsl;
        m_document->edit(state, label, true);
        m_session->setEdits(m_document->edits(), PhotoSession::Update::Interactive);
    });
    connect(m_hslPanel, &HslPanel::resetRequested, this, [this] {
        EditState state = m_document->edits();
        state.hsl = {};
        commitEdit(state, tr("Reset Color"));
    });

    // Masks: panel sliders behave like other sliders; a brush stroke or handle drag on the
    // photo is one undo step.
    connect(m_maskPanel, &MaskPanel::addRequested, this, &MainWindow::addMask);
    connect(m_maskPanel, &MaskPanel::deleteRequested, this, &MainWindow::deleteMask);
    connect(m_maskPanel, &MaskPanel::selectionRequested, this, &MainWindow::selectMask);
    connect(m_maskPanel, &MaskPanel::maskEdited, this, [this](const iris::Mask& mask, const QString& label) {
        editSelectedMask(mask, label);
        m_maskPanel->setMasks(m_document->edits().masks, m_selectedMask);
        m_view->setMask(mask);
    });
    connect(m_maskPanel, &MaskPanel::toolChanged, this, [this](MaskEditor::Tool tool, const MaskEditor::Brush& brush) {
        m_view->setMaskTool(tool);
        m_view->setBrush(brush);
    });
    connect(m_maskPanel, &MaskPanel::overlayToggled, this, &MainWindow::updateMaskEditing);
    connect(m_view, &ImageView::maskGestureStarted, m_document, &EditDocument::beginGesture);
    connect(m_view, &ImageView::maskEdited, this, [this](const iris::Mask& mask, const QString& label) {
        editSelectedMask(mask, label);
        m_maskPanel->setMasks(m_document->edits().masks, m_selectedMask);
    });
    connect(m_view, &ImageView::maskGestureFinished, m_document, &EditDocument::endGesture);
    connect(m_session, &PhotoSession::maskOverlayReady, this, [this](const QImage& overlay) {
        if (m_selectedMask >= 0)
            m_view->setMaskOverlay(overlay);
    });

    connect(m_presets, &PresetPanel::presetActivated, this, &MainWindow::applyPreset);
    connect(m_presets, &PresetPanel::savePresetRequested, this, &MainWindow::showSavePresetDialog);
}

void MainWindow::commitEdit(const EditState& state, const QString& label)
{
    if (!m_document->isLoaded())
        return;
    m_document->edit(state, label);
    showEdits(m_document->edits());
    m_session->setEdits(m_document->edits(), PhotoSession::Update::Immediate);
}

void MainWindow::showEdits(const EditState& edits)
{
    m_develop->setAdjustments(edits.basic, m_document->defaults().basic.whiteBalance);
    m_curvePanel->setCurve(edits.toneCurve);
    m_hslPanel->setHsl(edits.hsl);
    if (m_selectedMask >= int(edits.masks.size())) // e.g. undoing "Add Mask"
        m_selectedMask = -1;
    m_maskPanel->setMasks(edits.masks, m_selectedMask);
    updateMaskEditing();
}

void MainWindow::setEditingEnabled(bool enabled)
{
    m_presets->setEnabled(enabled);
    m_develop->setEnabled(enabled);
    m_curvePanel->setEnabled(enabled);
    m_hslPanel->setEnabled(enabled);
    m_maskPanel->setEnabled(enabled);
    m_maskAction->setEnabled(enabled);
}

void MainWindow::applyWhiteBalance(const std::optional<iris::WhiteBalance>& wb)
{
    if (!wb)
        return;
    EditState state = m_document->edits();
    state.basic.whiteBalance = *wb;
    commitEdit(state, tr("White Balance"));
}

void MainWindow::applyPreset(const iris::Preset& preset)
{
    const QString name = QString::fromStdString(preset.name);
    commitEdit(iris::applyPreset(m_document->edits(), preset), tr("Preset: %1").arg(name));
    statusBar()->showMessage(tr("Applied preset “%1”").arg(name), 4000);
}

void MainWindow::selectMask(int index)
{
    const auto& masks = m_document->edits().masks;
    m_selectedMask = m_document->isLoaded() && index >= 0 && index < int(masks.size()) ? index : -1;
    if (m_selectedMask >= 0)
        m_lastSelectedMask = m_selectedMask;
    m_maskPanel->setMasks(masks, m_selectedMask);
    updateMaskEditing();
    if (m_selectedMask >= 0) {
        m_view->setFocus(); // for [ ] and Space
        const bool brush = m_maskPanel->tool() == MaskEditor::Tool::Brush;
        statusBar()->showMessage(brush ? tr("Paint on the photo. [ ] change the brush size, Space + drag pans, "
                                            "Esc when done.")
                                       : tr("Drag the handles, or drag on the photo to draw the gradient again. "
                                            "Esc when done."));
    } else {
        statusBar()->clearMessage();
    }
}

void MainWindow::updateMaskEditing()
{
    const auto& masks = m_document->edits().masks;
    const bool editing = m_selectedMask >= 0 && m_selectedMask < int(masks.size());
    m_view->setMaskTool(m_maskPanel->tool());
    m_view->setBrush(m_maskPanel->brush());
    m_view->setMask(editing ? std::optional<Mask>(masks[m_selectedMask]) : std::nullopt);
    m_session->setMaskOverlay(editing && m_maskPanel->overlayVisible() ? m_selectedMask : -1);
    m_maskAction->setChecked(editing);
    m_exitMaskAction->setEnabled(editing);
    m_brushSmallerAction->setEnabled(editing);
    m_brushLargerAction->setEnabled(editing);
    m_overlayAction->setEnabled(editing);
}

void MainWindow::addMask(iris::MaskType type)
{
    if (!m_document->isLoaded())
        return;
    EditState state = m_document->edits();
    if (state.masks.size() >= kMaxMasks) {
        statusBar()->showMessage(tr("A photo can have at most %1 masks.").arg(kMaxMasks), 4000);
        return;
    }
    state.masks.push_back(newMask(type, state.masks));
    m_selectedMask = int(state.masks.size()) - 1;
    commitEdit(state, tr("Add %1 Mask").arg(tr(maskTypeName(type))));
    selectMask(m_selectedMask);
}

void MainWindow::deleteMask(int index)
{
    EditState state = m_document->edits();
    if (index < 0 || index >= int(state.masks.size()))
        return;
    const QString name = QString::fromStdString(state.masks[index].name);
    state.masks.erase(state.masks.begin() + index);
    m_selectedMask = -1;
    commitEdit(state, tr("Delete %1").arg(name));
    selectMask(-1);
}

void MainWindow::toggleMaskEditing()
{
    if (!m_document->isLoaded()) {
        m_maskAction->setChecked(false);
        return;
    }
    const auto& masks = m_document->edits().masks;
    if (m_selectedMask >= 0)
        selectMask(-1);
    else if (masks.empty())
        addMask(MaskType::Brush);
    else
        selectMask(std::min(m_lastSelectedMask, int(masks.size()) - 1));
}

void MainWindow::editSelectedMask(const iris::Mask& mask, const QString& label)
{
    EditState state = m_document->edits();
    if (m_selectedMask < 0 || m_selectedMask >= int(state.masks.size()))
        return;
    state.masks[m_selectedMask] = mask;
    m_document->edit(state, label, true);
    m_session->setEdits(m_document->edits(), PhotoSession::Update::Interactive);
}

void MainWindow::showSavePresetDialog()
{
    if (!m_document->isLoaded())
        return;
    SavePresetDialog dialog(m_document->edits(), m_presetLibrary->userFolders(), this);
    if (dialog.exec() != QDialog::Accepted)
        return;
    const Preset preset = dialog.preset();
    for (const PresetEntry& entry : m_presetLibrary->presets()) {
        if (!entry.builtIn && entry.folder == dialog.folder() && entry.preset.name == preset.name &&
            QMessageBox::question(this, tr("Save Preset"),
                                  tr("A preset named “%1” already exists in %2. Replace it?")
                                      .arg(QString::fromStdString(preset.name), entry.folder)) != QMessageBox::Yes)
            return;
    }
    const QString error = m_presetLibrary->save(preset, dialog.folder());
    if (!error.isEmpty()) {
        QMessageBox::warning(this, tr("Save Preset"), error);
        return;
    }
    m_presets->refresh();
    statusBar()->showMessage(tr("Saved preset “%1”").arg(QString::fromStdString(preset.name)), 4000);
}

void MainWindow::setCompareMode(ImageView::CompareMode mode)
{
    if (!m_session->isLoaded())
        mode = ImageView::CompareMode::Off;
    m_view->setCompareMode(mode);
    m_session->setBeforeNeeded(mode != ImageView::CompareMode::Off);
    m_beforeAfterAction->setChecked(mode == ImageView::CompareMode::Before);
    m_splitAction->setChecked(mode == ImageView::CompareMode::Split);
}

void MainWindow::updateEditActions()
{
    const bool loaded = m_document->isLoaded();
    m_undoAction->setEnabled(m_document->canUndo());
    m_redoAction->setEnabled(m_document->canRedo());
    m_undoAction->setToolTip(m_document->canUndo() ? tr("Undo %1 (Ctrl+Z)").arg(m_document->undoLabel())
                                                   : tr("Undo (Ctrl+Z)"));
    m_redoAction->setToolTip(m_document->canRedo() ? tr("Redo %1 (Ctrl+Shift+Z)").arg(m_document->redoLabel())
                                                   : tr("Redo (Ctrl+Shift+Z)"));
    m_saveAction->setEnabled(loaded);
    m_saveAsAction->setEnabled(loaded);
    m_resetAction->setEnabled(loaded);

    const QString name = QFileInfo(m_session->path()).fileName();
    if (name.isEmpty())
        setWindowTitle("Project Iris");
    else
        setWindowTitle(QString("%1%2 — Project Iris").arg(name, m_document->isDirty() ? " •" : ""));
}

bool MainWindow::maybeSaveChanges()
{
    if (!m_document->isDirty())
        return true;
    const auto answer = QMessageBox::question(
        this, tr("Unsaved edits"),
        tr("Save the edits to %1 before continuing?").arg(QFileInfo(m_document->rawPath()).fileName()),
        QMessageBox::Save | QMessageBox::Discard | QMessageBox::Cancel, QMessageBox::Save);
    if (answer == QMessageBox::Cancel)
        return false;
    if (answer == QMessageBox::Save) {
        const QString error = m_document->save();
        if (!error.isEmpty()) {
            QMessageBox::warning(this, tr("Save failed"), error);
            return false;
        }
        m_library->setEdited(m_document->rawPath(), true);
    }
    return true;
}

void MainWindow::save()
{
    const QString error = m_document->save();
    if (!error.isEmpty()) {
        QMessageBox::warning(this, tr("Save failed"), tr("Cannot save edits:\n%1").arg(error));
        return;
    }
    m_library->setEdited(m_document->rawPath(), true);
    statusBar()->showMessage(tr("Saved edits to %1").arg(QFileInfo(m_document->sidecarPath()).fileName()), 4000);
}

void MainWindow::saveAs()
{
    if (!m_document->isLoaded())
        return;
    QString path = QFileDialog::getSaveFileName(this, tr("Save Edits As"), m_document->sidecarPath(),
                                                tr("Project Iris edits (*.iris.json)"));
    if (path.isEmpty())
        return;
    if (!path.endsWith(".iris.json"))
        path += ".iris.json";
    const QString error = m_document->saveAs(path);
    if (!error.isEmpty())
        QMessageBox::warning(this, tr("Save failed"), tr("Cannot save edits:\n%1").arg(error));
    else
        statusBar()->showMessage(tr("Saved edits to %1").arg(path), 4000);
}

void MainWindow::openPhoto(const QString& path)
{
    const QString absolute = QFileInfo(path).absoluteFilePath();
    if (absolute == m_session->path())
        return;
    if (!maybeSaveChanges()) {
        m_library->showFolderOf(m_session->path()); // keep the current photo selected
        return;
    }
    setCompareMode(ImageView::CompareMode::Off);
    selectMask(-1);
    m_document->clear();
    m_library->showFolderOf(absolute);
    m_session->open(absolute);
    updateEditActions();
    QSettings().setValue("lastDirectory", QFileInfo(absolute).absolutePath());
}

void MainWindow::showOpenDialog()
{
    const QString dir = QSettings().value("lastDirectory").toString();
    const QString path = QFileDialog::getOpenFileName(this, tr("Open RAW Photo"), dir, rawFileFilter());
    if (!path.isEmpty())
        openPhoto(path);
}

void MainWindow::showExportDialog()
{
    if (!m_session->isLoaded())
        return;
    ExportDialog dialog(m_session->path(), this);
    if (dialog.exec() != QDialog::Accepted)
        return;
    ++m_exportsRunning;
    statusBar()->showMessage(tr("Exporting %1…").arg(QFileInfo(dialog.outputPath()).fileName()));
    m_session->exportTo(dialog.outputPath(), dialog.settings());
}

void MainWindow::updateZoomLabel(double zoom, bool fit)
{
    if (!m_view->hasImage()) {
        m_zoomLabel->clear();
        return;
    }
    const QString percent = QString("%1%").arg(zoom * 100, 0, 'f', zoom < 0.1 ? 1 : 0);
    m_zoomLabel->setText(fit ? tr("Fit  ·  %1").arg(percent) : percent);
}

bool MainWindow::eventFilter(QObject* watched, QEvent* event)
{
    if ((event->type() == QEvent::KeyPress || event->type() == QEvent::KeyRelease) && isActiveWindow()) {
        auto* key = static_cast<QKeyEvent*>(event);
        QWidget* focus = QApplication::focusWidget();
        const bool typing = qobject_cast<QLineEdit*>(focus) || qobject_cast<QAbstractSpinBox*>(focus);
        if (isBeforeKey(key) && !typing && m_session->isLoaded()) {
            if (!key->isAutoRepeat()) {
                if (event->type() == QEvent::KeyPress) {
                    m_modeBeforeKey = m_view->compareMode();
                    m_beforeKeyTimer.start();
                    setCompareMode(m_modeBeforeKey == ImageView::CompareMode::Before ? ImageView::CompareMode::Off
                                                                                      : ImageView::CompareMode::Before);
                } else if (m_beforeKeyTimer.isValid() && m_beforeKeyTimer.elapsed() > kHoldThresholdMs) {
                    setCompareMode(m_modeBeforeKey); // held: peek only
                }
            }
            return true;
        }
    }
    return QMainWindow::eventFilter(watched, event);
}

void MainWindow::dragEnterEvent(QDragEnterEvent* event)
{
    const QList<QUrl> urls = event->mimeData()->urls();
    if (!urls.isEmpty() && urls.first().isLocalFile() && isRawFile(urls.first().toLocalFile()))
        event->acceptProposedAction();
}

void MainWindow::dropEvent(QDropEvent* event)
{
    openPhoto(event->mimeData()->urls().first().toLocalFile());
}

void MainWindow::closeEvent(QCloseEvent* event)
{
    if (m_exportsRunning > 0 &&
        QMessageBox::question(this, tr("Export in progress"), tr("An export is still running. Quit anyway?"),
                              QMessageBox::Yes | QMessageBox::No, QMessageBox::No) != QMessageBox::Yes) {
        event->ignore();
        return;
    }
    if (!maybeSaveChanges()) {
        event->ignore();
        return;
    }
    QSettings().setValue("window/geometry", saveGeometry());
    event->accept();
}

} // namespace iris::ui
