// iris-cli: headless RAW rendering using the same engine as the desktop app.
//
//   iris-cli INPUT OUTPUT [--quality N] [--long-edge N] [--16bit]
//   iris-cli --info INPUT

#include "export/Exporter.h"
#include "raw/RawDecoder.h"

#include <QCoreApplication>

#include <chrono>
#include <cstdio>
#include <string>

namespace {

int usage()
{
    std::fprintf(stderr,
                 "Usage: iris-cli INPUT OUTPUT [--quality N] [--long-edge N] [--16bit]\n"
                 "       iris-cli --info INPUT\n"
                 "OUTPUT format is chosen from its extension: .jpg, .png or .tif\n");
    return 2;
}

bool endsWith(const std::string& s, const std::string& suffix)
{
    if (s.size() < suffix.size())
        return false;
    for (std::size_t i = 0; i < suffix.size(); ++i)
        if (std::tolower(static_cast<unsigned char>(s[s.size() - suffix.size() + i])) != suffix[i])
            return false;
    return true;
}

double secondsSince(std::chrono::steady_clock::time_point start)
{
    return std::chrono::duration<double>(std::chrono::steady_clock::now() - start).count();
}

} // namespace

int main(int argc, char** argv)
{
    QCoreApplication app(argc, argv);
    if (argc < 3)
        return usage();

    try {
        if (std::string(argv[1]) == "--info") {
            const auto m = iris::decodeRaw(argv[2], iris::DecodeQuality::Preview).metadata;
            std::printf("Camera:      %s %s\nLens:        %s\nISO:         %.0f\nShutter:     %g s\n"
                        "Aperture:    f/%.1f\nFocal:       %.0f mm\nOrientation: %d\nSize:        %d x %d\n",
                        m.make.c_str(), m.model.c_str(), m.lens.c_str(), m.iso, m.shutterSeconds, m.aperture,
                        m.focalLengthMm, m.orientation, m.width, m.height);
            return 0;
        }

        const std::string input = argv[1];
        const std::string output = argv[2];
        iris::ExportSettings settings;
        if (endsWith(output, ".png"))
            settings.format = iris::ExportFormat::Png;
        else if (endsWith(output, ".tif") || endsWith(output, ".tiff"))
            settings.format = iris::ExportFormat::Tiff;
        else if (!endsWith(output, ".jpg") && !endsWith(output, ".jpeg"))
            return usage();

        for (int i = 3; i < argc; ++i) {
            const std::string arg = argv[i];
            if (arg == "--quality" && i + 1 < argc)
                settings.jpegQuality = std::stoi(argv[++i]);
            else if (arg == "--long-edge" && i + 1 < argc)
                settings.longEdge = std::stoi(argv[++i]);
            else if (arg == "--16bit")
                settings.bitsPerChannel = 16;
            else
                return usage();
        }

        auto start = std::chrono::steady_clock::now();
        const iris::DecodedRaw decoded = iris::decodeRaw(input, iris::DecodeQuality::Full);
        std::printf("Decoded %d x %d in %.2f s\n", decoded.image.width, decoded.image.height, secondsSince(start));

        start = std::chrono::steady_clock::now();
        iris::exportImage(decoded.image, settings, output);
        std::printf("Exported %s in %.2f s\n", output.c_str(), secondsSince(start));
    } catch (const std::exception& e) {
        std::fprintf(stderr, "iris-cli: %s\n", e.what());
        return 1;
    }
    return 0;
}
