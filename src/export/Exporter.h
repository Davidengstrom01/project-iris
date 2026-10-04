#pragma once

#include "core/EditState.h"
#include "core/Image.h"

#include <QImage>

#include <string>

namespace iris {

enum class ExportFormat { Jpeg, Png, Tiff };

struct ExportSettings {
    ExportFormat format = ExportFormat::Jpeg;
    int jpegQuality = 92;    // 1-100
    int longEdge = 0;        // 0 = original resolution; otherwise resize so the long edge fits
    int bitsPerChannel = 8;  // PNG/TIFF: 8 or 16. JPEG is always 8.
};

// Lower-case file extension without the dot ("jpg", "png", "tif").
const char* fileExtension(ExportFormat format);

// Renders the full-resolution source with the edits through the pipeline and writes it
// as an sRGB file with an embedded ICC profile. The write is atomic: an existing file is
// only replaced once the new one has been written completely. Throws std::runtime_error.
void exportImage(const ImageF& fullResolutionSource, const WhiteBalance& asShot, const EditState& edits,
                 const ExportSettings& settings, const std::string& outputPath);

// Wraps an encoded sRGB image in a QImage (deep copy) tagged with the sRGB colour space.
QImage toQImage(const EncodedImage& image);

} // namespace iris
