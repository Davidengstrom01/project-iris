#include "rendering/Pipeline.h"

#include "rendering/ColorTransform.h"
#include "rendering/Resample.h"

namespace iris {

EncodedImage render(const ImageF& source, const RenderOptions& options)
{
    // Resize first so every later stage runs at the output resolution.
    const ImageF* input = &source;
    ImageF resized;
    int width = 0, height = 0;
    fitSize(source.width, source.height, options.maxLongEdge, width, height);
    if (width != source.width || height != source.height) {
        resized = resizeArea(source, width, height);
        input = &resized;
    }

    const int bits = options.bitsPerChannel == 16 ? 16 : 8;
    const OutputTransform& transform = OutputTransform::sRGB(bits);
    EncodedImage output(input->width, input->height, bits);
#pragma omp parallel for schedule(static)
    for (int y = 0; y < input->height; ++y)
        transform.apply(input->row(y), output.row(y), std::size_t(input->width));
    return output;
}

} // namespace iris
