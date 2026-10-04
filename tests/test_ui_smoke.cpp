// End-to-end test of the desktop UI on a real RAW file, following the MVP workflow:
// open, zoom, edit, save, reopen, presets, undo, before/after, export.
//
//   IRIS_TEST_RAW=/path/to/photo.ARW [IRIS_TEST_SCREENSHOTS=/some/dir] test_ui_smoke
//
// The RAW file is copied to a temporary folder (sidecars are written next to it), and
// settings and user presets go to temporary folders too.

#include "core/EditState.h"
#include "persistence/Sidecar.h"
#include "presets/PresetLibrary.h"
#include "ui/DevelopPanel.h"
#include "ui/EditDocument.h"
#include "ui/ImageView.h"
#include "ui/MainWindow.h"
#include "ui/PhotoSession.h"
#include "ui/PresetPanel.h"
#include "ui/Theme.h"
#include "ui/HistogramWidget.h"
#include "ui/ToneCurvePanel.h"

#include <QApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QSignalSpy>
#include <QStatusBar>
#include <QTemporaryDir>
#include <QTest>

using namespace iris;

namespace {

QByteArray fileHash(const QString& path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly))
        return {};
    QCryptographicHash hash(QCryptographicHash::Sha256);
    hash.addData(&file);
    return hash.result();
}

double meanLuma(const QImage& image)
{
    const QImage small = image.scaled(200, 200, Qt::KeepAspectRatio).convertToFormat(QImage::Format_RGB888);
    double sum = 0;
    for (int y = 0; y < small.height(); ++y)
        for (int x = 0; x < small.width(); ++x)
            sum += qGray(small.pixel(x, y));
    return sum / (small.width() * small.height());
}

void saveScreenshot(QWidget& widget, const QString& name)
{
    const QString dir = qEnvironmentVariable("IRIS_TEST_SCREENSHOTS");
    if (!dir.isEmpty())
        widget.grab().save(QDir(dir).filePath(name));
}

struct Window {
    ui::MainWindow window;
    ui::PhotoSession* session = nullptr;
    ui::EditDocument* document = nullptr;
    ui::ImageView* view = nullptr;
    ui::DevelopPanel* develop = nullptr;
    ui::PresetPanel* presets = nullptr;

    Window()
    {
        window.resize(1500, 950);
        window.show();
        window.activateWindow();
        session = window.findChild<ui::PhotoSession*>();
        document = window.findChild<ui::EditDocument*>();
        view = window.findChild<ui::ImageView*>();
        develop = window.findChild<ui::DevelopPanel*>();
        presets = window.findChild<ui::PresetPanel*>();
    }

    // Opens a photo and waits for the sharp preview from the full-resolution decode.
    bool open(const QString& path)
    {
        QSignalSpy previews(session, &ui::PhotoSession::previewReady);
        window.openPhoto(path);
        return QTest::qWaitFor([&] { return previews.count() >= 2 && document->isLoaded(); }, 30000);
    }

    // Moves a slider (as the develop panel would report it).
    void setAdjustments(const BasicAdjustments& adjustments)
    {
        develop->setAdjustments(adjustments, document->defaults().basic.whiteBalance);
        emit develop->adjustmentsChanged(adjustments);
    }
};

} // namespace

class UiSmokeTest : public QObject {
    Q_OBJECT

private:
    QTemporaryDir m_home;
    QString m_photoA;
    QString m_photoB;
    QByteArray m_rawHash;

private slots:
    void initTestCase()
    {
        const QString raw = qEnvironmentVariable("IRIS_TEST_RAW");
        if (raw.isEmpty())
            QSKIP("Set IRIS_TEST_RAW to a RAW file to run this test");
        qputenv("XDG_CONFIG_HOME", m_home.filePath("config").toUtf8());
        qputenv("XDG_DATA_HOME", m_home.filePath("data").toUtf8());

        const QString suffix = QFileInfo(raw).suffix();
        QDir().mkpath(m_home.filePath("photos"));
        m_photoA = m_home.filePath("photos/photo-a." + suffix);
        m_photoB = m_home.filePath("photos/photo-b." + suffix);
        QVERIFY(QFile::copy(raw, m_photoA));
        QVERIFY(QFile::copy(raw, m_photoB));
        m_rawHash = fileHash(m_photoA);
        ui::applyDarkTheme(*qApp);
    }

