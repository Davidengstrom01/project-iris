#pragma once

#include "core/EditState.h"

#include <QJsonArray>
#include <QJsonObject>

#include <optional>
#include <vector>

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

// {"red": {"hue": 0, "saturation": -20, "luminance": 0}, ...}; neutral ranges are omitted.
QJsonObject hslToJson(const HslAdjustments& hsl);

// Reads HSL adjustments; missing ranges and values are 0.
HslAdjustments readHsl(const QJsonValue& json);

// [{"type": "radial", "name": "Radial 1", "invert": false, "radial": {...},
//   "strokes": [{"mode": "add", "radius": 0.05, ..., "points": [[0.1, 0.2], ...]}],
//   "adjustments": {"exposure": 0.5, ...}}, ...]
// Only the shape of the mask's own type is written.
QJsonArray masksToJson(const std::vector<Mask>& masks);

// Reads masks; unknown mask types are skipped. Values are clamped to their valid ranges.
std::vector<Mask> readMasks(const QJsonValue& json);

} // namespace iris
