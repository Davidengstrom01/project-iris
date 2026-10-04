#pragma once

#include "core/Image.h"

namespace iris {

// Area-averaging (box) downscale in linear light. Only shrinks; requested sizes
// larger than the source are clamped to the source size.
ImageF resizeArea(const ImageF& source, int width, int height);

// Size that fits within maxLongEdge while keeping the aspect ratio (never upscales).
void fitSize(int width, int height, int maxLongEdge, int& outWidth, int& outHeight);

// Downscales so the long edge is at most maxLongEdge.
ImageF downscaleToFit(const ImageF& source, int maxLongEdge);

} // namespace iris