    void viewAndZoom()
    {
        Window w;
        QVERIFY(QTest::qWaitForWindowActive(&w.window));
        QSignalSpy fullSpy(w.session, &ui::PhotoSession::fullImageReady);
        QVERIFY(w.open(m_photoA));
        QVERIFY(w.view->isFit());
        QCOMPARE(fullSpy.count(), 0); // full resolution is only rendered when zoomed in
        saveScreenshot(w.window, "01-fit.png");

        // "2" = 100%, drag to pan, "1" = fit, double-click = zoom to 100% at the cursor.
        QTest::keyClick(&w.window, Qt::Key_2);
        QCOMPARE(w.view->zoom(), 1.0);
        QVERIFY(fullSpy.wait(10000));
        const QPoint c = w.view->rect().center();
        QTest::mousePress(w.view, Qt::LeftButton, {}, c);
        QTest::mouseMove(w.view, c + QPoint(200, 120));
        QTest::mouseRelease(w.view, Qt::LeftButton, {}, c + QPoint(200, 120));
        QTest::keyClick(&w.window, Qt::Key_1);
        QVERIFY(w.view->isFit());
        QTest::mouseDClick(w.view, Qt::LeftButton, {}, c);
        QCOMPARE(w.view->zoom(), 1.0);
        QTest::mouseDClick(w.view, Qt::LeftButton, {}, c);
        QVERIFY(w.view->isFit());
    }

    void editSaveAndRestore()
    {
        double before = 0;
        {
            Window w;
            QVERIFY(QTest::qWaitForWindowActive(&w.window));
            QVERIFY(w.open(m_photoA));
            QSignalSpy previews(w.session, &ui::PhotoSession::previewReady);
            before = meanLuma(w.view->grab().toImage());

            // A slider drag: several changes, one undo step.
            BasicAdjustments a = w.document->edits().basic;
            QElapsedTimer timer;
            timer.start();
            for (float ev : {0.25f, 0.5f, 0.75f, 1.0f}) {
                a.exposure = ev;
                w.setAdjustments(a);
            }
            QVERIFY(previews.wait(5000));
            const qint64 draftMs = timer.elapsed();
            const int draftWidth = previews.last().at(0).value<QImage>().width();
            QTRY_VERIFY_WITH_TIMEOUT(previews.last().at(0).value<QImage>().width() > draftWidth, 5000);
            qInfo("draft after %lld ms (%d px), sharp preview after %lld ms", draftMs, draftWidth, timer.elapsed());
            QVERIFY(w.document->isDirty());
            QVERIFY(w.window.windowTitle().contains("•"));
            QCOMPARE(w.document->undoLabel(), QString("Exposure"));

            a.shadows = 40;
            w.setAdjustments(a);
            QTest::keyClick(&w.window, Qt::Key_Z, Qt::ControlModifier);
            QCOMPARE(w.document->edits().basic.shadows, 0.0f);
            QCOMPARE(w.document->edits().basic.exposure, 1.0f);
            QTest::keyClick(&w.window, Qt::Key_Z, Qt::ControlModifier | Qt::ShiftModifier);
            QCOMPARE(w.document->edits().basic.shadows, 40.0f);
            QCOMPARE(w.develop->adjustments().shadows, 40.0f); // panel follows undo/redo

            QTest::keyClick(&w.window, Qt::Key_S, Qt::ControlModifier);
            QVERIFY(!w.document->isDirty());
            QVERIFY(QFile::exists(sidecarPathFor(m_photoA)));
            QTest::qWait(600);
            QVERIFY(meanLuma(w.view->grab().toImage()) > before * 1.2);
            saveScreenshot(w.window, "02-edited.png");
        }

        // "Restart": a new window restores the saved edits automatically.
        Window w;
        QVERIFY(QTest::qWaitForWindowActive(&w.window));
        QVERIFY(w.open(m_photoA));
        QCOMPARE(w.document->edits().basic.exposure, 1.0f);
        QCOMPARE(w.document->edits().basic.shadows, 40.0f);
        QCOMPARE(w.develop->adjustments().exposure, 1.0f);
        QVERIFY(!w.document->isDirty());
        QVERIFY(!w.document->canUndo());
    }

