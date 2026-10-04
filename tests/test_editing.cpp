// Editing-state tests: undo history, sidecars and presets. No RAW file needed.

#include "core/EditHistory.h"
#include "persistence/Sidecar.h"
#include "presets/Preset.h"
#include "presets/PresetLibrary.h"

#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QTemporaryDir>
#include <QTest>

using namespace iris;

namespace {

const WhiteBalance kAsShot{5500, 10};

EditState withExposure(float ev)
{
    EditState s = defaultEditState(kAsShot);
    s.basic.exposure = ev;
    return s;
}

void writeFile(const QString& path, const QByteArray& content)
{
    QFile f(path);
    QVERIFY(f.open(QIODevice::WriteOnly));
    f.write(content);
}

} // namespace

class EditingTest : public QObject {
    Q_OBJECT

private slots:
    // --- Undo history -----------------------------------------------------------

    void undoRedo()
    {
        EditHistory h(withExposure(0));
        QVERIFY(!h.canUndo());
        h.record(withExposure(1), "Exposure");
        h.record(withExposure(2), "Preset");
        QCOMPARE(h.undoLabel(), std::string("Preset"));
        QCOMPARE(h.undo().basic.exposure, 1.0f);
        QCOMPARE(h.undo().basic.exposure, 0.0f);
        QVERIFY(!h.canUndo());
        QCOMPARE(h.redoLabel(), std::string("Exposure"));
        QCOMPARE(h.redo().basic.exposure, 1.0f);

        // A new edit after undo discards the redo steps.
        h.record(withExposure(5), "Exposure");
        QVERIFY(!h.canRedo());
        QCOMPARE(h.undo().basic.exposure, 1.0f);
    }

    void sliderDragIsOneStep()
    {
        EditHistory h(withExposure(0));
        for (float ev : {0.1f, 0.2f, 0.3f, 0.4f})
            h.record(withExposure(ev), "Exposure", true);
        QCOMPARE(h.current().basic.exposure, 0.4f);
        QCOMPARE(h.undo().basic.exposure, 0.0f);
        QVERIFY(!h.canUndo());

        // Dragging back to the start leaves no step behind.
        EditHistory back(withExposure(0));
        back.record(withExposure(0.5f), "Exposure", true);
        back.record(withExposure(0), "Exposure", true);
        QVERIFY(!back.canUndo());

        // Different controls are separate steps.
        EditHistory two(withExposure(0));
        two.record(withExposure(1), "Exposure", true);
        EditState contrast = two.current();
        contrast.basic.contrast = 20;
        two.record(contrast, "Contrast", true);
        two.undo();
        QCOMPARE(two.current().basic.exposure, 1.0f);
        QCOMPARE(two.current().basic.contrast, 0.0f);
    }

    void describesChanges()
    {
        BasicAdjustments a, b;
        b.exposure = 1;
        QCOMPARE(describeChange(a, b), std::string("Exposure"));
        b = a;
        b.whiteBalance.tint = 5;
        QCOMPARE(describeChange(a, b), std::string("White Balance"));
        b.contrast = 3;
        QCOMPARE(describeChange(a, b), std::string("Basic"));
    }

    // --- Sidecars ---------------------------------------------------------------

    void sidecarRoundTrip()
    {
        QTemporaryDir dir;
        const QString raw = dir.filePath("photo.ARW");
        writeFile(raw, "raw");
        QCOMPARE(sidecarPathFor(raw), dir.filePath("photo.iris.json"));

        EditState edits = defaultEditState(kAsShot);
        edits.basic.exposure = 0.5f;
        edits.basic.contrast = 10;
        edits.basic.whiteBalance = {5600, 4};
        edits.appliedPreset = "Warm Film";
        edits.toneCurve.rgb = {{0, 0}, {0.25f, 0.20f}, {0.5f, 0.52f}, {0.75f, 0.82f}, {1, 1}};
        QCOMPARE(writeSidecar(sidecarPathFor(raw), raw, edits), QString());

        const SidecarResult read = readSidecar(raw, defaultEditState(kAsShot));
        QVERIFY(read.error.isEmpty());
        QVERIFY(read.edits.has_value());
        QCOMPARE(*read.edits, edits);

        // The documented format.
        QFile f(sidecarPathFor(raw));
        QVERIFY(f.open(QIODevice::ReadOnly));
        const QJsonObject json = QJsonDocument::fromJson(f.readAll()).object();
        QCOMPARE(json.value("version").toInt(), 1);
        QCOMPARE(json.value("originalFilename").toString(), QString("photo.ARW"));
        QCOMPARE(json.value("adjustments").toObject().value("exposure").toDouble(), 0.5);
        QCOMPARE(json.value("adjustments").toObject().value("temperature").toDouble(), 5600.0);
        const QJsonArray points = json.value("toneCurve").toObject().value("points").toArray();
        QCOMPARE(points.size(), 5);
        QCOMPARE(points[2].toArray()[1].toDouble(), double(0.52f));
    }

