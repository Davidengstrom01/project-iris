#include "core/Hsl.h"

#include <algorithm>
#include <cmath>

namespace iris {

const char* hslColorKey(HslColor color)
{
    static const char* keys[] = {"red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta"};
    return keys[int(color)];
}

const char* hslColorName(HslColor color)
{
    static const char* names[] = {"Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"};
    return names[int(color)];
}

HslAdjustments sanitized(HslAdjustments hsl)
{
    for (HslBand& band : hsl.bands)
        for (float* v : {&band.hue, &band.saturation, &band.luminance})
            *v = std::isfinite(*v) ? std::clamp(*v, -100.0f, 100.0f) : 0.0f;
    return hsl;
}

} // namespace iris
