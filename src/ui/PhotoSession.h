#pragma once

#include "core/EditState.h"
#include "core/Image.h"
#include "core/PhotoMetadata.h"
#include "export/Exporter.h"
#include "rendering/Histogram.h"

#include <QImage>
#include <QObject>
#include <QPointF>
#include <QString>
#include <QTimer>

#include <array>
#include <atomic>
#include <memory>
#include <optional>

namespace iris::ui {

// The currently open photo and its edits. Decodes and renders on worker threads and
// reports results on the UI thread, so the UI never blocks on image processing.
//
// Loading runs two decodes in parallel: a fast half-size one for the first preview and a
// full-resolution one for the sharp preview, 100% view and export.
//
// Rendering uses three resolutions of the same source:
//   Draft   (1280 px)  while sliders are moving
//   Preview (3200 px)  once edits have settled
//   Full    (original) only when the view is zoomed in beyond the preview
// Each level has at most one render in flight; newer edits queue one follow-up render.
class PhotoSession : public QObject {
    Q_OBJECT

public:
    explicit PhotoSession(QObject* parent = nullptr);
    ~PhotoSession() override;

    void open(const QString& path);
    void exportTo(const QString& outputPath, const ExportSettings& settings);

    QString path() const { return m_path; }
    bool isLoaded() const { return m_previewSource != nullptr; }
    const PhotoMetadata& metadata() const { return m_metadata; }
    const EditState& edits() const { return m_edits; }

    enum class Update {
        Interactive, // a slider is moving: draft now, sharp preview once changes settle
        Immediate,   // a one-off change (load, undo, preset): draft and sharp preview now
    };
    // Sets the edits to render. Until the first call for a photo, nothing is rendered.
    void setEdits(const iris::EditState& edits, Update update = Update::Immediate);
    void setFullResolutionNeeded(bool needed);
    // The "before" (unedited) rendering is produced only while it is needed.
    void setBeforeNeeded(bool needed);
    // Shows the coverage of edits().masks[index] as an overlay (-1 = none). It follows edits.
    void setMaskOverlay(int index);

    std::optional<WhiteBalance> autoWhiteBalance() const;
    // Eyedropper at a normalised image position.
    std::optional<WhiteBalance> whiteBalanceAt(const QPointF& position) const;

signals:
    void loadingStarted(const QString& path);
    void metadataReady(const iris::PhotoMetadata& metadata);
    void previewReady(const QImage& preview, const QSize& fullSize);
    // Histogram of the preview on screen.
    void histogramReady(const iris::Histogram& histogram);
    void beforePreviewReady(const QImage& preview);
    void beforeFullReady(const QImage& image);
    // Tinted mask coverage at draft resolution; a null image when there is no overlay.
    void maskOverlayReady(const QImage& overlay);
    void fullImageReady(const QImage& image);
    void fullImageInvalidated();
    void loadFailed(const QString& path, const QString& message);
    void exportFinished(const QString& outputPath, const QString& error);

private:
    enum Level { Draft, Preview, Full, LevelCount };
    struct DecodeResult;
    struct RenderResult;
    struct RenderSlot {
        bool busy = false;
        bool pending = false;
        quint64 version = 0;               // edits being rendered
        const ImageF* source = nullptr;    // source being rendered
    };

    void handleDecoded(quint64 generation, bool fullResolution, const DecodeResult& result);
    void requestRender(Level level);
    void startRender(Level level);
    void handleRendered(Level level, quint64 version, const RenderResult& result);
    void renderBefore(Level level);
    void requestOverlay();
    std::shared_ptr<const ImageF> source(Level level) const;
    bool isCurrent(quint64 generation) const { return generation == m_generation->load(); }

    QString m_path;
    PhotoMetadata m_metadata;
    EditState m_edits;
    bool m_editsReady = false;
    bool m_fullLoaded = false;

    std::shared_ptr<const ImageF> m_draftSource;
    std::shared_ptr<const ImageF> m_previewSource;
    std::shared_ptr<const ImageF> m_fullSource;

    std::array<RenderSlot, LevelCount> m_slots;
    quint64 m_editVersion = 1;   // bumped on every edit
    quint64 m_shownVersion = 0;  // version of the preview on screen
    int m_shownLevel = -1;
    quint64 m_fullVersion = 0;   // version of the full-resolution image on screen (0 = none)
    bool m_fullNeeded = false;
    QTimer m_settleTimer;

    bool m_beforeNeeded = false;
    std::array<const ImageF*, LevelCount> m_beforeRendered{}; // source each before image was made from

    int m_overlayMask = -1;
    bool m_overlayBusy = false;
    bool m_overlayPending = false;

    // Shared with worker threads so they can notice they have been superseded.
    std::shared_ptr<std::atomic<quint64>> m_generation = std::make_shared<std::atomic<quint64>>(0);
};

} // namespace iris::ui