    void sidecarMissingValuesUseDefaults()
    {
        QTemporaryDir dir;
        const QString raw = dir.filePath("a.NEF");
        writeFile(dir.filePath("a.iris.json"), R"({"version": 1, "adjustments": {"exposure": 9, "shadows": 25}})");
        const SidecarResult read = readSidecar(raw, defaultEditState(kAsShot));
        QVERIFY(read.edits.has_value());
        QCOMPARE(read.edits->basic.exposure, 5.0f); // clamped
        QCOMPARE(read.edits->basic.shadows, 25.0f);
        QCOMPARE(read.edits->basic.whiteBalance, kAsShot);
        QVERIFY(read.edits->toneCurve.isIdentity());
    }

    void unreadableSidecarsAreReported()
    {
        QTemporaryDir dir;
        const QString raw = dir.filePath("b.CR2");
        QVERIFY(!readSidecar(raw, {}).edits.has_value()); // no sidecar: no error
        QVERIFY(readSidecar(raw, {}).error.isEmpty());

        writeFile(dir.filePath("b.iris.json"), "{ not json");
        SidecarResult broken = readSidecar(raw, {});
        QVERIFY(!broken.edits.has_value());
        QVERIFY(!broken.error.isEmpty());

        writeFile(dir.filePath("b.iris.json"), R"({"version": 99, "adjustments": {}})");
        QVERIFY(!readSidecar(raw, {}).error.isEmpty());
    }

    void sidecarNameCollision()
    {
        // photo.ARW and photo.CR2 in one folder must not share a sidecar.
        QTemporaryDir dir;
        const QString arw = dir.filePath("photo.ARW");
        const QString cr2 = dir.filePath("photo.CR2");
        QCOMPARE(writeSidecar(sidecarPathFor(arw), arw, withExposure(1)), QString());
        QCOMPARE(sidecarPathFor(cr2), dir.filePath("photo.CR2.iris.json"));
        QCOMPARE(writeSidecar(sidecarPathFor(cr2), cr2, withExposure(-1)), QString());
        QCOMPARE(readSidecar(arw, {}).edits->basic.exposure, 1.0f);
        QCOMPARE(readSidecar(cr2, {}).edits->basic.exposure, -1.0f);
    }

    // --- Presets ----------------------------------------------------------------

    void presetChangesOnlyItsSettings()
    {
        EditState state = defaultEditState(kAsShot);
        state.basic.exposure = 0.7f;
        state.basic.shadows = 30;
        state.basic.whiteBalance = {4800, -3};

        Preset preset;
        preset.name = "Punchy";
        preset.values = {{"contrast", 25}, {"vibrance", 30}};
        const EditState applied = applyPreset(state, preset);
        QCOMPARE(applied.basic.contrast, 25.0f);
        QCOMPARE(applied.basic.vibrance, 30.0f);
        QCOMPARE(applied.basic.exposure, 0.7f);
        QCOMPARE(applied.basic.shadows, 30.0f);
        QCOMPARE(applied.basic.whiteBalance, state.basic.whiteBalance);
        QCOMPARE(applied.appliedPreset, std::string("Punchy"));
    }

    void relativeWhiteBalanceShift()
    {
        Preset warm;
        warm.name = "Warm";
        warm.temperatureShift = 20; // mired
        warm.tintShift = 5;
        const EditState applied = applyPreset(defaultEditState({5000, 0}), warm);
        // 1e6/5000 = 200 mired -> 180 mired = 5556 K
        QVERIFY(std::abs(applied.basic.whiteBalance.temperature - 5555.6f) < 1);
        QCOMPARE(applied.basic.whiteBalance.tint, 5.0f);
    }

