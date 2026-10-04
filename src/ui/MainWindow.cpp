#include "ui/MainWindow.h"

#include "raw/RawDecoder.h"
#include "ui/ExportDialog.h"
#include "ui/ImageView.h"
#include "ui/InfoPanel.h"
#include "ui/LibraryPanel.h"
#include "ui/PhotoSession.h"

#include <QAction>
#include <QCloseEvent>
#include <QDragEnterEvent>
#include <QFileDialog>
#include <QFileInfo>
#include <QLabel>
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

} // namespace

MainWindow::MainWindow(QWidget* parent) : QMainWindow(parent), m_session(new PhotoSession(this))
{
    setWindowTitle("Project Iris");
    setAcceptDrops(true);
    createActions();
    createLayout();
    connectSession();

    QSettings s;
    if (!restoreGeometry(s.value("window/geometry").toByteArray()))
        resize(1500, 950);
}

void MainWindow::createActions()
{
    m_openAction = new QAction(tr("Open"), this);
    m_openAction->setShortcut(QKeySequence::Open);
    m_openAction->setToolTip(tr("Open a RAW photo (Ctrl+O)"));
    connect(m_openAction, &QAction::triggered, this, &MainWindow::showOpenDialog);

    m_exportAction = new QAction(tr("Export"), this);
    m_exportAction->setShortcut(QKeySequence(Qt::CTRL | Qt::Key_E));
    m_exportAction->setToolTip(tr("Export to JPEG, PNG or TIFF (Ctrl+E)"));
    m_exportAction->setEnabled(false);
    connect(m_exportAction, &QAction::triggered, this, &MainWindow::showExportDialog);

    m_fitAction = new QAction(tr("Fit"), this);
    m_fitAction->setShortcut(QKeySequence(Qt::Key_1));
    m_fitAction->setToolTip(tr("Fit image to window (1)"));

    m_actualSizeAction = new QAction(tr("100%"), this);
    m_actualSizeAction->setShortcut(QKeySequence(Qt::Key_2));
    m_actualSizeAction->setToolTip(tr("View at 100% (2)"));

    m_zoomInAction = new QAction(tr("Zoom In"), this);
    m_zoomInAction->setShortcuts({QKeySequence::ZoomIn, QKeySequence(Qt::CTRL | Qt::Key_Equal)});

    m_zoomOutAction = new QAction(tr("Zoom Out"), this);
    m_zoomOutAction->setShortcut(QKeySequence::ZoomOut);

    auto* quitAction = new QAction(tr("Quit"), this);
    quitAction->setShortcut(QKeySequence::Quit);
    connect(quitAction, &QAction::triggered, this, &QWidget::close);

    // Shortcuts work window-wide, including for actions that have no toolbar button.
    addActions({m_openAction, m_exportAction, m_fitAction, m_actualSizeAction, m_zoomInAction, m_zoomOutAction,
                quitAction});
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
    toolbar->addAction(m_exportAction);
    auto* spacer = new QWidget(toolbar);
    spacer->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Preferred);
    toolbar->addWidget(spacer);
    toolbar->addAction(m_fitAction);
    toolbar->addAction(m_actualSizeAction);
    addToolBar(Qt::TopToolBarArea, toolbar);

    m_library = new LibraryPanel(this);
    m_view = new ImageView(this);

    // Right panel: Develop tools are added here in later phases.
    auto* rightContent = new QWidget;
    rightContent->setObjectName("sidePanel");
    auto* rightLayout = new QVBoxLayout(rightContent);
    rightLayout->setContentsMargins(0, 0, 0, 0);
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
    rightPanel->setMinimumWidth(220);
    splitter->setSizes({190, 1000, 270});
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
    connect(m_library, &LibraryPanel::photoActivated, this, &MainWindow::openPhoto);
}

void MainWindow::connectSession()
{
    connect(m_session, &PhotoSession::loadingStarted, this, [this](const QString& path) {
        m_view->beginLoading();
        m_exportAction->setEnabled(false);
        m_info->clear();
        m_sizeLabel->clear();
        setWindowTitle(QFileInfo(path).fileName() + " — Project Iris");
        statusBar()->showMessage(tr("Decoding %1…").arg(QFileInfo(path).fileName()));
    });
    connect(m_session, &PhotoSession::metadataReady, this, [this](const iris::PhotoMetadata& m) {
        m_info->setMetadata(m, QFileInfo(m_session->path()).fileName());
        m_sizeLabel->setText(QString("%1 × %2").arg(m.width).arg(m.height));
    });
    connect(m_session, &PhotoSession::previewReady, this, [this](const QImage& preview, const QSize& fullSize) {
        m_view->setPreview(preview, fullSize);
        m_exportAction->setEnabled(true);
    });
    connect(m_session, &PhotoSession::fullImageReady, this, [this](const QImage& full) {
        m_view->setFullImage(full);
        m_sizeLabel->setText(QString("%1 × %2").arg(full.width()).arg(full.height()));
        statusBar()->clearMessage();
    });
    connect(m_session, &PhotoSession::loadFailed, this, [this](const QString& path, const QString& message) {
        const QString text = tr("Cannot open %1\n%2").arg(QFileInfo(path).fileName(), message);
        m_view->setLoadFailed(text);
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

void MainWindow::openPhoto(const QString& path)
{
    const QString absolute = QFileInfo(path).absoluteFilePath();
    if (absolute == m_session->path())
        return;
    m_library->showFolderOf(absolute);
    m_session->open(absolute);
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
    QSettings().setValue("window/geometry", saveGeometry());
    event->accept();
}

} // namespace iris::ui