    void presetsAndUndo()
    {
        Window w;
        QVERIFY(QTest::qWaitForWindowActive(&w.window));
        QVERIFY(w.open(m_photoA));
        const EditState saved = w.document->edits();

        // Built-in preset, then undo.
        PresetLibrary library(":/presets/builtin", PresetLibrary::defaultUserDirectory());
        const PresetEntry monochrome = *std::find_if(library.presets().begin(), library.presets().end(),
                                                     [](const PresetEntry& e) { return e.preset.name == "Monochrome"; });
        emit w.presets->presetActivated(monochrome.preset);
        QCOMPARE(w.document->edits().basic.saturation, -100.0f);
        QCOMPARE(w.document->edits().basic.exposure, saved.basic.exposure); // not in the preset
        QCOMPARE(w.document->undoLabel(), QString("Preset: Monochrome"));
        QTest::qWait(700);
        saveScreenshot(w.window, "03-monochrome.png");
        QTest::keyClick(&w.window, Qt::Key_Z, Qt::ControlModifier);
        QCOMPARE(w.document->edits(), saved);

        // A custom preset with only some settings, applied to another photo.
        Preset custom = presetFromEdits("Bright & Warm", w.document->edits(), {"exposure", "shadows"});
        custom.values["exposure"] = 0.6f;
        QCOMPARE(library.save(custom, "My Presets"), QString());
        QVERIFY(QFile::exists(PresetLibrary::defaultUserDirectory() + "/My Presets/bright-warm.json"));
        QVERIFY(QDir(PresetLibrary::defaultUserDirectory()).absolutePath().startsWith(m_home.path()));

        QVERIFY(w.open(m_photoB));
        QCOMPARE(w.document->edits().basic.exposure, 0.0f);
        const WhiteBalance asShot = w.document->edits().basic.whiteBalance;
        emit w.presets->presetActivated(custom);
        QCOMPARE(w.document->edits().basic.exposure, 0.6f);
        QCOMPARE(w.document->edits().basic.shadows, 40.0f);
        QCOMPARE(w.document->edits().basic.whiteBalance, asShot);
        QTest::keyClick(&w.window, Qt::Key_Z, Qt::ControlModifier);
        QCOMPARE(w.document->edits().basic.exposure, 0.0f);
        QVERIFY(!w.document->isDirty());
    }

    void toneCurve()
    {
        Window w;
        QVERIFY(QTest::qWaitForWindowActive(&w.window));
        QVERIFY(w.open(m_photoB));
        auto* curvePanel = w.window.findChild<ui::ToneCurvePanel*>();
        auto* histogram = w.window.findChild<ui::HistogramWidget*>();
        QVERIFY(curvePanel && histogram);
        QTRY_VERIFY(histogram->hasData());
        QVERIFY(w.document->edits().toneCurve.isIdentity());

        // Picking the S-curve preset is one undoable step.
        QSignalSpy previews(w.session, &ui::PhotoSession::previewReady);
        ToneCurve s;
        s.rgb = sCurve();
        emit curvePanel->curveChosen(s);
        QCOMPARE(w.document->edits().toneCurve.rgb, sCurve());
        QCOMPARE(w.document->undoLabel(), QString("Tone Curve Preset"));
        QVERIFY(previews.wait(5000));

        // Dragging a point is interactive and merges into one step.
        ToneCurve dragged = s;
        for (float y : {0.82f, 0.84f, 0.86f}) {
            dragged.rgb[3].y = y;
            curvePanel->setCurve(dragged);
            emit curvePanel->curveEdited(dragged);
        }
        QCOMPARE(w.document->undoLabel(), QString("Tone Curve"));
        QTest::keyClick(&w.window, Qt::Key_Z, Qt::ControlModifier);
        QCOMPARE(w.document->edits().toneCurve.rgb, sCurve());
        QCOMPARE(curvePanel->curve().rgb, sCurve()); // panel follows undo
        QTest::qWait(700);
        saveScreenshot(w.window, "06-s-curve.png");
        saveScreenshot(*curvePanel, "07-curve-panel.png");

        // Saved with the photo and restored on reopen.
        QTest::keyClick(&w.window, Qt::Key_S, Qt::ControlModifier);
        Window again;
        QVERIFY(again.open(m_photoB));
        QCOMPARE(again.document->edits().toneCurve.rgb, sCurve());
    }

