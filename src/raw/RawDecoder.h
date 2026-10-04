#pragma once

#include "core/Image.h"
#include "core/PhotoMetadata.h"

#include <functional>
#include <stdexcept>
#include <string>
#include <vector>

namespace iris {

enum class DecodeQuality {
    Preview, // half-size demosaic: ~4x faster, used for the first on-screen preview
    Full,    // full-resolution demosaic: used for 100% view and export
};

struct DecodedRaw {
    ImageF image; // linear Rec.2020, camera white balance applied, orientation applied
    PhotoMetadata metadata;
};

class RawDecodeError : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

class DecodeCancelled : public RawDecodeError {
public:
    DecodeCancelled() : RawDecodeError("Decoding cancelled") {}
};

// Returns true when decoding should stop early. Called from the decoding thread.
using CancelCheck = std::function<bool()>;

// Decodes a RAW file. The file is opened read-only and never modified.
// Throws RawDecodeError (or DecodeCancelled) on failure.
DecodedRaw decodeRaw(const std::string& path, DecodeQuality quality, const CancelCheck& cancelled = {});

// Lower-case RAW file extensions (without the dot) that Project Iris offers to open.
const std::vector<std::string>& rawFileExtensions();

} // namespace iris
