// Engine tests that need no RAW file: colour science, pipeline stages, resampling, export.

#include "core/ColorScience.h"
#include "export/Exporter.h"
#include "rendering/Pipeline.h"
#include "rendering/Resample.h"
#include "rendering/Tone.h"
#include "rendering/WhiteBalanceTools.h"

#include <QColorSpace>
#include <QImageReader>
#include <QTemporaryDir>
#include <QTest>

using namespace iris;

namespace {

const WhiteBalance kAsShot{5500, 10};

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

EditState neutral()
{
    return defaultEditState(kAsShot);
}

// Renders one pixel of the given colour and returns its 8-bit sRGB value.
std::array<int, 3> renderPixel(float r, float g, float b, const EditState& edits = neutral())
{
    const EncodedImage out = render(solid(1, 1, r, g, b), kAsShot, edits, {});
    return {out.data8[0], out.data8[1], out.data8[2]};
}

} // namespace

class EngineTest : public QObject {
    Q_OBJECT

private slots:
    // --- Output transform -------------------------------------------------------

    void outputTransformMapsNeutralsToSrgb()
    {
        // Linear 0, 0.18 (mid grey) and 1.0 must land on the sRGB curve and stay neutral.
        const float levels[] = {0.0f, 0.18f, 1.0f};
        const int expected[] = {0, 118, 255};
        for (int i = 0; i < 3; ++i) {
            const auto px = renderPixel(levels[i], levels[i], levels[i]);
            QVERIFY(std::abs(px[0] - expected[i]) <= 1);
            QCOMPARE(px[0], px[1]);
            QCOMPARE(px[1], px[2]);
        }
    }

    void sixteenBitOutput()
    {
        const EncodedImage out = render(solid(1, 1, 1, 1, 1), kAsShot, neutral(), {.bitsPerChannel = 16});
        QCOMPARE(out.bitsPerChannel, 16);
        QVERIFY(out.data16[0] >= 65534);
        QVERIFY(out.data8.empty());
    }

    // --- White balance ----------------------------------------------------------

    void whiteBalanceRoundTrips()
    {
        for (float t : {2500.0f, 3200.0f, 5000.0f, 6500.0f, 10000.0f}) {
            for (float tint : {-60.0f, 0.0f, 35.0f}) {
                const WhiteBalance wb = whiteBalanceFromWhitePoint(whitePoint({t, tint}));
                QVERIFY2(std::abs(wb.temperature - t) < 0.5f, qPrintable(QString::number(wb.temperature)));
                QVERIFY(std::abs(wb.tint - tint) < 0.05f);
            }
        }
        const WhiteBalance d65 = whiteBalanceFromWhitePoint(kD65);
        QVERIFY(std::abs(d65.temperature - 6504) < 10);
    }

    void asShotWhiteBalanceIsIdentity()
    {
        const Mat3 m = whiteBalanceMatrix(kAsShot, kAsShot);
        for (int i = 0; i < 9; ++i)
            QVERIFY(std::abs(m[i] - (i % 4 == 0 ? 1.0 : 0.0)) < 1e-9);
    }

    void higherTemperatureRendersWarmer()
    {
        EditState warm = neutral();
        warm.basic.whiteBalance.temperature = 8000;
        const auto px = renderPixel(0.18f, 0.18f, 0.18f, warm);
        QVERIFY(px[0] > px[2] + 10);

        EditState cool = neutral();
        cool.basic.whiteBalance.temperature = 3500;
        const auto cp = renderPixel(0.18f, 0.18f, 0.18f, cool);
        QVERIFY(cp[2] > cp[0] + 10);

        EditState magenta = neutral();
        magenta.basic.whiteBalance.tint = 60;
        const auto mp = renderPixel(0.18f, 0.18f, 0.18f, magenta);
        QVERIFY(mp[1] < mp[0] && mp[1] < mp[2]);
    }

    void eyedropperNeutralisesTheSample()
    {
        const ImageF image = solid(40, 30, 0.22f, 0.18f, 0.12f); // warm cast
        const auto wb = sampleWhiteBalance(image, 0.5, 0.5, kAsShot);
        QVERIFY(wb.has_value());
        EditState edits = neutral();
        edits.basic.whiteBalance = *wb;
        const auto px = renderPixel(0.22f, 0.18f, 0.12f, edits);
        QVERIFY2(std::abs(px[0] - px[1]) <= 1 && std::abs(px[1] - px[2]) <= 1,
                 qPrintable(QString("%1 %2 %3").arg(px[0]).arg(px[1]).arg(px[2])));
        QVERIFY(wb->temperature < kAsShot.temperature); // warm cast -> lower temperature

        // Clipped samples are rejected.
        QVERIFY(!sampleWhiteBalance(solid(10, 10, 1, 1, 1), 0.5, 0.5, kAsShot).has_value());
    }

