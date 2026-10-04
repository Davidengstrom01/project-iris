#include "presets/PresetLibrary.h"

#include <QDir>
#include <QFileInfo>
#include <QFile>
#include <QJsonDocument>
#include <QRegularExpression>
#include <QSaveFile>
#include <QStandardPaths>

#include <algorithm>

namespace iris {

namespace {

// "Warm Film" -> "warm-film"
QString slug(const QString& name)
{
    QString s = name.toLower();
    s.replace(QRegularExpression("[^a-z0-9]+"), "-");
    s.remove(QRegularExpression("^-+|-+$"));
    return s.isEmpty() ? QStringLiteral("preset") : s;
}

// Folder names become directory names: keep them to one safe path component.
QString cleanFolder(const QString& folder)
{
    QString f = folder.trimmed();
    f.replace(QRegularExpression("[/\\\\]"), "-");
    f.remove(QRegularExpression("^[.\\s]+")); // no hidden folders, no "." or ".."
    if (f.isEmpty() || f == PresetLibrary::kBuiltInFolder)
        return PresetLibrary::kDefaultUserFolder;
    return f;
}

} // namespace

PresetLibrary::PresetLibrary(QString builtInDirectory, QString userDirectory)
    : m_builtInDirectory(std::move(builtInDirectory)), m_userDirectory(std::move(userDirectory))
{
    reload();
}

QString PresetLibrary::defaultUserDirectory()
{
    return QStandardPaths::writableLocation(QStandardPaths::AppDataLocation) + "/presets";
}

void PresetLibrary::reload()
{
    m_presets.clear();
    m_warnings.clear();
    loadFolder(m_builtInDirectory, kBuiltInFolder, true);
    const QDir user(m_userDirectory);
    for (const QString& folder : user.entryList(QDir::Dirs | QDir::NoDotAndDotDot, QDir::Name | QDir::IgnoreCase))
        loadFolder(user.filePath(folder), folder, false);

    std::stable_sort(m_presets.begin(), m_presets.end(), [](const PresetEntry& a, const PresetEntry& b) {
        if (a.builtIn != b.builtIn)
            return a.builtIn;
        if (a.folder != b.folder)
            return a.folder.compare(b.folder, Qt::CaseInsensitive) < 0;
        if (a.preset.order != b.preset.order)
            return a.preset.order < b.preset.order;
        return QString::fromStdString(a.preset.name).compare(QString::fromStdString(b.preset.name),
                                                             Qt::CaseInsensitive) < 0;
    });
}

void PresetLibrary::loadFolder(const QString& directory, const QString& folder, bool builtIn)
{
    const QDir dir(directory);
    for (const QString& name : dir.entryList({"*.json"}, QDir::Files, QDir::Name)) {
        const QString path = dir.filePath(name);
        QFile file(path);
        try {
            if (!file.open(QIODevice::ReadOnly))
                throw std::runtime_error(file.errorString().toStdString());
            QJsonParseError error;
            const QJsonDocument doc = QJsonDocument::fromJson(file.readAll(), &error);
            if (!doc.isObject())
                throw std::runtime_error(error.errorString().toStdString());
            m_presets.append({presetFromJson(doc.object()), folder, path, builtIn});
        } catch (const std::exception& e) {
            m_warnings << QString("%1: %2").arg(path, QString::fromStdString(e.what()));
        }
    }
}

QStringList PresetLibrary::userFolders() const
{
    QStringList folders = QDir(m_userDirectory).entryList(QDir::Dirs | QDir::NoDotAndDotDot, QDir::Name);
    if (!folders.contains(kDefaultUserFolder))
        folders.prepend(kDefaultUserFolder);
    return folders;
}

QString PresetLibrary::writePreset(const Preset& preset, const QString& folder, const QString& replacing)
{
    const QDir dir(QDir(m_userDirectory).filePath(cleanFolder(folder)));
    if (!dir.exists() && !QDir().mkpath(dir.path()))
        return QObject::tr("Cannot create folder %1").arg(dir.path());

    const QString base = slug(QString::fromStdString(preset.name));
    QString path = dir.filePath(base + ".json");
    for (int i = 2; QFileInfo::exists(path) && QFileInfo(path) != QFileInfo(replacing); ++i)
        path = dir.filePath(QString("%1-%2.json").arg(base).arg(i));

    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly))
        return file.errorString();
    file.write(QJsonDocument(presetToJson(preset)).toJson(QJsonDocument::Indented));
    if (!file.commit())
        return file.errorString();
    if (!replacing.isEmpty() && QFileInfo(replacing) != QFileInfo(path))
        QFile::remove(replacing);
    reload();
    return {};
}

QString PresetLibrary::save(const Preset& preset, const QString& folder)
{
    // Saving under an existing name in the same folder replaces that preset.
    const QString target = cleanFolder(folder);
    for (const PresetEntry& entry : m_presets)
        if (!entry.builtIn && entry.folder == target && entry.preset.name == preset.name)
            return writePreset(preset, target, entry.filePath);
    return writePreset(preset, target);
}

QString PresetLibrary::rename(const PresetEntry& original, const QString& newName)
{
    // Copy: `original` usually refers into m_presets, which reload() replaces.
    const PresetEntry entry = original;
    if (entry.builtIn)
        return QObject::tr("Built-in presets cannot be renamed");
    if (newName.trimmed().isEmpty())
        return QObject::tr("The name cannot be empty");
    Preset preset = entry.preset;
    preset.name = newName.trimmed().toStdString();
    return writePreset(preset, entry.folder, entry.filePath);
}

QString PresetLibrary::move(const PresetEntry& original, const QString& folder)
{
    // Copy: `original` usually refers into m_presets, which reload() replaces.
    const PresetEntry entry = original;
    if (entry.builtIn)
        return QObject::tr("Built-in presets cannot be moved");
    const QString target = cleanFolder(folder);
    if (target == entry.folder)
        return {};
    const QString error = writePreset(entry.preset, target);
    if (!error.isEmpty())
        return error;
    QFile::remove(entry.filePath);
    QDir().rmdir(QFileInfo(entry.filePath).absolutePath()); // only succeeds if now empty
    reload();
    return {};
}

QString PresetLibrary::remove(const PresetEntry& original)
{
    // Copy: `original` usually refers into m_presets, which reload() replaces.
    const PresetEntry entry = original;
    if (entry.builtIn)
        return QObject::tr("Built-in presets cannot be deleted");
    if (!QFile::remove(entry.filePath))
        return QObject::tr("Cannot delete %1").arg(entry.filePath);
    QDir().rmdir(QFileInfo(entry.filePath).absolutePath());
    reload();
    return {};
}

} // namespace iris
