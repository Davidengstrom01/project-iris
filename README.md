# Project Iris

A lightweight, Linux-first, non-destructive RAW photo editor.

**Open RAW → Edit → Apply presets/masks → Compare → Save edits → Export**

Original RAW files are only ever opened read-only.

## Status

| Phase | Scope | State |
|---|---|---|
| 1 — Foundation | Window, open, RAW decode, preview, zoom/pan, metadata, export | ✅ done |
| 2 — Basic development | Exposure, contrast, highlights/shadows, whites/blacks, WB (+ auto, eyedropper), vibrance/saturation | ✅ done |
| 3 — Editing state + presets | Sidecars, undo/redo, before/after, presets | ✅ done |
| 4 — Tone curve | Histogram, RGB point curve, S / inverse-S presets | ✅ done |
| 5 — HSL | Hue / saturation / luminance for eight colour ranges | ✅ done |
| 6 — Masking | | next |
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

`iris-cli` renders with the same engine as the desktop app, using the photo's saved edits
(its `.iris.json` sidecar) when there are any:

```sh
./build/src/iris-cli photo.ARW photo.jpg --quality 92 --long-edge 2048
./build/src/iris-cli photo.ARW photo.tif --16bit --set exposure=0.5 --set shadows=30
./build/src/iris-cli photo.ARW photo.jpg --edits other.iris.json
./build/src/iris-cli --info photo.ARW
```

## Keyboard shortcuts

| Key | Action |
|---|---|
| Ctrl+O | Open |
| Ctrl+S | Save edits (sidecar) |
| Ctrl+Shift+S | Save edits as… |
| Ctrl+E | Export |
| Ctrl+Z / Ctrl+Shift+Z (Ctrl+Y) | Undo / Redo |
| Ctrl+Shift+R | Reset all edits |
| \ | Before/after: tap to toggle, hold to peek |
| Y | Split before/after view (drag the divider) |
| 1 | Fit image |
| 2 | 100% |
| Ctrl++ / Ctrl+- | Zoom in / out |
| Tab | Hide/show side panels |
| Mouse wheel | Zoom around the cursor |
| Drag | Pan |
| Double-click | Toggle fit / 100% at the cursor |
| Double-click a slider name | Reset that slider |
| Esc | Cancel the white-balance eyedropper |
| Tone curve: click / drag / double-click | Add / move / remove a point (also right-click or Delete) |

## Architecture

```
src/core        Image buffers and metadata types (header-only, no dependencies)
src/raw         LibRaw decoding → linear Rec.2020 float working image
src/rendering   The rendering pipeline: resample + LittleCMS output transform (OpenMP)
src/export      JPEG/PNG/TIFF writing (QtGui image writers, embedded sRGB ICC profile)
src/persistence .iris.json sidecars (QtCore JSON)
src/presets     Preset format and library (QtCore JSON)
presets/builtin Built-in presets: ordinary preset files, bundled as Qt resources
src/ui          Qt Widgets desktop application
src/cli         Headless renderer
tests           Qt Test suites
```

Dependency direction: `ui / cli → export → rendering → core`, `ui / cli → raw → core`,
`ui / cli → persistence → core` and `ui → presets → core`.
The engine libraries contain no UI code; the UI contains no image processing.

### Pipeline

RAW files are decoded by LibRaw with the camera's white balance into **scene-linear
Rec.2020** (32-bit float), with no automatic brightening. The same `render()` function
produces the screen preview and the exported file; only the resolution differs.

```
source (linear Rec.2020, as-shot WB)
  -> resize
  -> white balance + exposure        one 3x3 matrix (Bradford adaptation), scene-linear
  -> highlights / shadows            edge-aware local gain (guided-filter base layer)
  -> contrast / whites / blacks      hue-preserving tone mapping -> display-linear
  -> RGB tone curve                  monotone spline, hue-preserving (same lookup table)
  -> HSL                             per colour range, in Oklab
  -> vibrance / saturation
  -> sRGB output transform           LittleCMS
```

- **White balance** is absolute (Kelvin + tint, 1 tint unit = 1/3000 Duv). The as-shot
  value is derived from the camera's colour matrix and WB multipliers. Absolute numbers can
  differ somewhat from Lightroom's, which uses Adobe's own camera profiles.
- **Highlights/Shadows** work on regions, not single pixels: a guided filter computed at a
  fixed 1024 px resolution decides how bright each region is, so local detail is kept, edges
  get no halos, and the preview matches the full-resolution export.
- The **tone curve** is a monotone cubic spline through the control points (no overshoot
  between points), applied in gamma 2.2 space. Because both it and the basic tone mapping
  keep hue, they are combined into one lookup table. Only the RGB curve exists so far;
  `ToneCurve` is ready for separate red/green/blue curves.
- **HSL** works in Oklab, a perceptual colour space where hue angles match how colours look
  and lightness is separate from chroma. The eight ranges (red, orange, yellow, green,
  aqua, blue, purple, magenta) are centred on reference sRGB colours; each pixel blends
  smoothly between its two nearest ranges, and near-neutral pixels are left alone.
- With every slider at zero the render is neutral (no hidden "look" curve).

### Editing state, undo and sidecars

All edits live in `EditState`, a plain value. Undo/redo keeps snapshots of it
(`EditHistory`); a slider drag is merged into a single step. Nothing ever writes to the RAW
file. **Ctrl+S** saves the edits next to it:

```
photo.ARW
photo.iris.json     {"version": 1, "originalFilename": "photo.ARW", "adjustments": {...},
                     "toneCurve": {"points": [[0, 0], [0.25, 0.2], [0.75, 0.8], [1, 1]]},
                     "hsl": {"blue": {"hue": 0, "saturation": 20, "luminance": -30}}}
```

Reopening a photo restores its sidecar automatically. If two RAW files share a base name
(`photo.ARW`, `photo.CR2`), the second one uses `photo.CR2.iris.json`. Leaving a photo or
quitting with unsaved edits asks whether to save them.

### Presets

A preset is a JSON file holding only the settings it was saved with; applying it changes
just those (as one undoable step) and never touches crop, masks or other photo-specific
edits. *Save Preset…* lets you pick which settings to include and a folder to put it in.

```json
{"version": 1, "name": "Warm Film", "adjustments": {"contrast": 10, "temperatureShift": 14}}
```

Absolute `temperature`/`tint` set a fixed white balance; `temperatureShift` (mired,
positive = warmer) and `tintShift` adjust relative to the photo, which is what the
built-in looks use. User presets are stored in `~/.local/share/project-iris/iris/presets/<Folder>/`.

### Responsiveness

Opening a photo starts two decodes on worker threads: a half-size one (~0.3 s), shown as
soon as it is ready, and a full-resolution one (~0.8 s for 24 MP) that provides a sharper
preview and the source for 100% view and export. Opening another photo cancels in-flight
decodes (via LibRaw's progress callback).

Edits render at three resolutions of the same source, each on a worker thread with at
most one render in flight per level (newer edits replace queued ones):

| Level | Size | When |
|---|---|---|
| Draft | 1280 px | immediately on every slider change (~50 ms) |
| Preview | 3200 px | 200 ms after the last change |
| Full | original | only when zoomed in beyond the preview |

Export runs in the background and writes atomically (an existing file is only replaced
once the new one is complete).
