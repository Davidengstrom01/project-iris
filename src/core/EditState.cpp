#include "core/EditState.h"

#include "core/ColorScience.h"

#include <algorithm>
#include <cmath>

namespace iris {

const std::array<AdjustmentField, 10>& adjustmentFields()
{
    static const std::array<AdjustmentField, 10> fields = {{
        {"exposure", "Exposure", -5, 5, [](BasicAdjustments& a) -> float& { return a.exposure; }},
        {"contrast", "Contrast", -100, 100, [](BasicAdjustments& a) -> float& { return a.contrast; }},
        {"highlights", "Highlights", -100, 100, [](BasicAdjustments& a) -> float& { return a.highlights; }},
        {"shadows", "Shadows", -100, 100, [](BasicAdjustments& a) -> float& { return a.shadows; }},
        {"whites", "Whites", -100, 100, [](BasicAdjustments& a) -> float& { return a.whites; }},
        {"blacks", "Blacks", -100, 100, [](BasicAdjustments& a) -> float& { return a.blacks; }},
        {"temperature", "Temperature", kMinTemperature, kMaxTemperature,
         [](BasicAdjustments& a) -> float& { return a.whiteBalance.temperature; }},
        {"tint", "Tint", -kMaxTint, kMaxTint, [](BasicAdjustments& a) -> float& { return a.whiteBalance.tint; }},
        {"vibrance", "Vibrance", -100, 100, [](BasicAdjustments& a) -> float& { return a.vibrance; }},
        {"saturation", "Saturation", -100, 100, [](BasicAdjustments& a) -> float& { return a.saturation; }},
    }};
    return fields;
}

const AdjustmentField* findAdjustmentField(const std::string& key)
{
    for (const AdjustmentField& field : adjustmentFields())
        if (key == field.key)
            return &field;
    return nullptr;
}

BasicAdjustments sanitized(BasicAdjustments adjustments)
{
    for (const AdjustmentField& field : adjustmentFields()) {
        float& v = field.value(adjustments);
        v = std::isfinite(v) ? std::clamp(v, field.minimum, field.maximum) : 0.0f;
    }
    if (!std::isfinite(adjustments.whiteBalance.temperature) || adjustments.whiteBalance.temperature == 0)
        adjustments.whiteBalance.temperature = 6500;
    return adjustments;
}

std::string describeChange(const BasicAdjustments& before, const BasicAdjustments& after)
{
    if (before.whiteBalance != after.whiteBalance) {
        BasicAdjustments a = before, b = after;
        a.whiteBalance = b.whiteBalance = {};
        return a == b ? "White Balance" : "Basic";
    }
    std::string changed;
    for (const AdjustmentField& field : adjustmentFields()) {
        BasicAdjustments a = before, b = after;
        if (field.value(a) != field.value(b)) {
            if (!changed.empty())
                return "Basic";
            changed = field.label;
        }
    }
    return changed.empty() ? "Basic" : changed;
}

} // namespace iris
