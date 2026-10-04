// iris-cli: headless RAW rendering using the same engine as the desktop app.
//
//   iris-cli INPUT OUTPUT [--quality N] [--long-edge N] [--16bit] [--edits FILE] [--set NAME=VALUE ...]
//   iris-cli --info INPUT
//
// Edits come from the photo's .iris.json sidecar if it has one (or from --edits FILE);
// --set then adjusts individual settings, e.g. --set exposure=0.7 --set temperature=5200.

#include "export/Exporter.h"
#include "persistence/Sidecar.h"
#include "raw/RawDecoder.h"

#include <QCoreApplication>

#include <chrono>
#include <cstdio>
#include <vector>
#include <string>

namespace {

int usage()
{
    std::fprintf(stderr,
                 "Usage: iris-cli INPUT OUTPUT [--quality N] [--long-edge N] [--16bit] [--edits FILE]\n"
                 "                    [--set NAME=VALUE ...]\n"
                 "       iris-cli --info INPUT\n"
                 "OUTPUT format is chosen from its extension: .jpg, .png or .tif\n"
                 "Settings: exposure contrast highlights shadows whites blacks\n"
                 "          temperature tint vibrance saturation\n");
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

bool applySetting(iris::BasicAdjustments& a, const std::string& assignment)
{
    const auto eq = assignment.find('=');
    if (eq == std::string::npos)
        return false;
    const iris::AdjustmentField* field = iris::findAdjustmentField(assignment.substr(0, eq));
    if (!field)
        return false;
    field->value(a) = std::stof(assignment.substr(eq + 1));
    a = iris::sanitized(a);
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
                        "Aperture:    f/%.1f\nFocal:       %.0f mm\nOrientation: %d\nSize:        %d x %d\n"
                        "As shot WB:  %.0f K, tint %+.0f\n",
                        m.make.c_str(), m.model.c_str(), m.lens.c_str(), m.iso, m.shutterSeconds, m.aperture,
                        m.focalLengthMm, m.orientation, m.width, m.height, m.asShot.temperature, m.asShot.tint);
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

        std::vector<std::string> assignments;
        std::string editsFile;
        for (int i = 3; i < argc; ++i) {
            const std::string arg = argv[i];
            if (arg == "--quality" && i + 1 < argc)
                settings.jpegQuality = std::stoi(argv[++i]);
            else if (arg == "--long-edge" && i + 1 < argc)
                settings.longEdge = std::stoi(argv[++i]);
            else if (arg == "--16bit")
                settings.bitsPerChannel = 16;
            else if (arg == "--edits" && i + 1 < argc)
                editsFile = argv[++i];
            else if (arg == "--set" && i + 1 < argc)
                assignments.push_back(argv[++i]);
            else
                return usage();
        }

        auto start = std::chrono::steady_clock::now();
        const iris::DecodedRaw decoded = iris::decodeRaw(input, iris::DecodeQuality::Full);
        std::printf("Decoded %d x %d in %.2f s\n", decoded.image.width, decoded.image.height, secondsSince(start));

        iris::EditState edits = iris::defaultEditState(decoded.metadata.asShot);
        const QString sidecar = editsFile.empty() ? iris::sidecarPathFor(QString::fromStdString(input))
                                                  : QString::fromStdString(editsFile);
        const iris::SidecarResult saved = iris::readSidecarFile(sidecar, edits);
        if (!saved.error.isEmpty())
            throw std::runtime_error("Cannot read " + sidecar.toStdString() + ": " + saved.error.toStdString());
        if (saved.edits) {
            edits = *saved.edits;
            std::printf("Using edits from %s\n", sidecar.toStdString().c_str());
        }
        for (const std::string& assignment : assignments)
            if (!applySetting(edits.basic, assignment))
                return usage();

        start = std::chrono::steady_clock::now();
        iris::exportImage(decoded.image, decoded.metadata.asShot, edits, settings, output);
        std::printf("Exported %s in %.2f s\n", output.c_str(), secondsSince(start));
    } catch (const std::exception& e) {
        std::fprintf(stderr, "iris-cli: %s\n", e.what());
        return 1;
    }
    return 0;
}
