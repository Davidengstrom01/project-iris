#include "ui/PhotoSession.h"

#include "raw/RawDecoder.h"
#include "rendering/Pipeline.h"
#include "rendering/Resample.h"

#include <QFuture>
#include <QtConcurrent/QtConcurrentRun>

namespace iris::ui {

namespace {

// Long edge of the screen preview rendering. Large enough for a 4K window in fit mode.
constexpr int kPreviewLongEdge = 3200;

} // namespace

struct PhotoSession::PreviewResult {
    PhotoMetadata metadata;
    QImage preview;
    QString error;
    bool cancelled = false;
};

struct PhotoSession::FullResult {
    std::shared_ptr<const ImageF> source;
    PhotoMetadata metadata;
    QImage preview;
    QImage full;
    QString error;
    bool cancelled = false;
};

PhotoSession::PhotoSession(QObject* parent) : QObject(parent) {}

PhotoSession::~PhotoSession()
{
    ++*m_generation; // cancel in-flight decodes
}

void PhotoSession::open(const QString& path)
{
    const quint64 generation = ++*m_generation;
    m_path = path;
    m_metadata = {};
    m_loaded = false;
    m_fullLoaded = false;
    m_fullSource.reset();
    emit loadingStarted(path);

    const std::string file = path.toStdString();
    const auto token = m_generation;
    const CancelCheck cancelled = [token, generation] { return token->load() != generation; };

    QtConcurrent::run([file, cancelled] {
        PreviewResult r;
        try {
            DecodedRaw decoded = decodeRaw(file, DecodeQuality::Preview, cancelled);
            r.metadata = decoded.metadata;
            r.preview = toQImage(render(decoded.image, {.maxLongEdge = kPreviewLongEdge}));
        } catch (const DecodeCancelled&) {
            r.cancelled = true;
        } catch (const std::exception& e) {
            r.error = QString::fromStdString(e.what());
        }
        return r;
    }).then(this, [this, generation](const PreviewResult& r) { handlePreview(generation, r); });

    QtConcurrent::run([file, cancelled] {
        FullResult r;
        try {
            DecodedRaw decoded = decodeRaw(file, DecodeQuality::Full, cancelled);
            r.metadata = decoded.metadata;
            auto source = std::make_shared<const ImageF>(std::move(decoded.image));
            if (cancelled())
                throw DecodeCancelled();
            r.preview = toQImage(render(*source, {.maxLongEdge = kPreviewLongEdge}));
            r.full = toQImage(render(*source, {}));
            r.source = std::move(source);
        } catch (const DecodeCancelled&) {
            r.cancelled = true;
        } catch (const std::exception& e) {
            r.error = QString::fromStdString(e.what());
        }
        return r;
    }).then(this, [this, generation](const FullResult& r) { handleFull(generation, r); });
}

void PhotoSession::handlePreview(quint64 generation, const PreviewResult& r)
{
    // A late half-size preview must not replace the sharper one from the full decode.
    if (!isCurrent(generation) || r.cancelled || m_fullLoaded)
        return;
    if (!r.error.isEmpty()) {
        emit loadFailed(m_path, r.error);
        return;
    }
    m_metadata = r.metadata;
    m_loaded = true;
    emit metadataReady(m_metadata);
    emit previewReady(r.preview, QSize(m_metadata.width, m_metadata.height));
}

void PhotoSession::handleFull(quint64 generation, const FullResult& r)
{
    if (!isCurrent(generation) || r.cancelled)
        return;
    if (!r.error.isEmpty()) {
        // The preview decode reports the error unless it has already succeeded.
        if (m_loaded)
            emit loadFailed(m_path, r.error);
        return;
    }
    m_fullLoaded = true;
    m_fullSource = r.source;
    m_metadata = r.metadata;
    const bool first = !m_loaded;
    m_loaded = true;
    if (first)
        emit metadataReady(m_metadata);
    emit previewReady(r.preview, r.full.size());
    emit fullImageReady(r.full);
}

void PhotoSession::exportTo(const QString& outputPath, const ExportSettings& settings)
{
    const std::shared_ptr<const ImageF> source = m_fullSource;
    const std::string rawPath = m_path.toStdString();
    const std::string output = outputPath.toStdString();

    QtConcurrent::run([source, rawPath, output, settings]() -> QString {
        try {
            // Export always uses the full-resolution decode of the original RAW.
            std::shared_ptr<const ImageF> image = source;
            if (!image)
                image = std::make_shared<const ImageF>(decodeRaw(rawPath, DecodeQuality::Full).image);
            exportImage(*image, settings, output);
            return {};
        } catch (const std::exception& e) {
            return QString::fromStdString(e.what());
        }
    }).then(this, [this, outputPath](const QString& error) { emit exportFinished(outputPath, error); });
}

} // namespace iris::ui
