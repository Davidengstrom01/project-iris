#pragma once

#include "core/EditState.h"
#include "core/Image.h"

#include <optional>

namespace iris {

// Auto white balance: grey-world estimate over well-exposed pixels of a source image
// decoded with the as-shot white balance.
WhiteBalance estimateWhiteBalance(const ImageF& source, const WhiteBalance& asShot);

// Eyedropper: the white balance that makes the area around (x, y) neutral. x and y are
// normalised [0, 1] image coordinates. Returns nothing if the area is clipped or black.
std::optional<WhiteBalance> sampleWhiteBalance(const ImageF& source, double x, double y, const WhiteBalance& asShot);

} // namespace iris
