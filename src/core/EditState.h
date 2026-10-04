#pragma once

#include <array>
#include <string>

namespace iris {

// Illuminant the photo is balanced for, in Lightroom-style units.
struct WhiteBalance {
    float temperature = 6500; // Kelvin; higher renders warmer
    float tint = 0;           // positive renders more magenta, negative more green

    bool operator==(const WhiteBalance&) const = default;
};

// Global develop adjustments. Ranges follow Lightroom conventions.
struct BasicAdjustments {
    WhiteBalance whiteBalance;
    float exposure = 0;   // EV, -5..+5
    float contrast = 0;   // -100..+100
    float highlights = 0; // -100..+100
    float shadows = 0;    // -100..+100
    float whites = 0;     // -100..+100
    float blacks = 0;     // -100..+100
    float vibrance = 0;   // -100..+100
    float saturation = 0; // -100..+100

    bool operator==(const BasicAdjustments&) const = default;
};

// Every edit applied to a photo. A plain value: copying it is cheap, and undo/redo keeps
// snapshots of it. The RAW file itself is never changed.
struct EditState {
    BasicAdjustments basic;
    std::string appliedPreset; // name of the last preset applied, if any

    bool operator==(const EditState&) const = default;
};

// The untouched state for a photo: all adjustments neutral, white balance as shot.
inline EditState defaultEditState(const WhiteBalance& asShot)
{
    EditState state;
    state.basic.whiteBalance = asShot;
    return state;
}

// Describes one adjustment so that persistence, presets and the CLI share a single list.
struct AdjustmentField {
    const char* key;   // JSON / CLI name, e.g. "exposure"
    const char* label; // display name, e.g. "Exposure"
    float minimum;
    float maximum;
    float& (*value)(BasicAdjustments&);
};

const std::array<AdjustmentField, 10>& adjustmentFields();
const AdjustmentField* findAdjustmentField(const std::string& key);

// Clamps every adjustment into its valid range (e.g. after reading a file).
BasicAdjustments sanitized(BasicAdjustments adjustments);

// Short description of what changed between two states, e.g. "Exposure", "White Balance".
std::string describeChange(const BasicAdjustments& before, const BasicAdjustments& after);

} // namespace iris
