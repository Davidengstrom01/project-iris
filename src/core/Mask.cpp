#include "core/Mask.h"

#include <algorithm>
#include <cmath>

namespace iris {

namespace {

float clean(float v, float lo, float hi, float fallback)
{
    return std::isfinite(v) ? std::clamp(v, lo, hi) : fallback;
}

// Angles are kept in (-180, 180].
float normalizedAngle(float degrees)
{
    if (!std::isfinite(degrees))
        return 0;
    degrees = std::fmod(degrees, 360.0f);
    if (degrees > 180)
        degrees -= 360;
    else if (degrees <= -180)
        degrees += 360;
    return degrees;
}

} // namespace

const char* maskTypeKey(MaskType type)
{
    switch (type) {
    case MaskType::Linear: return "linear";
    case MaskType::Radial: return "radial";
    default: return "brush";
    }
}

const char* brushModeKey(BrushMode mode)
{
    switch (mode) {
    case BrushMode::Subtract: return "subtract";
    case BrushMode::Erase: return "erase";
    default: return "add";
    }
}

const char* maskTypeName(MaskType type)
{
    switch (type) {
    case MaskType::Linear: return "Linear Gradient";
    case MaskType::Radial: return "Radial Gradient";
    default: return "Brush";
    }
}

const std::array<LocalAdjustmentField, 6>& localAdjustmentFields()
{
    static const std::array<LocalAdjustmentField, 6> fields = {{
        {"exposure", "Exposure", -4, 4, [](LocalAdjustments& a) -> float& { return a.exposure; }},
        {"contrast", "Contrast", -100, 100, [](LocalAdjustments& a) -> float& { return a.contrast; }},
        {"highlights", "Highlights", -100, 100, [](LocalAdjustments& a) -> float& { return a.highlights; }},
        {"shadows", "Shadows", -100, 100, [](LocalAdjustments& a) -> float& { return a.shadows; }},
        {"saturation", "Saturation", -100, 100, [](LocalAdjustments& a) -> float& { return a.saturation; }},
        {"temperature", "Temperature", -100, 100, [](LocalAdjustments& a) -> float& { return a.temperature; }},
    }};
    return fields;
}

Mask newMask(MaskType type, const std::vector<Mask>& existing)
{
    Mask mask;
    mask.type = type;
    const std::string base = type == MaskType::Brush ? "Brush" : type == MaskType::Linear ? "Linear" : "Radial";
    for (int n = 1;; ++n) {
        const std::string name = base + " " + std::to_string(n);
        if (std::none_of(existing.begin(), existing.end(), [&](const Mask& m) { return m.name == name; })) {
            mask.name = name;
            break;
        }
    }
    return mask;
}

Mask sanitized(Mask mask)
{
    LinearGradient& l = mask.linear;
    l.x = clean(l.x, -1, 2, 0.5f);
    l.y = clean(l.y, -1, 2, 0.5f);
    l.angle = normalizedAngle(l.angle);
    l.feather = clean(l.feather, 0, 2, 0.3f);

    RadialGradient& r = mask.radial;
    r.x = clean(r.x, -1, 2, 0.5f);
    r.y = clean(r.y, -1, 2, 0.5f);
    r.width = clean(r.width, 0.002f, 4, 0.4f);
    r.height = clean(r.height, 0.002f, 4, 0.3f);
    r.rotation = normalizedAngle(r.rotation);
    r.feather = clean(r.feather, 0, 1, 0.5f);

    if (mask.strokes.size() > kMaxStrokesPerMask)
        mask.strokes.resize(kMaxStrokesPerMask);
    std::erase_if(mask.strokes, [](const BrushStroke& s) { return s.points.empty(); });
    for (BrushStroke& s : mask.strokes) {
        s.radius = clean(s.radius, kMinBrushRadius, kMaxBrushRadius, 0.05f);
        s.feather = clean(s.feather, 0, 1, 0.5f);
        s.opacity = clean(s.opacity, 0, 1, 1);
        if (s.points.size() > kMaxPointsPerStroke)
            s.points.resize(kMaxPointsPerStroke);
        for (MaskPoint& p : s.points) {
            p.x = clean(p.x, -1, 2, 0);
            p.y = clean(p.y, -1, 2, 0);
        }
    }

    for (const LocalAdjustmentField& field : localAdjustmentFields()) {
        float& v = field.value(mask.adjustments);
        v = clean(v, field.minimum, field.maximum, 0);
    }
    return mask;
}

} // namespace iris
