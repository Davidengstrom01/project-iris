#pragma once

#include <array>
#include <cstddef>
#include <string>
#include <vector>

namespace iris {

// Local adjustments: a mask selects part of the photo and a few develop adjustments are
// applied there. Geometry is resolution-independent so the preview and the export agree:
//   positions are fractions of the image width (x) and height (y), 0..1;
//   lengths (brush radius, gradient feather, ellipse size) are fractions of the long edge.
// Masks live in the coordinates of the uncropped, unrotated photo.

enum class MaskType { Brush, Linear, Radial };

// How a brush stroke changes the mask:
//   Add       paints the mask in
//   Subtract  removes the mask where painted, including any gradient underneath
//   Erase     removes earlier brush strokes (both Add and Subtract) where painted
enum class BrushMode { Add, Subtract, Erase };

struct MaskPoint {
    float x = 0;
    float y = 0;

    bool operator==(const MaskPoint&) const = default;
};

struct BrushStroke {
    BrushMode mode = BrushMode::Add;
    float radius = 0.05f; // fraction of the long edge
    float feather = 0.5f; // soft part of the radius, 0 (hard) .. 1 (soft from the centre)
    float opacity = 1.0f; // 0..1
    std::vector<MaskPoint> points;

    bool operator==(const BrushStroke&) const = default;
};

// Full effect on one side of a line, fading out across `feather`.
struct LinearGradient {
    float x = 0.5f; // centre of the transition
    float y = 0.4f;
    float angle = 0;      // degrees; 0 = horizontal with the effect above, counter-clockwise
    float feather = 0.3f; // width of the transition, fraction of the long edge

    bool operator==(const LinearGradient&) const = default;
};

// Full effect inside an ellipse, fading out towards its edge.
struct RadialGradient {
    float x = 0.5f;
    float y = 0.5f;
    float width = 0.4f;  // diameters, fractions of the long edge
    float height = 0.3f;
    float rotation = 0;  // degrees, counter-clockwise
    float feather = 0.5f; // soft part of the radius, 0..1

    bool operator==(const RadialGradient&) const = default;
};

// The adjustments a mask applies. They act like the global sliders, relative to them.
struct LocalAdjustments {
    float exposure = 0;    // EV, -4..+4
    float contrast = 0;    // -100..+100
    float highlights = 0;  // -100..+100
    float shadows = 0;     // -100..+100
    float saturation = 0;  // -100..+100
    float temperature = 0; // -100..+100, relative: + warmer

    bool isNeutral() const { return *this == LocalAdjustments{}; }
    bool operator==(const LocalAdjustments&) const = default;
};

// One mask: a shape (none for a brush mask), brush strokes that refine it, and the
// adjustments applied through it. Invert flips the shape and Add / Erase strokes; Subtract
// strokes always remove the effect.
struct Mask {
    MaskType type = MaskType::Brush;
    std::string name;
    bool invert = false;
    LinearGradient linear; // used when type == Linear
    RadialGradient radial; // used when type == Radial
    std::vector<BrushStroke> strokes;
    LocalAdjustments adjustments;

    bool operator==(const Mask&) const = default;
};

inline constexpr std::size_t kMaxMasks = 32;
inline constexpr std::size_t kMaxStrokesPerMask = 2000;
inline constexpr std::size_t kMaxPointsPerStroke = 20000;
inline constexpr float kMinBrushRadius = 0.001f;
inline constexpr float kMaxBrushRadius = 0.5f;

// "brush" / "linear" / "radial" and "add" / "subtract" / "erase" (file format keys)
const char* maskTypeKey(MaskType type);
const char* brushModeKey(BrushMode mode);
// "Brush", "Linear Gradient", "Radial Gradient"
const char* maskTypeName(MaskType type);

// Describes one local adjustment (shared by persistence and the UI).
struct LocalAdjustmentField {
    const char* key;
    const char* label;
    float minimum;
    float maximum;
    float& (*value)(LocalAdjustments&);
};

const std::array<LocalAdjustmentField, 6>& localAdjustmentFields();

// A new mask with a default shape and a name such as "Radial 2" that is not yet used.
Mask newMask(MaskType type, const std::vector<Mask>& existing);

// Clamps every value into its valid range and drops unusable strokes (e.g. after reading
// a file).
Mask sanitized(Mask mask);

} // namespace iris
