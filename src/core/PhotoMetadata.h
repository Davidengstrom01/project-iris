#pragma once

#include <cstdint>
#include <string>

namespace iris {

// Shooting information extracted from a RAW file. Zero / empty means "unknown".
struct PhotoMetadata {
    std::string make;
    std::string model;
    std::string lens;
    float iso = 0;
    float shutterSeconds = 0;
    float aperture = 0;        // f-number
    float focalLengthMm = 0;
    std::int64_t timestamp = 0; // seconds since epoch
    int orientation = 1;        // EXIF orientation (1 = normal, 3 = 180°, 6 = 90° CW, 8 = 90° CCW)
    int width = 0;              // full-resolution size after orientation is applied
    int height = 0;
};

} // namespace iris