    void selectivePresetAndJson()
    {
        EditState edits;
        edits.basic.exposure = 0.3f;
        edits.basic.contrast = 12;
        edits.basic.whiteBalance = {6100, 7};
        edits.toneCurve.rgb = sCurve();
        const Preset preset = presetFromEdits("Mine", edits, {"contrast", "temperature", "tint"});
        QCOMPARE(preset.values.size(), std::size_t(3));
        QVERIFY(!preset.values.count("exposure"));
        QVERIFY(!preset.toneCurve.has_value());

        const Preset withCurve = presetFromEdits("Curve", edits, {kToneCurveKey});
        QVERIFY(withCurve.values.empty());
        QCOMPARE(withCurve.toneCurve->rgb, sCurve());
        QCOMPARE(presetFromJson(presetToJson(withCurve)), withCurve);
        // Applying a preset without a curve keeps the photo's curve; one with a curve sets it.
        EditState photo = defaultEditState(kAsShot);
        photo.toneCurve.rgb = inverseSCurve();
        QCOMPARE(applyPreset(photo, preset).toneCurve.rgb, inverseSCurve());
        QCOMPARE(applyPreset(photo, withCurve).toneCurve.rgb, sCurve());

        const Preset back = presetFromJson(presetToJson(preset));
        QCOMPARE(back, preset);
        QVERIFY_THROWS_EXCEPTION(std::runtime_error, presetFromJson(QJsonObject{{"version", 1}}));
    }

    void builtInPresetsLoad()
    {
        QTemporaryDir user;
        PresetLibrary library(":/presets/builtin", user.path());
        QVERIFY2(library.warnings().isEmpty(), qPrintable(library.warnings().join('\n')));
        QStringList names;
        for (const PresetEntry& e : library.presets()) {
            QVERIFY(e.builtIn);
            names << QString::fromStdString(e.preset.name);
        }
        QCOMPARE(names, QStringList({"Neutral", "Natural", "Soft Contrast", "High Contrast", "Warm Film", "Cool Film",
                                     "Golden Hour", "Muted", "Monochrome"}));
    }

    void userPresetLifecycle()
    {
        QTemporaryDir user;
        PresetLibrary library(":/presets/builtin", user.path());
        const int builtIns = library.presets().size();

        Preset p;
        p.name = "Warm Film";
        p.values = {{"contrast", 8}};
        QCOMPARE(library.save(p, "My Presets"), QString());
        QVERIFY(QFile::exists(user.filePath("My Presets/warm-film.json")));
        QCOMPARE(library.presets().size(), builtIns + 1);

        // Saving the same name again replaces it.
        p.values = {{"contrast", 9}};
        QCOMPARE(library.save(p, "My Presets"), QString());
        QCOMPARE(library.presets().size(), builtIns + 1);

        auto findUser = [&]() -> PresetEntry {
            for (const PresetEntry& e : library.presets())
                if (!e.builtIn)
                    return e;
            return {};
        };
        QCOMPARE(findUser().preset.values.at("contrast"), 9.0f);

        QCOMPARE(library.rename(findUser(), "Golden"), QString());
        QCOMPARE(findUser().preset.name, std::string("Golden"));
        QVERIFY(QFile::exists(user.filePath("My Presets/golden.json")));
        QVERIFY(!QFile::exists(user.filePath("My Presets/warm-film.json")));

        QCOMPARE(library.move(findUser(), "Landscapes"), QString());
        QCOMPARE(findUser().folder, QString("Landscapes"));
        QVERIFY(library.userFolders().contains("Landscapes"));

        // Folder names cannot escape the presets directory.
        QCOMPARE(library.move(findUser(), "../evil"), QString());
        QVERIFY(QFileInfo(findUser().filePath).absolutePath().startsWith(QDir(user.path()).absolutePath()));

        QCOMPARE(library.remove(findUser()), QString());
        QCOMPARE(library.presets().size(), builtIns);
        QVERIFY(!library.remove(library.presets().first()).isEmpty()); // built-ins are read-only
    }
};

QTEST_GUILESS_MAIN(EditingTest)
#include "test_editing.moc"
