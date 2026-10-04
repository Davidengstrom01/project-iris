// Engine tests that need no RAW file: colour transform, resampling and export.

#include "export/Exporter.h"
#include "rendering/Pipeline.h"
#include "rendering/Resample.h"

#include <QColorSpace>
#include <QImageReader>
#include <QTemporaryDir>
#include <QTest>

using namespace iris;

namespace {

ImageF solid(int w, int h, float r, float g, float b)
{
    ImageF image(w, h);
    for (std::size_t i = 0; i < image.pixels.size(); i += 3) {
        image.pixels[i] = r;
        image.pixels[i + 1] = g;
        image.pixels[i + 2] = b;
    }
    return image;
}

} // namespace

class EngineTest : public QObject {
    Q_OBJECT

private slots:
    void outputTransformMapsNeutralsToSrgb()
    {
        // Linear 0, 0.18 (mid grey) and 1.0 must land on the sRGB curve and stay neutral.
        const float levels[] = {0.0f, 0.18f, 1.0f};
        const int expected[] = {0, 118, 255};
        for (int i = 0; i < 3; ++i) {
            const EncodedImage out = render(solid(2, 2, levels[i], levels[i], levels[i]), {});
            QCOMPARE(out.width, 2);
            QVERIFY(std::abs(out.data8[0] - expected[i]) <= 1);
            QCOMPARE(out.data8[0], out.data8[1]);
            QCOMPARE(out.data8[1], out.data8[2]);
        }
    }

    void sixteenBitOutput()
    {
        const EncodedImage out = render(solid(1, 1, 1, 1, 1), {.bitsPerChannel = 16});
        QCOMPARE(out.bitsPerChannel, 16);
        QVERIFY(out.data16[0] >= 65534);
        QVERIFY(out.data8.empty());
    }

    void fitSizeNeverUpscales()
    {
        int w = 0, h = 0;
        fitSize(6000, 4000, 1500, w, h);
        QCOMPARE(w, 1500);
        QCOMPARE(h, 1000);
        fitSize(4000, 6000, 1500, w, h);
        QCOMPARE(w, 1000);
        QCOMPARE(h, 1500);
        fitSize(800, 600, 1500, w, h);
        QCOMPARE(w, 800);
        fitSize(800, 600, 0, w, h);
        QCOMPARE(w, 800);
    }

    void areaResizeAveragesBlocks()
    {
        // 4x2 image: left half 0, right half 1 -> 2x1 image of exactly 0 and 1.
        ImageF image(4, 2);
        for (int y = 0; y < 2; ++y)
            for (int x = 0; x < 4; ++x)
                for (int c = 0; c < 3; ++c)
                    image.row(y)[x * 3 + c] = x < 2 ? 0.0f : 1.0f;
        const ImageF small = resizeArea(image, 2, 1);
        QCOMPARE(small.width, 2);
        QCOMPARE(small.height, 1);
        QCOMPARE(small.row(0)[0], 0.0f);
        QCOMPARE(small.row(0)[3], 1.0f);

        // Non-integer ratio preserves the mean of a constant image.
        const ImageF resized = resizeArea(solid(7, 5, 0.25f, 0.5f, 0.75f), 3, 2);
        for (std::size_t i = 0; i < resized.pixels.size(); i += 3) {
            QVERIFY(std::abs(resized.pixels[i] - 0.25f) < 1e-5f);
            QVERIFY(std::abs(resized.pixels[i + 2] - 0.75f) < 1e-5f);
        }
    }

    void exportJpegWithIccAndResize()
    {
        QTemporaryDir dir;
        const QString path = dir.filePath("out.jpg");
        exportImage(solid(300, 200, 0.18f, 0.18f, 0.18f), {.format = ExportFormat::Jpeg, .longEdge = 150},
                    path.toStdString());
        QImageReader reader(path);
        const QImage image = reader.read();
        QCOMPARE(image.size(), QSize(150, 100));
        QVERIFY(image.colorSpace().isValid());
        QCOMPARE(image.colorSpace().primaries(), QColorSpace::Primaries::SRgb);
    }

    void exportSixteenBitPngAndTiff()
    {
        QTemporaryDir dir;
        for (const auto format : {ExportFormat::Png, ExportFormat::Tiff}) {
            const QString path = dir.filePath(QString("out.") + fileExtension(format));
            exportImage(solid(64, 32, 0.5f, 0.25f, 0.1f), {.format = format, .bitsPerChannel = 16}, path.toStdString());
            const QImage image(path);
            QCOMPARE(image.size(), QSize(64, 32));
            QCOMPARE(image.depth(), 64); // 16 bits x 4 channels
        }
    }

    void failedExportLeavesNoFile()
    {
        QTemporaryDir dir;
        const QString path = dir.filePath("missing-folder/out.jpg");
        QVERIFY_THROWS_EXCEPTION(std::runtime_error, exportImage(solid(4, 4, 1, 1, 1), {}, path.toStdString()));
        QVERIFY(!QFile::exists(path));
    }
};

QTEST_GUILESS_MAIN(EngineTest)
#include "test_engine.moc"
