#include "raw/RawDecoder.h"

#include "core/ColorScience.h"

#include <libraw/libraw.h>

#include <cmath>
#include <memory>

namespace iris {

namespace {

constexpr int kOutputColorRec2020 = 8; // LibRaw/dcraw "-o 8"

struct ProcessedImageDeleter {
    void operator()(libraw_processed_image_t* image) const { LibRaw::dcraw_clear_mem(image); }
};

int progressCallback(void* data, LibRaw_progress, int, int)
{
    const auto* cancelled = static_cast<const CancelCheck*>(data);
    return (*cancelled && (*cancelled)()) ? 1 : 0;
}

void check(int result, const char* step)
{
    if (result == LIBRAW_CANCELLED_BY_CALLBACK)
        throw DecodeCancelled();
    if (result != LIBRAW_SUCCESS)
        throw RawDecodeError(std::string(step) + ": " + libraw_strerror(result));
}

int exifOrientationFromFlip(int flip)
{
    switch (flip) {
    case 3: return 3;
    case 5: return 8;
    case 6: return 6;
    default: return 1;
    }
}

// The camera's as-shot white balance: the colour a neutral surface had in camera RGB
// (inverse of the WB multipliers), converted to XYZ through the camera matrix.
WhiteBalance asShotWhiteBalance(const libraw_colordata_t& color)
{
    const float* mul = color.cam_mul[0] > 0 && color.cam_mul[1] > 0 && color.cam_mul[2] > 0 ? color.cam_mul
                                                                                             : color.pre_mul;
    const Mat3 xyzToCamera = {
        color.cam_xyz[0][0], color.cam_xyz[0][1], color.cam_xyz[0][2],
        color.cam_xyz[1][0], color.cam_xyz[1][1], color.cam_xyz[1][2],
        color.cam_xyz[2][0], color.cam_xyz[2][1], color.cam_xyz[2][2],
    };
    const Mat3 cameraToXyz = inverse(xyzToCamera);
    if (mul[0] > 0 && mul[1] > 0 && mul[2] > 0 && std::isfinite(cameraToXyz[0])) {
        const Vec3 xyz = cameraToXyz * Vec3{1.0 / mul[0], 1.0 / mul[1], 1.0 / mul[2]};
        const double sum = xyz[0] + xyz[1] + xyz[2];
        if (std::isfinite(sum) && sum > 0 && xyz[1] > 0)
            return whiteBalanceFromWhitePoint(chromaticity(xyz));
    }
    return whiteBalanceFromWhitePoint(kD65);
}

PhotoMetadata extractMetadata(const libraw_data_t& data)
{
    PhotoMetadata m;
    m.make = data.idata.make;
    m.model = data.idata.model;
    m.lens = data.lens.Lens;
    if (m.lens.empty())
        m.lens = data.lens.makernotes.Lens;
    while (!m.lens.empty() && m.lens.back() == ' ')
        m.lens.pop_back();
    m.iso = data.other.iso_speed;
    m.shutterSeconds = data.other.shutter;
    m.aperture = data.other.aperture;
    m.focalLengthMm = data.other.focal_len;
    m.timestamp = static_cast<std::int64_t>(data.other.timestamp);
    m.orientation = exifOrientationFromFlip(data.sizes.flip);
    const bool swapped = data.sizes.flip & 4;
    m.width = swapped ? data.sizes.height : data.sizes.width;
    m.height = swapped ? data.sizes.width : data.sizes.height;
    m.asShot = asShotWhiteBalance(data.color);
    return m;
}

} // namespace

DecodedRaw decodeRaw(const std::string& path, DecodeQuality quality, const CancelCheck& cancelled)
{
    // LibRaw is large (hundreds of KB); keep it off the stack.
    auto raw = std::make_unique<LibRaw>();
    raw->set_progress_handler(progressCallback, const_cast<CancelCheck*>(&cancelled));

    // Scene-linear output in a wide-gamut working space; no automatic brightening,
    // so the pipeline sees the sensor's real exposure. Clipped highlights stay neutral.
    libraw_output_params_t& p = raw->imgdata.params;
    p.output_color = kOutputColorRec2020;
    p.output_bps = 16;
    p.gamm[0] = 1.0;
    p.gamm[1] = 1.0;
    p.no_auto_bright = 1;
    p.use_camera_wb = 1;
    p.use_camera_matrix = 1;
    p.highlight = 0;
    p.half_size = quality == DecodeQuality::Preview ? 1 : 0;

    check(raw->open_file(path.c_str()), "Cannot open RAW file");
    DecodedRaw result;
    result.metadata = extractMetadata(raw->imgdata);

    check(raw->unpack(), "Cannot read RAW data");
    if (cancelled && cancelled())
        throw DecodeCancelled();
    check(raw->dcraw_process(), "Cannot process RAW data");

    int error = LIBRAW_SUCCESS;
    std::unique_ptr<libraw_processed_image_t, ProcessedImageDeleter> processed(raw->dcraw_make_mem_image(&error));
    check(error, "Cannot create image");
    if (!processed || processed->type != LIBRAW_IMAGE_BITMAP || processed->colors != 3 || processed->bits != 16)
        throw RawDecodeError("Unsupported RAW image layout");

    ImageF image(processed->width, processed->height);
    const auto* src = reinterpret_cast<const std::uint16_t*>(processed->data);
    const std::ptrdiff_t samples = std::ptrdiff_t(image.pixels.size());
    constexpr float scale = 1.0f / 65535.0f;
#pragma omp parallel for schedule(static)
    for (std::ptrdiff_t i = 0; i < samples; ++i)
        image.pixels[i] = src[i] * scale;

    result.image = std::move(image);
    if (quality == DecodeQuality::Full) {
        result.metadata.width = result.image.width;
        result.metadata.height = result.image.height;
    }
    return result;
}

const std::vector<std::string>& rawFileExtensions()
{
    static const std::vector<std::string> extensions = {
        "arw", "srf", "sr2", "cr2", "cr3", "crw", "nef", "nrw", "raf", "dng",
        "orf", "rw2", "pef", "srw", "3fr", "iiq", "erf", "kdc", "mos", "rwl",
    };
    return extensions;
}

} // namespace iris
