#pragma once

#include "core/Image.h"
#include "core/PhotoMetadata.h"
#include "export/Exporter.h"

#include <QImage>
#include <QObject>
#include <QString>

#include <atomic>
#include <memory>

namespace iris::ui {

// The currently open photo. Runs decoding and rendering on worker threads and reports
// results back on the UI thread, so the UI never blocks on image processing.
//
// Loading happens in two stages that run in parallel:
//   1. a fast half-size decode -> first preview on screen
//   2. a full-resolution decode -> sharper preview, 100% view, export source
// Opening another photo cancels the work for the previous one.
class PhotoSession : public QObject {
    Q_OBJECT

public:
    explicit PhotoSession(QObject* parent = nullptr);
    ~PhotoSession() override;

    void open(const QString& path);
    void exportTo(const QString& outputPath, const ExportSettings& settings);

    QString path() const { return m_path; }
    bool isLoaded() const { return m_loaded; }
    const PhotoMetadata& metadata() const { return m_metadata; }

signals:
    void loadingStarted(const QString& path);
    void previewReady(const QImage& preview, const QSize& fullSize);
    void fullImageReady(const QImage& image);
    void metadataReady(const iris::PhotoMetadata& metadata);
    void loadFailed(const QString& path, const QString& message);
    void exportFinished(const QString& outputPath, const QString& error);

private:
    struct PreviewResult;
    struct FullResult;

    void handlePreview(quint64 generation, const PreviewResult& result);
    void handleFull(quint64 generation, const FullResult& result);
    bool isCurrent(quint64 generation) const { return generation == m_generation->load(); }

    QString m_path;
    PhotoMetadata m_metadata;
    bool m_loaded = false;
    bool m_fullLoaded = false;
    std::shared_ptr<const ImageF> m_fullSource;
    // Shared with worker threads so they can notice they have been superseded.
    std::shared_ptr<std::atomic<quint64>> m_generation = std::make_shared<std::atomic<quint64>>(0);
};

} // namespace iris::ui
