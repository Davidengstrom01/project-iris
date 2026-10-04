#include "rendering/ColorTransform.h"

#include <lcms2.h>

#include <stdexcept>

namespace iris {

namespace {

cmsHPROFILE createLinearRec2020Profile()
{
    const cmsCIExyY d65 = {0.3127, 0.3290, 1.0};
    const cmsCIExyYTRIPLE primaries = {
        {0.708, 0.292, 1.0},
        {0.170, 0.797, 1.0},
        {0.131, 0.046, 1.0},
    };
    cmsToneCurve* linear = cmsBuildGamma(nullptr, 1.0);
    cmsToneCurve* curves[3] = {linear, linear, linear};
    cmsHPROFILE profile = cmsCreateRGBProfile(&d65, &primaries, curves);
    cmsFreeToneCurve(linear);
    return profile;
}

} // namespace

const OutputTransform& OutputTransform::sRGB(int bitsPerChannel)
{
    static const OutputTransform srgb8(8);
    static const OutputTransform srgb16(16);
    return bitsPerChannel == 16 ? srgb16 : srgb8;
}

OutputTransform::OutputTransform(int bitsPerChannel)
{
    cmsHPROFILE working = createLinearRec2020Profile();
    cmsHPROFILE output = cmsCreate_sRGBProfile();
    // NOCACHE makes cmsDoTransform safe to call concurrently on one transform.
    m_transform = cmsCreateTransform(working, TYPE_RGB_FLT, output, bitsPerChannel == 16 ? TYPE_RGB_16 : TYPE_RGB_8,
                                     INTENT_RELATIVE_COLORIMETRIC, cmsFLAGS_NOCACHE);
    cmsCloseProfile(working);
    cmsCloseProfile(output);
    if (!m_transform)
        throw std::runtime_error("Cannot create output colour transform");
}

OutputTransform::~OutputTransform()
{
    cmsDeleteTransform(static_cast<cmsHTRANSFORM>(m_transform));
}

void OutputTransform::apply(const float* in, void* out, std::size_t pixelCount) const
{
    cmsDoTransform(static_cast<cmsHTRANSFORM>(m_transform), in, out, static_cast<cmsUInt32Number>(pixelCount));
}

} // namespace iris
