#include "presets/Preset.h"

#include "core/ColorScience.h"

#include <QJsonValue>

#include <algorithm>
#include <stdexcept>

namespace iris {

EditState applyPreset(const EditState& state, const Preset& preset)
{
    EditState result = state;
    BasicAdjustments& a = result.basic;
    for (const auto& [key, value] : preset.values)
        if (const AdjustmentField* field = findAdjustmentField(key))
            field->value(a) = value;
    if (preset.temperatureShift) {
        const double mired = 1e6 / a.whiteBalance.temperature - *preset.temperatureShift;
        a.whiteBalance.temperature = float(1e6 / std::max(mired, 1.0));
    }
    if (preset.tintShift)
        a.whiteBalance.tint += *preset.tintShift;
    a = sanitized(a);
    result.appliedPreset = preset.name;
    return result;
}

Preset presetFromAdjustments(const std::string& name, const BasicAdjustments& adjustments,
                             const std::vector<std::string>& keys)
{
    Preset preset;
    preset.name = name;
    BasicAdjustments copy = adjustments;
    for (const std::string& key : keys)
        if (const AdjustmentField* field = findAdjustmentField(key))
            preset.values[key] = field->value(copy);
    return preset;
}

QJsonObject presetToJson(const Preset& preset)
{
    QJsonObject adjustments;
    for (const auto& [key, value] : preset.values)
        adjustments.insert(QString::fromStdString(key), double(value));
    if (preset.temperatureShift)
        adjustments.insert("temperatureShift", double(*preset.temperatureShift));
    if (preset.tintShift)
        adjustments.insert("tintShift", double(*preset.tintShift));

    QJsonObject json;
    json.insert("version", kPresetVersion);
    json.insert("name", QString::fromStdString(preset.name));
    if (preset.order != 0)
        json.insert("order", preset.order);
    json.insert("adjustments", adjustments);
    return json;
}

Preset presetFromJson(const QJsonObject& json)
{
    if (json.value("version").toInt(0) > kPresetVersion)
        throw std::runtime_error("written by a newer version of Project Iris");
    Preset preset;
    preset.name = json.value("name").toString().trimmed().toStdString();
    if (preset.name.empty())
        throw std::runtime_error("preset has no name");
    preset.order = json.value("order").toInt(0);

    const QJsonObject adjustments = json.value("adjustments").toObject();
    for (const AdjustmentField& field : adjustmentFields()) {
        const QJsonValue value = adjustments.value(QLatin1String(field.key));
        if (value.isDouble())
            preset.values[field.key] = std::clamp(float(value.toDouble()), field.minimum, field.maximum);
    }
    if (adjustments.value("temperatureShift").isDouble())
        preset.temperatureShift = float(adjustments.value("temperatureShift").toDouble());
    if (adjustments.value("tintShift").isDouble())
        preset.tintShift = float(adjustments.value("tintShift").toDouble());
    return preset;
}

} // namespace iris
