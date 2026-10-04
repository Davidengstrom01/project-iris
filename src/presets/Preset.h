#pragma once

#include "core/EditState.h"

#include <QJsonObject>

#include <map>
#include <optional>
#include <string>
#include <vector>

namespace iris {

// A reusable set of develop settings. Only the settings a preset contains are changed
// when it is applied; everything else (and anything photo-specific, such as crop or
// masks) is left alone.
struct Preset {
    std::string name;
    int order = 0; // sort key within its folder (used by the built-in presets)

    // Absolute values by adjustment key ("exposure", "temperature", ...).
    std::map<std::string, float> values;
    // Relative white balance, so a look can warm or cool any photo:
    // temperatureShift is in mired (positive = warmer), tintShift in tint units.
    std::optional<float> temperatureShift;
    std::optional<float> tintShift;

    bool operator==(const Preset&) const = default;
};

// Applies a preset as a new edit state (the caller records it for undo).
EditState applyPreset(const EditState& state, const Preset& preset);

// Builds a preset from the current adjustments, keeping only the given keys.
Preset presetFromAdjustments(const std::string& name, const BasicAdjustments& adjustments,
                             const std::vector<std::string>& keys);

QJsonObject presetToJson(const Preset& preset);
// Throws std::runtime_error for invalid presets.
Preset presetFromJson(const QJsonObject& json);

inline constexpr int kPresetVersion = 1;

} // namespace iris
