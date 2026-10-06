// Engine tests that need no RAW file: colour science, pipeline stages, resampling, export.

#include "core/ColorScience.h"
#include "export/Exporter.h"
#include "rendering/Histogram.h"
#include "rendering/HslMixer.h"
#include "rendering/MaskCoverage.h"
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
                    const ToneLut curve(a, ToneCurve{});
                    float previous = -1;
                    for (int i = 0; i <= 4000; ++i) {
                        const float v = curve(i / 1000.0f);
                        QVERIFY(v >= previous - 1e-6f);
                        QVERIFY(v >= 0 && v <= 1);
                        previous = v;
                    }
                }
        // Neutral curve is the identity below white.
        const ToneLut identity{BasicAdjustments{}, ToneCurve{}};
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

    // --- Tone curve -------------------------------------------------------------

    void curveSplinePassesThroughPointsWithoutOvershoot()
    {
        const CurveSpline linear(linearCurve());
        for (float x : {0.0f, 0.3f, 0.77f, 1.0f})
            QVERIFY(std::abs(linear(x) - x) < 1e-6f);

        const CurvePoints s = sCurve();
        const CurveSpline spline(s);
        for (const CurvePoint& p : s)
            QVERIFY(std::abs(spline(p.x) - p.y) < 1e-5f);
        float previous = -1;
        for (int i = 0; i <= 1000; ++i) { // monotone data gives a monotone curve
            const float y = spline(i / 1000.0f);
            QVERIFY(y >= previous);
            previous = y;
        }

        // A sharp step must not ring above or below its points.
        const CurveSpline step({{0, 0}, {0.45f, 0.1f}, {0.55f, 0.9f}, {1, 1}});
        for (int i = 0; i <= 1000; ++i) {
            const float y = step(i / 1000.0f);
            QVERIFY(y >= 0 && y <= 1);
        }
        QVERIFY(step(0.3f) <= 0.1f + 1e-6f);
        QVERIFY(step(0.7f) >= 0.9f - 1e-6f);
    }

    void curvesAreNormalised()
    {
        QCOMPARE(normalizedCurve({{1, 1}, {0.5f, 0.6f}, {0, 0}}), CurvePoints({{0, 0}, {0.5f, 0.6f}, {1, 1}}));
        QCOMPARE(normalizedCurve({{0, 0}}), linearCurve());                         // too few points
        QCOMPARE(normalizedCurve({{0, 0}, {0.5f, 2}, {1, 1}})[1].y, 1.0f);           // clamped
        QCOMPARE(normalizedCurve({{0, 0}, {0.5f, 0.5f}, {0.502f, 0.6f}, {1, 1}}).size(), std::size_t(3));
        QVERIFY(isLinear(linearCurve()));
        QVERIFY(!isLinear(sCurve()));
    }

    void sCurveAddsContrast()
    {
        EditState edits = neutral();
        edits.toneCurve.rgb = sCurve();
        QVERIFY(renderPixel(0.03f, 0.03f, 0.03f, edits)[0] < renderPixel(0.03f, 0.03f, 0.03f)[0] - 3);
        QVERIFY(renderPixel(0.5f, 0.5f, 0.5f, edits)[0] > renderPixel(0.5f, 0.5f, 0.5f)[0] + 3);
        QCOMPARE(renderPixel(1, 1, 1, edits)[0], 255);
        QCOMPARE(renderPixel(0, 0, 0, edits)[0], 0);

        edits.toneCurve.rgb = inverseSCurve();
        QVERIFY(renderPixel(0.03f, 0.03f, 0.03f, edits)[0] > renderPixel(0.03f, 0.03f, 0.03f)[0] + 3);

        // A lifted black point fades blacks.
        edits.toneCurve.rgb = {{0, 0.1f}, {1, 1}};
        QVERIFY(renderPixel(0, 0, 0, edits)[0] > 12); // 0.1 in gamma 2.2 is ~19/255 in sRGB
    }

    void histogramCountsEveryPixel()
    {
        EncodedImage image(4, 2, 8);
        std::fill(image.data8.begin(), image.data8.end(), 100);
        image.data8[0] = 255; // one pixel with a red channel at 255
        const Histogram h = computeHistogram(image);
        QCOMPARE(h.pixels, std::uint64_t(8));
        QCOMPARE(h.green[100], 8u);
        QCOMPARE(h.red[100], 7u);
        QCOMPARE(h.red[255], 1u);
        QCOMPARE(h.luminance[100], 7u);
    }

    // --- HSL ----------------------------------------------------------------------

    void hslBandsAreInHueOrder()
    {
        const auto& c = HslMixer::bandCenters();
        for (int i = 0; i + 1 < kHslColorCount; ++i)
            QVERIFY2(c[i] < c[i + 1], qPrintable(QString("band %1: %2 >= %3").arg(i).arg(c[i]).arg(c[i + 1])));
        QVERIFY(c[0] > 15 && c[0] < 45); // red sits around 29 degrees in Oklab
    }

    void hslLeavesNeutralsAndOtherColoursAlone()
    {
        EditState edits = neutral();
        for (HslBand& band : edits.hsl.bands)
            band = {60, -80, 50};
        // Greys have no hue, so HSL must not touch them.
        QCOMPARE(renderPixel(0.18f, 0.18f, 0.18f, edits), renderPixel(0.18f, 0.18f, 0.18f));

        // Adjusting blue does not change red.
        EditState blue = neutral();
        blue.hsl[HslColor::Blue] = {50, -100, -100};
        QCOMPARE(renderPixel(0.5f, 0.05f, 0.04f, blue), renderPixel(0.5f, 0.05f, 0.04f));
    }

    void hslAdjustsItsColour()
    {
        auto saturationOf = [](const std::array<int, 3>& px) {
            const int hi = std::max({px[0], px[1], px[2]}), lo = std::min({px[0], px[1], px[2]});
            return hi == 0 ? 0.0 : double(hi - lo) / hi;
        };
        const float sky[3] = {0.105f, 0.15f, 0.335f}; // a typical sky, sRGB (100, 150, 220) at half brightness

        EditState desaturate = neutral();
        desaturate.hsl[HslColor::Blue].saturation = -100;
        QVERIFY(saturationOf(renderPixel(sky[0], sky[1], sky[2], desaturate)) < 0.12);

        EditState darker = neutral();
        darker.hsl[HslColor::Blue].luminance = -100;
        const auto before = renderPixel(sky[0], sky[1], sky[2]);
        const auto after = renderPixel(sky[0], sky[1], sky[2], darker);
        QVERIFY(after[2] < before[2] - 20);

        // Red hue +100 moves red towards orange (more green), -100 towards magenta (more blue).
        EditState warmer = neutral();
        warmer.hsl[HslColor::Red].hue = 100;
        // (A red inside the sRGB gamut, sRGB (200, 40, 40), so the result is not clipped.)
        const auto red = renderPixel(0.37f, 0.06f, 0.03f);
        QVERIFY(renderPixel(0.37f, 0.06f, 0.03f, warmer)[1] > red[1] + 15);
        warmer.hsl[HslColor::Red].hue = -100;
        QVERIFY(renderPixel(0.37f, 0.06f, 0.03f, warmer)[2] > red[2] + 15);
    }

    void hslBlendsSmoothlyAroundTheHueCircle()
    {
        // Walk around the hue circle with one range desaturated: no sudden jumps.
        HslAdjustments hsl;
        hsl[HslColor::Green].saturation = -100;
        hsl[HslColor::Yellow].luminance = 80;
        const HslMixer mixer(hsl);
        float previous[3] = {-1, -1, -1};
        for (int deg = 0; deg <= 360; ++deg) {
            const float h = deg * 3.14159265f / 180;
            float rgb[3] = {0.3f + 0.2f * std::cos(h), 0.3f + 0.2f * std::cos(h - 2.094f),
                            0.3f + 0.2f * std::cos(h + 2.094f)};
            mixer.apply(rgb);
            if (previous[0] >= 0)
                for (int c = 0; c < 3; ++c)
                    QVERIFY2(std::abs(rgb[c] - previous[c]) < 0.03f, qPrintable(QString("jump at %1 deg").arg(deg)));
            std::copy(rgb, rgb + 3, previous);
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

    // --- Masks ------------------------------------------------------------------

    void linearGradientCoverage()
    {
        Mask mask = newMask(MaskType::Linear, {});
        mask.linear = {0.5f, 0.5f, 0, 0.2f}; // horizontal, effect above, 20% transition
        const MaskCoverage coverage(mask, 100, 100);
        std::vector<float> row(100);
        auto at = [&](int y) { coverage.row(y, row.data()); return row[50]; };
        QCOMPARE(at(0), 1.0f);
        QVERIFY(std::abs(at(50) - 0.5f) < 0.05f);
        QCOMPARE(at(99), 0.0f);
        QVERIFY(at(42) > at(45) && at(45) > at(55)); // smooth transition

        mask.linear.angle = 90; // rotated counter-clockwise: effect on the left
        const MaskCoverage left(mask, 100, 100);
        left.row(50, row.data());
        QCOMPARE(row[0], 1.0f);
        QCOMPARE(row[99], 0.0f);

        mask.invert = true;
        const MaskCoverage inverted(mask, 100, 100);
        inverted.row(50, row.data());
        QCOMPARE(row[0], 0.0f);
        QCOMPARE(row[99], 1.0f);
    }

    void radialGradientCoverage()
    {
        Mask mask = newMask(MaskType::Radial, {});
        mask.radial = {0.5f, 0.5f, 0.6f, 0.2f, 0, 0.3f}; // wide, flat ellipse
        std::vector<float> row(200);
        const MaskCoverage flat(mask, 200, 200);
        flat.row(100, row.data());
        QCOMPARE(row[100], 1.0f); // centre
        QCOMPARE(row[65], 1.0f);  // inside along the long axis (60 px radius)
        QVERIFY(row[42] > 0 && row[42] < 1); // in the feathered edge
        QCOMPARE(row[5], 0.0f);   // outside
        flat.row(70, row.data()); // 30 px above the centre: beyond the 20 px half-height
        QCOMPARE(row[100], 0.0f);

        mask.radial.rotation = 90; // now tall
        const MaskCoverage tall(mask, 200, 200);
        tall.row(70, row.data());
        QCOMPARE(row[100], 1.0f);
        tall.row(100, row.data());
        QCOMPARE(row[40], 0.0f);
    }

    void brushStrokesAddSubtractAndErase()
    {
        auto stroke = [](BrushMode mode, float y, float opacity = 1) {
            BrushStroke s;
            s.mode = mode;
            s.radius = 0.05f;
            s.feather = 0;
            s.opacity = opacity;
            s.points = {{0.2f, y}, {0.8f, y}};
            return s;
        };
        std::vector<float> row(100);
        auto at = [&](const Mask& m, int x, int y) {
            MaskCoverage(m, 100, 100).row(y, row.data());
            return row[x];
        };

        Mask brush = newMask(MaskType::Brush, {});
        QCOMPARE(at(brush, 50, 50), 0.0f); // nothing painted
        brush.strokes = {stroke(BrushMode::Add, 0.5f)};
        QCOMPARE(at(brush, 50, 50), 1.0f);
        QCOMPARE(at(brush, 50, 60), 0.0f);
        QCOMPARE(at(brush, 10, 50), 0.0f);

        // Opacity, and a stroke does not build up where it overlaps itself.
        brush.strokes = {stroke(BrushMode::Add, 0.5f, 0.5f)};
        brush.strokes[0].points.push_back({0.5f, 0.5f});
        QVERIFY(std::abs(at(brush, 50, 50) - 0.5f) < 0.01f);

        // Erase removes earlier paint.
        brush.strokes = {stroke(BrushMode::Add, 0.5f), stroke(BrushMode::Erase, 0.5f)};
        QCOMPARE(at(brush, 50, 50), 0.0f);

        // Subtract removes the gradient underneath; erasing it brings the gradient back.
        Mask linear = newMask(MaskType::Linear, {});
        linear.linear = {0.5f, 1.5f, 0, 0.01f}; // effect everywhere
        QCOMPARE(at(linear, 50, 50), 1.0f);
        linear.strokes = {stroke(BrushMode::Subtract, 0.5f)};
        QCOMPARE(at(linear, 50, 50), 0.0f);
        QCOMPARE(at(linear, 50, 20), 1.0f);
        linear.strokes.push_back(stroke(BrushMode::Erase, 0.5f));
        QCOMPARE(at(linear, 50, 50), 1.0f);

        // Invert flips paint, but Subtract always removes.
        brush.strokes = {stroke(BrushMode::Add, 0.5f), stroke(BrushMode::Subtract, 0.2f)};
        brush.invert = true;
        QCOMPARE(at(brush, 50, 50), 0.0f);
        QCOMPARE(at(brush, 50, 80), 1.0f);
        QCOMPARE(at(brush, 50, 20), 0.0f);
    }

    void maskCoverageMatchesAcrossResolutions()
    {
        Mask mask = newMask(MaskType::Radial, {});
        mask.radial = {0.4f, 0.55f, 0.5f, 0.3f, 30, 0.6f};
        BrushStroke s;
        s.radius = 0.04f;
        s.points = {{0.1f, 0.1f}, {0.5f, 0.3f}, {0.9f, 0.2f}};
        mask.strokes = {s};
        const std::vector<std::uint8_t> small = renderMaskCoverage(mask, 300, 200);
        const std::vector<std::uint8_t> large = renderMaskCoverage(mask, 1500, 1000);
        for (const double fx : {0.1, 0.3, 0.45, 0.6, 0.8})
            for (const double fy : {0.15, 0.3, 0.55, 0.7}) {
                const int a = small[int(fy * 200) * 300 + int(fx * 300)];
                const int b = large[(int(fy * 200) * 5 + 2) * 1500 + int(fx * 300) * 5 + 2];
                QVERIFY2(std::abs(a - b) <= 8, qPrintable(QString("%1 vs %2 at %3,%4").arg(a).arg(b).arg(fx).arg(fy)));
            }
    }

    void localAdjustmentsOnlyInsideTheMask()
    {
        // A grey image; a linear mask covering the left half brightens it by 1 EV.
        EditState edits = neutral();
        Mask mask = newMask(MaskType::Linear, {});
        mask.linear = {0.5f, 0.5f, 90, 0.02f};
        edits.masks = {mask};
        const ImageF grey = solid(100, 20, 0.1f, 0.1f, 0.1f);
        const EncodedImage plain = render(grey, kAsShot, neutral(), {});
        const EncodedImage neutralMask = render(grey, kAsShot, edits, {});
        QCOMPARE(neutralMask.data8, plain.data8); // a mask with no adjustments changes nothing

        edits.masks[0].adjustments.exposure = 1;
        const EncodedImage out = render(grey, kAsShot, edits, {});
        auto px = [&](const EncodedImage& image, int x) { return int(image.data8[(10 * 100 + x) * 3]); };
        QCOMPARE(px(out, 90), px(plain, 90));
        QVERIFY(std::abs(px(out, 10) - renderPixel(0.2f, 0.2f, 0.2f)[0]) <= 1); // +1 EV = twice the light
        QVERIFY(px(out, 48) > px(out, 52));
    }

    void localAdjustmentsMoveInTheRightDirection()
    {
        auto withLocal = [](const char* key, float value) {
            EditState edits = neutral();
            Mask mask = newMask(MaskType::Brush, {});
            mask.invert = true; // everywhere
            for (const LocalAdjustmentField& field : localAdjustmentFields())
                if (std::string(field.key) == key)
                    field.value(mask.adjustments) = value;
            edits.masks = {mask};
            return edits;
        };
        const std::array<int, 3> grey = renderPixel(0.18f, 0.18f, 0.18f);

        const auto warm = renderPixel(0.18f, 0.18f, 0.18f, withLocal("temperature", 60));
        QVERIFY(warm[0] > grey[0] + 3 && warm[2] < grey[2] - 3);

        const auto bright = renderPixel(0.6f, 0.6f, 0.6f, withLocal("contrast", 100));
        const auto dark = renderPixel(0.03f, 0.03f, 0.03f, withLocal("contrast", 100));
        QVERIFY(bright[0] > renderPixel(0.6f, 0.6f, 0.6f)[0] + 5);
        QVERIFY(dark[0] < renderPixel(0.03f, 0.03f, 0.03f)[0] - 5);
        QVERIFY(std::abs(renderPixel(0.18f, 0.18f, 0.18f, withLocal("contrast", 100))[0] - grey[0]) <= 1);

        const auto mono = renderPixel(0.4f, 0.1f, 0.05f, withLocal("saturation", -100));
        QVERIFY(std::abs(mono[0] - mono[1]) <= 1 && std::abs(mono[1] - mono[2]) <= 1);

        QVERIFY(renderPixel(0.01f, 0.01f, 0.01f, withLocal("shadows", 100))[0] >
                renderPixel(0.01f, 0.01f, 0.01f)[0] + 15);
        QVERIFY(renderPixel(0.7f, 0.7f, 0.7f, withLocal("highlights", -100))[0] <
                renderPixel(0.7f, 0.7f, 0.7f)[0] - 15);
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
