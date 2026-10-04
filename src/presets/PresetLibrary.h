#pragma once

#include "presets/Preset.h"

#include <QList>
#include <QString>
#include <QStringList>

namespace iris {

struct PresetEntry {
    Preset preset;
    QString folder;   // display folder ("Built-in", "My Presets", ...)
    QString filePath; // JSON file the preset was loaded from
    bool builtIn = false;
};

// Presets on disk. Built-in presets are ordinary preset files bundled with the
// application; user presets live in <userDirectory>/<folder>/<name>.json.
class PresetLibrary {
public:
    static constexpr const char* kBuiltInFolder = "Built-in";
    static constexpr const char* kDefaultUserFolder = "My Presets";

    // builtInDirectory may be a Qt resource path (":/presets/builtin").
    PresetLibrary(QString builtInDirectory, QString userDirectory);

    // Re-reads all presets. Unreadable files are skipped and reported by warnings().
    void reload();
    const QList<PresetEntry>& presets() const { return m_presets; }
    const QStringList& warnings() const { return m_warnings; }
    QStringList userFolders() const;

    // Each returns an error message, or an empty string on success.
    QString save(const Preset& preset, const QString& folder);
    QString rename(const PresetEntry& entry, const QString& newName);
    QString move(const PresetEntry& entry, const QString& folder);
    QString remove(const PresetEntry& entry);

    static QString defaultUserDirectory();

private:
    void loadFolder(const QString& directory, const QString& folder, bool builtIn);
    QString writePreset(const Preset& preset, const QString& folder, const QString& replacing = {});

    QString m_builtInDirectory;
    QString m_userDirectory;
    QList<PresetEntry> m_presets;
    QStringList m_warnings;
};

} // namespace iris
