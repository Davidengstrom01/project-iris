#include "persistence/Sidecar.h"

#include "persistence/EditStateJson.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QSaveFile>

namespace iris {

namespace {

constexpr qint64 kMaxSidecarBytes = 16 * 1024 * 1024;

std::optional<QJsonObject> readJsonObject(const QString& path, QString* error)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        *error = file.errorString();
        return std::nullopt;
    }
    if (file.size() > kMaxSidecarBytes) {
        *error = QObject::tr("File is too large");
        return std::nullopt;
    }
    QJsonParseError parseError;
    const QJsonDocument doc = QJsonDocument::fromJson(file.readAll(), &parseError);
    if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
        *error = parseError.errorString();
        return std::nullopt;
    }
    return doc.object();
}

} // namespace

QString sidecarPathFor(const QString& rawPath)
{
    const QFileInfo raw(rawPath);
    const QString primary = raw.dir().filePath(raw.completeBaseName() + ".iris.json");
    if (QFileInfo::exists(primary)) {
        QString error;
        const auto json = readJsonObject(primary, &error);
        const QString owner = json ? json->value("originalFilename").toString() : QString();
        if (!owner.isEmpty() && owner != raw.fileName())
            return raw.dir().filePath(raw.fileName() + ".iris.json");
    }
    return primary;
}

SidecarResult readSidecarFile(const QString& sidecarPath, const EditState& defaults)
{
    SidecarResult result;
    if (!QFileInfo::exists(sidecarPath))
        return result;
    const auto json = readJsonObject(sidecarPath, &result.error);
    if (!json)
        return result;
    if (json->value("version").toInt(0) > kSidecarVersion) {
        result.error = QObject::tr("It was written by a newer version of Project Iris");
        return result;
    }
    EditState edits = defaults;
    readAdjustments(json->value("adjustments").toObject(), edits.basic);
    edits.appliedPreset = json->value("preset").toString().toStdString();
    result.edits = edits;
    return result;
}

SidecarResult readSidecar(const QString& rawPath, const EditState& defaults)
{
    return readSidecarFile(sidecarPathFor(rawPath), defaults);
}

QString writeSidecar(const QString& sidecarPath, const QString& rawPath, const EditState& edits)
{
    QJsonObject json;
    json.insert("version", kSidecarVersion);
    json.insert("software", "Project Iris");
    json.insert("originalFilename", QFileInfo(rawPath).fileName());
    if (!edits.appliedPreset.empty())
        json.insert("preset", QString::fromStdString(edits.appliedPreset));
    json.insert("adjustments", adjustmentsToJson(edits.basic));

    QSaveFile file(sidecarPath);
    if (!file.open(QIODevice::WriteOnly))
        return file.errorString();
    file.write(QJsonDocument(json).toJson(QJsonDocument::Indented));
    if (!file.commit())
        return file.errorString();
    return {};
}

} // namespace iris
