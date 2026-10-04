#pragma once

#include "core/Image.h"

namespace iris {

struct RenderOptions {
    int maxLongEdge = 0;     // 0 = render at source resolution
    int bitsPerChannel = 8;  // 8 or 16
};

// The single rendering pipeline shared by the interactive preview and export:
//
//   working-space source -> [resize] -> output colour transform (sRGB)
//
// Editing stages (adjustments, tone curve, colour, masks, crop, detail) are inserted
// between the resize and the output transform as they are implemented.
EncodedImage render(const ImageF& source, const RenderOptions& options);

} // namespace iris