    void autoWhiteBalanceOfNeutralImageIsAsShot()
    {
        const WhiteBalance wb = estimateWhiteBalance(solid(20, 20, 0.3f, 0.3f, 0.3f), kAsShot);
        QVERIFY(std::abs(wb.temperature - kAsShot.temperature) < 5);
        QVERIFY(std::abs(wb.tint - kAsShot.tint) < 0.5f);
    }

    // --- Tone -------------------------------------------------------------------

    void exposureIsLinearGain()
    {
        EditState plusOne = neutral();
        plusOne.basic.exposure = 1;
        QCOMPARE(renderPixel(0.09f, 0.09f, 0.09f, plusOne), renderPixel(0.18f, 0.18f, 0.18f));
    }

    void toneCurveIsMonotonicAndBounded()
    {
        for (float contrast : {-100.0f, 0.0f, 100.0f})
            for (float whites : {-100.0f, 0.0f, 100.0f})
                for (float blacks : {-100.0f, 0.0f, 100.0f}) {
                    BasicAdjustments a;
                    a.contrast = contrast;
                    a.whites = whites;
                    a.blacks = blacks;
                    const ToneCurve curve(a);
                    float previous = -1;
                    for (int i = 0; i <= 4000; ++i) {
                        const float v = curve(i / 1000.0f);
                        QVERIFY(v >= previous - 1e-6f);
                        QVERIFY(v >= 0 && v <= 1);
                        previous = v;
                    }
                }
        // Neutral curve is the identity below white.
        const ToneCurve identity{BasicAdjustments{}};
        QVERIFY(std::abs(identity(0.18f) - 0.18f) < 1e-5f);
        QCOMPARE(identity(3.0f), 1.0f);
    }

    void contrastKeepsMiddleGrey()
    {
        EditState edits = neutral();
        edits.basic.contrast = 80;
        const auto grey = renderPixel(0.18f, 0.18f, 0.18f, edits);
        QVERIFY(std::abs(grey[0] - 118) <= 1);
        QVERIFY(renderPixel(0.05f, 0.05f, 0.05f, edits)[0] < renderPixel(0.05f, 0.05f, 0.05f)[0]);
        QVERIFY(renderPixel(0.6f, 0.6f, 0.6f, edits)[0] > renderPixel(0.6f, 0.6f, 0.6f)[0]);
    }

    void whitesRecoverValuesAboveWhite()
    {
        EditState edits = neutral();
        edits.basic.exposure = 1; // 0.8 -> 1.6, clipped by default
        QCOMPARE(renderPixel(0.8f, 0.8f, 0.8f, edits)[0], 255);
        edits.basic.whites = -100;
        QVERIFY(renderPixel(0.8f, 0.8f, 0.8f, edits)[0] < 250);
    }

    void shadowsAndHighlightsActOnTheirRegions()
    {
        EditState shadows = neutral();
        shadows.basic.shadows = 100;
        const int darkBefore = renderPixel(0.01f, 0.01f, 0.01f)[0];
        QVERIFY(renderPixel(0.01f, 0.01f, 0.01f, shadows)[0] > darkBefore + 15);
        QVERIFY(std::abs(renderPixel(0.7f, 0.7f, 0.7f, shadows)[0] - renderPixel(0.7f, 0.7f, 0.7f)[0]) <= 1);

        EditState highlights = neutral();
        highlights.basic.highlights = -100;
        QVERIFY(renderPixel(0.7f, 0.7f, 0.7f, highlights)[0] < renderPixel(0.7f, 0.7f, 0.7f)[0] - 15);
        QVERIFY(std::abs(renderPixel(0.01f, 0.01f, 0.01f, highlights)[0] - darkBefore) <= 1);
    }

    void localToneMatchesAcrossResolutions()
    {
        // Preview and export must agree: a left-dark / right-bright image rendered at two
        // resolutions gives the same values at corresponding points.
        auto makeImage = [](int w, int h) {
            ImageF image(w, h);
            for (int y = 0; y < h; ++y)
                for (int x = 0; x < w; ++x)
                    for (int c = 0; c < 3; ++c)
                        image.row(y)[x * 3 + c] = x < w / 2 ? 0.02f : 0.6f;
            return image;
        };
        EditState edits = neutral();
        edits.basic.shadows = 70;
        edits.basic.highlights = -70;
        const EncodedImage small = render(makeImage(400, 300), kAsShot, edits, {});
        const EncodedImage large = render(makeImage(2000, 1500), kAsShot, edits, {});
        for (const double fx : {0.1, 0.4, 0.6, 0.9}) {
            const int a = small.data8[(150 * 400 + int(fx * 400)) * 3];
            const int b = large.data8[(750 * 2000 + int(fx * 2000)) * 3];
            QVERIFY2(std::abs(a - b) <= 2, qPrintable(QString("%1 vs %2 at %3").arg(a).arg(b).arg(fx)));
        }
    }

