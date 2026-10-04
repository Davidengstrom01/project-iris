#pragma once

#include "core/EditState.h"

#include <QString>

#include <optional>

namespace iris {

// Edits are stored next to the RAW file as JSON: photo.ARW -> photo.iris.json.
// If that name is already taken by another photo's sidecar (photo.ARW and photo.CR2 in
// one folder), photo.ARW.iris.json is used instead.
QString sidecarPathFor(const QString& rawPath);

struct SidecarResult {
    std::optional<EditState> edits; // empty if there is no sidecar or it could not be read
    QString error;                  // set if a sidecar exists but could not be read
};

// Reads the sidecar of rawPath. Missing values fall back to `defaults`.
SidecarResult readSidecar(const QString& rawPath, const EditState& defaults);

// Reads edits from an explicit sidecar file.
SidecarResult readSidecarFile(const QString& sidecarPath, const EditState& defaults);

// Writes edits for rawPath to sidecarPath atomically. Returns an error message on failure.
QString writeSidecar(const QString& sidecarPath, const QString& rawPath, const EditState& edits);

inline constexpr int kSidecarVersion = 1;

} // namespace iris
