# Project Iris

A lightweight, Linux-first, non-destructive RAW photo editor.

**Open RAW → Edit → Apply presets/masks → Compare → Save edits → Export**

Original RAW files are only ever opened read-only.

## Status

| Phase | Scope | State |
|---|---|---|
| 1 — Foundation | Window, open, RAW decode, preview, zoom/pan, metadata, export | ✅ done |
| 2 — Basic development | Exposure, contrast, highlights/shadows, whites/blacks, WB, vibrance/saturation | next |
| 3 — Editing state + presets | Sidecars, undo/redo, before/after, presets | |
| 4 — Tone curve | | |
| 5 — HSL | | |
| 6 — Masking | | |
| 7 — Crop & polish | | |
| 8 — Packaging | AppImage, `.deb` | |

## Building

Dependencies (Arch package names): `qt6-base libraw lcms2 cmake ninja` and a C++20 compiler.
Debian/Ubuntu: `qt6-base-dev libraw-dev liblcms2-dev cmake ninja-build g++`.

```sh
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build
./build/src/iris [photo.ARW]
```

### Tests

```sh
ctest --test-dir build --output-on-failure
# The UI smoke test opens a real RAW file when one is provided:
IRIS_TEST_RAW=/path/to/photo.ARW ctest --test-dir build --output-on-failure
```

### Command line

`iris-cli` renders with the same engine as the desktop app:

```sh
./build/src/iris-cli photo.ARW photo.jpg --quality 92 --long-edge 2048
./build/src/iris-cli photo.ARW photo.tif --16bit
./build/src/iris-cli --info photo.ARW
```

## Keyboard shortcuts

| Key | Action |
|---|---|
| Ctrl+O | Open |
| Ctrl+E | Export |
| 1 | Fit image |
| 2 | 100% |
| Ctrl++ / Ctrl+- | Zoom in / out |
| Tab | Hide/show side panels |
| Mouse wheel | Zoom around the cursor |
| Drag | Pan |
| Double-click | Toggle fit / 100% at the cursor |

## Architecture

```
src/core        Image buffers and metadata types (header-only, no dependencies)
src/raw         LibRaw decoding → linear Rec.2020 float working image
src/rendering   The rendering pipeline: resample + LittleCMS output transform (OpenMP)
src/export      JPEG/PNG/TIFF writing (QtGui image writers, embedded sRGB ICC profile)
src/ui          Qt Widgets desktop application
src/cli         Headless renderer
tests           Qt Test suites
```

Dependency direction: `ui / cli → export → rendering → core`, and `ui / cli → raw → core`.
The engine libraries contain no UI code; the UI contains no image processing.

### Pipeline

RAW files are decoded by LibRaw with camera white balance into **scene-linear Rec.2020**
(32-bit float), with no automatic brightening. All editing will happen in this space. The
same `render()` function produces the screen preview and the exported file; only the
resolution differs. The output transform (working space → sRGB) is done by LittleCMS.

### Responsiveness

Opening a photo starts two decodes on worker threads:

1. a half-size decode (~0.3 s), shown as soon as it is ready;
2. a full-resolution decode (~0.8 s for 24 MP), which replaces the preview with a sharper
   one and provides the 100% view and the export source.

Opening another photo cancels in-flight decodes (via LibRaw's progress callback), so
browsing a folder quickly stays responsive. Export runs in the background and writes
atomically (an existing file is only replaced once the new one is complete).
