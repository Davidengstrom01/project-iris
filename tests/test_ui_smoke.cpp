// End-to-end smoke test of the desktop UI on a real RAW file.
//
//   IRIS_TEST_RAW=/path/to/photo.ARW [IRIS_TEST_SCREENSHOTS=/some/dir] test_ui_smoke

#include "ui/ImageView.h"
#include "ui/MainWindow.h"
#include "ui/PhotoSession.h"
#include "ui/Theme.h"

#include <QApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QSignalSpy>
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

void saveScreenshot(QWidget& widget, const QString& name)
{
    const QString dir = qEnvironmentVariable("IRIS_TEST_SCREENSHOTS");
    if (!dir.isEmpty())
        widget.grab().save(QDir(dir).filePath(name));
}

} // namespace

class UiSmokeTest : public QObject {
    Q_OBJECT

private slots:
    void openViewZoomExport()
    {
        const QString raw = qEnvironmentVariable("IRIS_TEST_RAW");
        if (raw.isEmpty())
            QSKIP("Set IRIS_TEST_RAW to a RAW file to run this test");
        const QByteArray hashBefore = fileHash(raw);
        QVERIFY(!hashBefore.isEmpty());

        ui::applyDarkTheme(*qApp);
        ui::MainWindow window;
        window.resize(1500, 950);
        window.show();
        window.activateWindow();
        QVERIFY(QTest::qWaitForWindowActive(&window));

        auto* session = window.findChild<ui::PhotoSession*>();
        auto* view = window.findChild<ui::ImageView*>();
        QVERIFY(session && view);

        QSignalSpy previewSpy(session, &ui::PhotoSession::previewReady);
        QSignalSpy fullSpy(session, &ui::PhotoSession::fullImageReady);
        window.openPhoto(raw);
        QVERIFY(fullSpy.wait(30000));
        QVERIFY(previewSpy.count() >= 1);
        QVERIFY(session->isLoaded());
        QVERIFY(view->isFit());
        QVERIFY(view->zoom() < 1.0);
        saveScreenshot(window, "01-fit.png");

        // "2" = 100%, drag to pan, "1" = fit, double-click = zoom to 100% at the cursor.
        QTest::keyClick(&window, Qt::Key_2);
        QCOMPARE(view->zoom(), 1.0);
        QVERIFY(!view->isFit());
        const QPoint c = view->rect().center();
        QTest::mousePress(view, Qt::LeftButton, {}, c);
        QTest::mouseMove(view, c + QPoint(200, 120));
        QTest::mouseRelease(view, Qt::LeftButton, {}, c + QPoint(200, 120));
        saveScreenshot(window, "02-100-percent.png");

        QTest::keyClick(&window, Qt::Key_1);
        QVERIFY(view->isFit());
        QTest::mouseDClick(view, Qt::LeftButton, {}, c);
        QCOMPARE(view->zoom(), 1.0);
        QTest::mouseDClick(view, Qt::LeftButton, {}, c);
        QVERIFY(view->isFit());

        // Full-resolution JPEG export.
        QTemporaryDir dir;
        const QString output = dir.filePath("export.jpg");
        QSignalSpy exportSpy(session, &ui::PhotoSession::exportFinished);
        session->exportTo(output, {});
        QVERIFY(exportSpy.wait(30000));
        QCOMPARE(exportSpy.first().at(1).toString(), QString());
        const QImage exported(output);
        QCOMPARE(exported.size(), QSize(session->metadata().width, session->metadata().height));

        // The original RAW must never be modified.
        QCOMPARE(fileHash(raw), hashBefore);
    }
};

QTEST_MAIN(UiSmokeTest)
#include "test_ui_smoke.moc"
