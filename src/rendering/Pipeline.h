#pragma once

#include "core/EditState.h"
#include "core/Image.h"

namespace iris {

struct RenderOptions {
    int maxLongEdge = 0;     // 0 = render at source resolution
    int bitsPerChannel = 8;  // 8 or 16
};

// The single rendering pipeline shared by the interactive preview and export:
//
//   source (linear Rec.2020, as-shot white balance)
//     -> resize
//     -> white balance + exposure           (one 3x3 matrix, scene-linear)
//     -> highlights / shadows                (edge-aware local gain, scene-linear)
//     -> contrast / whites / blacks          (hue-preserving -> display-linear)
//     -> RGB tone curve                      (hue-preserving, perceptual space)
//     -> HSL: hue / saturation / luminance per colour range (Oklab)
//     -> vibrance / saturation
//     -> output colour transform (sRGB)
//
// Later stages (masks, crop, detail) slot in between.
// asShot is the white balance the source was decoded with.
EncodedImage render(const ImageF& source, const WhiteBalance& asShot, const EditState& edits,
                    const RenderOptions& options);

} // namespace iris
