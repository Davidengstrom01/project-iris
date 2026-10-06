#pragma once

namespace iris {

// Crop and rotation of a photo, applied after all tonal and colour edits:
//
//   photo -> quarter turns -> straighten (rotate about the centre) -> crop rectangle
//
// The crop rectangle is axis-aligned in the straightened frame: the frame has the size of
// the photo after its quarter turns, and the photo is rotated by `angle` inside it.
struct Crop {
    int quarterTurns = 0; // clockwise 90° rotations, 0..3
    float angle = 0;      // straighten, degrees, -45..45; positive turns the photo clockwise
    float left = 0;       // crop rectangle as fractions of the straightened frame
    float top = 0;
    float right = 1;
    float bottom = 1;
    float aspect = 0;     // locked aspect ratio (width / height in pixels); 0 = free

    bool hasRectangle() const { return left != 0 || top != 0 || right != 1 || bottom != 1; }
    bool isIdentity() const { return quarterTurns == 0 && angle == 0 && !hasRectangle(); }
    bool operator==(const Crop&) const = default;
};

inline constexpr float kMaxStraightenAngle = 45;

// x' = m11 x + m12 y + dx,  y' = m21 x + m22 y + dy
struct Affine {
    double m11 = 1, m12 = 0, m21 = 0, m22 = 1, dx = 0, dy = 0;

    void map(double x, double y, double& outX, double& outY) const
    {
        outX = m11 * x + m12 * y + dx;
        outY = m21 * x + m22 * y + dy;
    }
    Affine inverted() const;
    // (a * b) maps a point through b, then a.
    friend Affine operator*(const Affine& a, const Affine& b);
    static Affine scale(double sx, double sy) { return {sx, 0, 0, sy, 0, 0}; }
    static Affine translate(double tx, double ty) { return {1, 0, 0, 1, tx, ty}; }
};

// The result of cropping an image of a given size: its size and where each photo pixel
// lands in it. Coordinates are continuous, with pixel centres at i + 0.5.
struct CropGeometry {
    int width = 0;
    int height = 0;
    Affine photoToResult;
};

// Size of the straightened frame (the photo after its quarter turns).
void frameSize(int photoWidth, int photoHeight, int quarterTurns, int& width, int& height);

// Geometry of `crop` for a photo of the given size. With wholeFrame the crop rectangle is
// ignored and the result is the whole straightened frame (as shown while cropping).
// Without rotation, the rectangle is rounded to whole pixels so crops stay sharp.
CropGeometry cropGeometry(int photoWidth, int photoHeight, const Crop& crop, bool wholeFrame = false);

// Whether the crop rectangle lies within the frame and within the straightened photo
// (no empty corners).
bool cropFitsPhoto(const Crop& crop, int photoWidth, int photoHeight);

// Shrinks the crop rectangle about its centre (keeping its aspect ratio) until it fits.
Crop constrainedCrop(Crop crop, int photoWidth, int photoHeight);

// The largest rectangle of the given aspect ratio (0 = the frame's), centred on the
// current one, that fits the photo.
Crop withAspect(Crop crop, float aspect, int photoWidth, int photoHeight);

// Turns the photo by 90°; the crop rectangle turns with it.
Crop rotatedQuarter(const Crop& crop, bool clockwise);

// Clamps values into range (e.g. after reading a file); the rectangle may still need
// constrainedCrop() once the photo size is known.
Crop sanitized(Crop crop);

} // namespace iris
