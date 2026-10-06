#include "ui/PhotoSession.h"

#include "raw/RawDecoder.h"
#include "rendering/MaskCoverage.h"
#include "rendering/Pipeline.h"
#include "rendering/Resample.h"
#include "rendering/WhiteBalanceTools.h"

#include <QFuture>
#include <QtConcurrent/QtConcurrentRun>

namespace iris::ui {

namespace {

constexpr int kDraftLongEdge = 1280;
constexpr int kPreviewLongEdge = 3200; // large enough for a 4K window in fit mode
constexpr int kSettleDelayMs = 200;    // pause after the last edit before rendering the sharp preview
const QRgb kOverlayColor = qRgb(255, 48, 64);
constexpr double kOverlayOpacity = 0.5;

// Mask coverage as a translucent tint.
QImage overlayImage(const std::vector<std::uint8_t>& coverage, int width, int height)
{
    QImage image(width, height, QImage::Format_ARGB32_Premultiplied);
    for (int y = 0; y < height; ++y) {
        QRgb* out = reinterpret_cast<QRgb*>(image.scanLine(y));
        const std::uint8_t* in = &coverage[std::size_t(y) * width];
        for (int x = 0; x < width; ++x) {
            const int a = int(in[x] * kOverlayOpacity + 0.5);
            out[x] = qRgba(qRed(kOverlayColor) * a / 255, qGreen(kOverlayColor) * a / 255,
                           qBlue(kOverlayColor) * a / 255, a);
        }
    }
    return image;
}

} // namespace

struct PhotoSession::DecodeResult {
    PhotoMetadata metadata;
    std::shared_ptr<const ImageF> draft;
    std::shared_ptr<const ImageF> preview;
    std::shared_ptr<const ImageF> full;
    QString error;
    bool cancelled = false;
};

struct PhotoSession::RenderResult {
    QImage image;
    Histogram histogram;
};

PhotoSession::PhotoSession(QObject* parent) : QObject(parent)
{
    m_settleTimer.setSingleShot(true);
    m_settleTimer.setInterval(kSettleDelayMs);
    connect(&m_settleTimer, &QTimer::timeout, this, [this] { requestRender(Preview); });
}

PhotoSession::~PhotoSession()
{
    ++*m_generation; // cancel in-flight decodes
}

void PhotoSession::open(const QString& path)
{
    const quint64 generation = ++*m_generation;
    m_path = path;
    m_metadata = {};
    m_edits = {};
    m_editsReady = false;
    m_fullLoaded = false;
    m_draftSource.reset();
    m_previewSource.reset();
    m_fullSource.reset();
    ++m_editVersion;
    m_shownVersion = 0;
    m_shownLevel = -1;
    m_fullVersion = 0;
    m_settleTimer.stop();
    m_beforeRendered = {};
    m_overlayMask = -1;
    emit loadingStarted(path);

    const std::string file = path.toStdString();
    const auto token = m_generation;
    const CancelCheck cancelled = [token, generation] { return token->load() != generation; };

    for (const bool fullResolution : {false, true}) {
        QtConcurrent::run([file, cancelled, fullResolution] {
            DecodeResult r;
            try {
                DecodedRaw decoded =
                    decodeRaw(file, fullResolution ? DecodeQuality::Full : DecodeQuality::Preview, cancelled);
                r.metadata = decoded.metadata;
                auto image = std::make_shared<const ImageF>(std::move(decoded.image));
                r.preview = std::make_shared<const ImageF>(downscaleToFit(*image, kPreviewLongEdge));
                r.draft = std::make_shared<const ImageF>(downscaleToFit(*r.preview, kDraftLongEdge));
                if (fullResolution)
                    r.full = std::move(image);
            } catch (const DecodeCancelled&) {
                r.cancelled = true;
            } catch (const std::exception& e) {
                r.error = QString::fromStdString(e.what());
            }
            return r;
        }).then(this, [this, generation, fullResolution](const DecodeResult& r) {
            handleDecoded(generation, fullResolution, r);
        });
    }
}

void PhotoSession::handleDecoded(quint64 generation, bool fullResolution, const DecodeResult& r)
{
    if (!isCurrent(generation) || r.cancelled)
        return;
    if (!r.error.isEmpty()) {
        // Report once: from the half-size decode, or from the full one if the first succeeded.
        if (!fullResolution || isLoaded())
            emit loadFailed(m_path, r.error);
        return;
    }
    // A late half-size decode must not replace sources derived from the full decode.
    if (!fullResolution && m_fullLoaded)
        return;

    m_draftSource = r.draft;
    m_previewSource = r.preview;
    if (fullResolution) {
        m_fullSource = r.full;
        m_fullLoaded = true;
    }
    const bool firstResult = !m_metadata.width;
    m_metadata = r.metadata;
    if (fullResolution) {
        m_metadata.width = r.full->width;
        m_metadata.height = r.full->height;
    }
    // Listeners respond by calling setEdits() with the photo's saved edits.
    if (firstResult)
        emit metadataReady(m_metadata);
    if (!m_editsReady)
        setEdits(defaultEditState(m_metadata.asShot));

    requestRender(Preview);
    if (fullResolution && m_fullNeeded)
        requestRender(Full);
    if (m_beforeNeeded) {
        renderBefore(Preview);
        if (fullResolution && m_fullNeeded)
            renderBefore(Full);
    }
}

void PhotoSession::setEdits(const EditState& edits, Update update)
{
    if (m_editsReady && edits == m_edits)
        return;
    m_edits = edits;
    m_editsReady = true;
    ++m_editVersion;
    if (m_fullVersion != 0) {
        m_fullVersion = 0;
        emit fullImageInvalidated();
    }
    requestRender(Draft);
    requestOverlay();
    if (update == Update::Interactive) {
        m_settleTimer.start();
    } else {
        m_settleTimer.stop();
        requestRender(Preview);
    }
}

void PhotoSession::setFullResolutionNeeded(bool needed)
{
    m_fullNeeded = needed;
    if (needed && m_fullVersion != m_editVersion)
        requestRender(Full);
    if (needed && m_beforeNeeded)
        renderBefore(Full);
}

void PhotoSession::setBeforeNeeded(bool needed)
{
    m_beforeNeeded = needed;
    if (needed) {
        renderBefore(Preview);
        if (m_fullNeeded)
            renderBefore(Full);
    }
}

void PhotoSession::setMaskOverlay(int index)
{
    if (index == m_overlayMask)
        return;
    m_overlayMask = index;
    requestOverlay();
}

void PhotoSession::requestOverlay()
{
    if (m_overlayBusy) {
        m_overlayPending = true;
        return;
    }
    if (m_overlayMask < 0 || m_overlayMask >= int(m_edits.masks.size()) || !m_draftSource) {
        emit maskOverlayReady(QImage());
        return;
    }
    m_overlayBusy = true;
    m_overlayPending = false;
    const Mask mask = m_edits.masks[m_overlayMask];
    const int width = m_draftSource->width, height = m_draftSource->height;
    const quint64 generation = m_generation->load();
    QtConcurrent::run([mask, width, height] {
        return overlayImage(renderMaskCoverage(mask, width, height), width, height);
    }).then(this, [this, generation](const QImage& overlay) {
        m_overlayBusy = false;
        // Show it even if the mask changed meanwhile (it is still the newest available),
        // then catch up.
        if (isCurrent(generation) && m_overlayMask >= 0)
            emit maskOverlayReady(overlay);
        if (m_overlayPending)
            requestOverlay();
    });
}

void PhotoSession::renderBefore(Level level)
{
    // The unedited photo does not change while editing; render each source only once.
    const std::shared_ptr<const ImageF> image = source(level);
    if (!image || m_beforeRendered[level] == image.get())
        return;
    m_beforeRendered[level] = image.get();
    const WhiteBalance asShot = m_metadata.asShot;
    const quint64 generation = m_generation->load();
    QtConcurrent::run([image, asShot] {
        try {
            return toQImage(render(*image, asShot, defaultEditState(asShot), {}));
        } catch (const std::exception&) {
            return QImage();
        }
    }).then(this, [this, level, generation](const QImage& result) {
        if (!isCurrent(generation) || result.isNull())
            return;
        if (level == Full)
            emit beforeFullReady(result);
        else
            emit beforePreviewReady(result);
    });
}

std::shared_ptr<const ImageF> PhotoSession::source(Level level) const
{
    switch (level) {
    case Draft: return m_draftSource;
    case Preview: return m_previewSource;
    default: return m_fullSource;
    }
}

void PhotoSession::requestRender(Level level)
{
    if (!m_editsReady || !source(level))
        return;
    RenderSlot& slot = m_slots[level];
    if (!slot.busy)
        startRender(level);
    else if (slot.version != m_editVersion || slot.source != source(level).get())
        slot.pending = true; // re-render once the current one finishes
}

void PhotoSession::startRender(Level level)
{
    RenderSlot& slot = m_slots[level];
    const std::shared_ptr<const ImageF> image = source(level);
    slot.busy = true;
    slot.pending = false;
    slot.version = m_editVersion;
    slot.source = image.get();

    const WhiteBalance asShot = m_metadata.asShot;
    const EditState edits = m_edits;
    const quint64 generation = m_generation->load();
    const quint64 version = m_editVersion;

    const bool withHistogram = level != Full;
    QtConcurrent::run([image, asShot, edits, withHistogram] {
        RenderResult result;
        try {
            const EncodedImage encoded = render(*image, asShot, edits, {});
            if (withHistogram)
                result.histogram = computeHistogram(encoded);
            result.image = toQImage(encoded);
        } catch (const std::exception&) {
        }
        return result;
    }).then(this, [this, level, generation, version](const RenderResult& result) {
        m_slots[level].busy = false;
        if (isCurrent(generation) && !result.image.isNull())
            handleRendered(level, version, result);
        if (m_slots[level].pending)
            requestRender(level);
    });
}

void PhotoSession::handleRendered(Level level, quint64 version, const RenderResult& result)
{
    const QImage& image = result.image;
    if (level == Full) {
        if (version == m_editVersion) {
            m_fullVersion = version;
            emit fullImageReady(image);
        }
        return;
    }
    // Show a result unless something newer, or the same edits at higher quality, is on screen.
    if (version > m_shownVersion || (version == m_shownVersion && level >= m_shownLevel)) {
        m_shownVersion = version;
        m_shownLevel = level;
        emit previewReady(image, QSize(m_metadata.width, m_metadata.height));
        emit histogramReady(result.histogram);
    }
    if (level == Preview && version == m_editVersion && m_fullNeeded && m_fullVersion != version)
        requestRender(Full);
}

std::optional<WhiteBalance> PhotoSession::autoWhiteBalance() const
{
    if (!m_draftSource)
        return std::nullopt;
    return estimateWhiteBalance(*m_draftSource, m_metadata.asShot);
}

std::optional<WhiteBalance> PhotoSession::whiteBalanceAt(const QPointF& position) const
{
    if (!m_previewSource)
        return std::nullopt;
    return sampleWhiteBalance(*m_previewSource, position.x(), position.y(), m_metadata.asShot);
}

void PhotoSession::exportTo(const QString& outputPath, const ExportSettings& settings)
{
    const std::shared_ptr<const ImageF> source = m_fullSource;
    const std::string rawPath = m_path.toStdString();
    const std::string output = outputPath.toStdString();
    const WhiteBalance asShot = m_metadata.asShot;
    const EditState edits = m_edits;

    QtConcurrent::run([source, rawPath, output, settings, asShot, edits]() -> QString {
        try {
            // Export always uses the full-resolution decode of the original RAW.
            std::shared_ptr<const ImageF> image = source;
            if (!image)
                image = std::make_shared<const ImageF>(decodeRaw(rawPath, DecodeQuality::Full).image);
            exportImage(*image, asShot, edits, settings, output);
            return {};
        } catch (const std::exception& e) {
            return QString::fromStdString(e.what());
        }
    }).then(this, [this, outputPath](const QString& error) { emit exportFinished(outputPath, error); });
}

} // namespace iris::ui