    // --- Presence ---------------------------------------------------------------

    void saturationMinus100IsMonochrome()
    {
        EditState edits = neutral();
        edits.basic.saturation = -100;
        const auto px = renderPixel(0.4f, 0.1f, 0.05f, edits);
        QVERIFY(std::abs(px[0] - px[1]) <= 1 && std::abs(px[1] - px[2]) <= 1);
    }

    void vibranceFavoursMutedColours()
    {
        auto saturationOf = [](const std::array<int, 3>& px) {
            const int hi = std::max({px[0], px[1], px[2]}), lo = std::min({px[0], px[1], px[2]});
            return double(hi - lo) / hi;
        };
        EditState edits = neutral();
        edits.basic.vibrance = 100;
        // Muted blue-green vs. strongly saturated blue-green (away from skin hues).
        const double mutedGain = saturationOf(renderPixel(0.15f, 0.2f, 0.22f, edits)) -
                                 saturationOf(renderPixel(0.15f, 0.2f, 0.22f));
        const double vividGain = saturationOf(renderPixel(0.02f, 0.2f, 0.3f, edits)) -
                                 saturationOf(renderPixel(0.02f, 0.2f, 0.3f));
        QVERIFY(mutedGain > 0.05);
        QVERIFY(mutedGain > vividGain);
    }

    // --- Resampling -------------------------------------------------------------

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

        const ImageF resized = resizeArea(solid(7, 5, 0.25f, 0.5f, 0.75f), 3, 2);
        for (std::size_t i = 0; i < resized.pixels.size(); i += 3) {
            QVERIFY(std::abs(resized.pixels[i] - 0.25f) < 1e-5f);
            QVERIFY(std::abs(resized.pixels[i + 2] - 0.75f) < 1e-5f);
        }
    }

    // --- Export -----------------------------------------------------------------

    void exportJpegWithIccAndResize()
    {
        QTemporaryDir dir;
        const QString path = dir.filePath("out.jpg");
        exportImage(solid(300, 200, 0.18f, 0.18f, 0.18f), kAsShot, neutral(),
                    {.format = ExportFormat::Jpeg, .longEdge = 150}, path.toStdString());
        QImageReader reader(path);
        const QImage image = reader.read();
        QCOMPARE(image.size(), QSize(150, 100));
        QVERIFY(image.colorSpace().isValid());
        QCOMPARE(image.colorSpace().primaries(), QColorSpace::Primaries::SRgb);
    }

    void exportAppliesEdits()
    {
        QTemporaryDir dir;
        const QString path = dir.filePath("out.png");
        EditState edits = neutral();
        edits.basic.exposure = 1;
        exportImage(solid(8, 8, 0.09f, 0.09f, 0.09f), kAsShot, edits, {.format = ExportFormat::Png},
                    path.toStdString());
        QVERIFY(std::abs(qRed(QImage(path).pixel(4, 4)) - 118) <= 1);
    }

    void exportSixteenBitPngAndTiff()
    {
        QTemporaryDir dir;
        for (const auto format : {ExportFormat::Png, ExportFormat::Tiff}) {
            const QString path = dir.filePath(QString("out.") + fileExtension(format));
            exportImage(solid(64, 32, 0.5f, 0.25f, 0.1f), kAsShot, neutral(), {.format = format, .bitsPerChannel = 16},
                        path.toStdString());
            const QImage image(path);
            QCOMPARE(image.size(), QSize(64, 32));
            QCOMPARE(image.depth(), 64); // 16 bits x 4 channels
        }
    }

    void failedExportLeavesNoFile()
    {
        QTemporaryDir dir;
        const QString path = dir.filePath("missing-folder/out.jpg");
        QVERIFY_THROWS_EXCEPTION(std::runtime_error,
                                 exportImage(solid(4, 4, 1, 1, 1), kAsShot, neutral(), {}, path.toStdString()));
        QVERIFY(!QFile::exists(path));
    }
};

QTEST_GUILESS_MAIN(EngineTest)
#include "test_engine.moc"
