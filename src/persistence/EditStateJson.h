#pragma once

#include "core/EditState.h"

#include <QJsonObject>

#include <optional>

namespace iris {

// {"exposure": 0.5, "contrast": 10, ..., "temperature": 5600, "tint": 4}
QJsonObject adjustmentsToJson(const BasicAdjustments& adjustments);

// Sets the adjustments present in `json`; missing keys keep their current value.
// Values are clamped to their valid ranges.
void readAdjustments(const QJsonObject& json, BasicAdjustments& adjustments);

// {"points": [[0, 0], [0.25, 0.2], ..., [1, 1]]}
QJsonObject toneCurveToJson(const ToneCurve& curve);

// Reads a tone curve object; returns nothing if `json` is not a valid curve.
std::optional<ToneCurve> readToneCurve(const QJsonValue& json);

} // namespace iris
