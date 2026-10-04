#include "export/Exporter.h"

#include "rendering/Pipeline.h"

#include <QColorSpace>
#include <QImageWriter>
#include <QSaveFile>

#include <cstring>
#include <stdexcept>

namespace iris {

namespace {

QByteArray writerFormat(ExportFormat format)
{
    switch (format) {
    case ExportFormat::Jpeg: return "jpeg";
    case ExportFormat::Png: return "png";
    case ExportFormat::Tiff: return "tiff";
    }
    return "jpeg";
}

} // namespace

const char* fileExtension(ExportFormat format)
{
    switch (format) {
    case ExportFormat::Jpeg: return "jpg";
    case ExportFormat::Png: return "png";
    case ExportFormat::Tiff: return "tif";
    }
    return "jpg";
}

QImage toQImage(const EncodedImage& image)
{
    QImage result;
    if (image.bitsPerChannel == 16) {
        result = QImage(image.width, image.height, QImage::Format_RGBX64);
        for (int y = 0; y < image.height; ++y) {
            const std::uint16_t* src = image.data16.data() + std::size_t(y) * image.width * 3;
            auto* dst = reinterpret_cast<quint16*>(result.scanLine(y));
            for (int x = 0; x < image.width; ++x) {
                dst[x * 4 + 0] = src[x * 3 + 0];
                dst[x * 4 + 1] = src[x * 3 + 1];
                dst[x * 4 + 2] = src[x * 3 + 2];
                dst[x * 4 + 3] = 0xffff;
            }
        }
    } else {
        result = QImage(image.width, image.height, QImage::Format_RGB888);
        const std::size_t rowBytes = std::size_t(image.width) * 3;
        for (int y = 0; y < image.height; ++y)
            std::memcpy(result.scanLine(y), image.data8.data() + std::size_t(y) * rowBytes, rowBytes);
    }
    result.setColorSpace(QColorSpace::SRgb);
    return result;
}

void exportImage(const ImageF& fullResolutionSource, const WhiteBalance& asShot, const EditState& edits,
                 const ExportSettings& settings, const std::string& outputPath)
{
    if (fullResolutionSource.empty())
        throw std::runtime_error("Nothing to export");

    RenderOptions options;
    options.maxLongEdge = settings.longEdge;
    options.bitsPerChannel = settings.format == ExportFormat::Jpeg ? 8 : settings.bitsPerChannel;
    const QImage image = toQImage(render(fullResolutionSource, asShot, edits, options));

    QSaveFile file(QString::fromStdString(outputPath));
    if (!file.open(QIODevice::WriteOnly))
        throw std::runtime_error("Cannot write " + outputPath + ": " + file.errorString().toStdString());

    QImageWriter writer(&file, writerFormat(settings.format));
    if (settings.format == ExportFormat::Jpeg) {
        writer.setQuality(settings.jpegQuality);
        writer.setOptimizedWrite(true);
    } else if (settings.format == ExportFormat::Tiff) {
        writer.setCompression(1); // LZW
    }
    if (!writer.write(image))
        throw std::runtime_error("Cannot encode image: " + writer.errorString().toStdString());
    if (!file.commit())
        throw std::runtime_error("Cannot write " + outputPath + ": " + file.errorString().toStdString());
}

} // namespace iris