    void beforeAfter()
    {
        Window w;
        QVERIFY(QTest::qWaitForWindowActive(&w.window));
        QVERIFY(w.open(m_photoA)); // has saved edits (+1 EV)
        const double edited = meanLuma(w.view->grab().toImage());

        // "\" tap toggles the original on; it stays until the next tap.
        QSignalSpy beforeSpy(w.session, &ui::PhotoSession::beforePreviewReady);
        QTest::keyPress(&w.window, Qt::Key_Backslash);
        QTest::keyRelease(&w.window, Qt::Key_Backslash);
        QCOMPARE(w.view->compareMode(), ui::ImageView::CompareMode::Before);
        QVERIFY(beforeSpy.wait(5000));
        QTest::qWait(50);
        QVERIFY(meanLuma(w.view->grab().toImage()) < edited * 0.85);
        saveScreenshot(w.window, "04-before.png");
        QTest::keyClick(&w.window, Qt::Key_Backslash);
        QCOMPARE(w.view->compareMode(), ui::ImageView::CompareMode::Off);

        // Holding "\" only peeks.
        QTest::keyPress(&w.window, Qt::Key_Backslash);
        QTest::qWait(450);
        QCOMPARE(w.view->compareMode(), ui::ImageView::CompareMode::Before);
        QTest::keyRelease(&w.window, Qt::Key_Backslash);
        QCOMPARE(w.view->compareMode(), ui::ImageView::CompareMode::Off);

        // "Y" shows a split view.
        QTest::keyClick(&w.window, Qt::Key_Y);
        QCOMPARE(w.view->compareMode(), ui::ImageView::CompareMode::Split);
        QTest::qWait(100);
        saveScreenshot(w.window, "05-split.png");
        QTest::keyClick(&w.window, Qt::Key_Y);
        QCOMPARE(w.view->compareMode(), ui::ImageView::CompareMode::Off);
    }

    void exportWithEdits()
    {
        Window w;
        QVERIFY(QTest::qWaitForWindowActive(&w.window));
        QVERIFY(w.open(m_photoA));
        QTemporaryDir dir;
        const QString output = dir.filePath("export.jpg");
        QSignalSpy exportSpy(w.session, &ui::PhotoSession::exportFinished);
        w.session->exportTo(output, {});
        QVERIFY(exportSpy.wait(30000));
        QCOMPARE(exportSpy.first().at(1).toString(), QString());
        const QImage exported(output);
        QCOMPARE(exported.size(), QSize(w.session->metadata().width, w.session->metadata().height));

        // An export of the unedited photo is darker (the saved edits add +1 EV).
        QVERIFY(QFile::rename(sidecarPathFor(m_photoA), sidecarPathFor(m_photoA) + ".bak"));
        Window plain;
        QVERIFY(plain.open(m_photoA));
        const QString plainOutput = dir.filePath("plain.jpg");
        QSignalSpy plainSpy(plain.session, &ui::PhotoSession::exportFinished);
        plain.session->exportTo(plainOutput, {});
        QVERIFY(plainSpy.wait(30000));
        QVERIFY(meanLuma(exported) > meanLuma(QImage(plainOutput)) * 1.2);
    }

    void cleanupTestCase()
    {
        // The RAW file itself must never be modified.
        if (!m_photoA.isEmpty())
            QCOMPARE(fileHash(m_photoA), m_rawHash);
    }
};

QTEST_MAIN(UiSmokeTest)
#include "test_ui_smoke.moc"
