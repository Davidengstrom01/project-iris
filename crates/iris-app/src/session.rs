//! The currently open photo and its rendering. Decodes and renders on worker threads and
//! reports results through [`PhotoSession::poll`], so the UI never blocks on image
//! processing.
//!
//! Loading runs two decodes in parallel: a fast half-size one for the first preview and a
//! full-resolution one for the sharp preview, 100% view and export.
//!
//! Rendering uses three resolutions of the same source:
//!   Draft   (1280 px)  while sliders are moving
//!   Preview (3200 px)  once edits have settled
//!   Full    (original) only when the view is zoomed in beyond the preview
//! Each level has at most one render in flight; newer edits queue one follow-up render.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use egui::Color32;
use iris_core::{EditState, ImageF, Mask, PhotoMetadata, WhiteBalance};
use iris_export::{ExportSettings, export_image};
use iris_raw::{DecodeQuality, decode};
use iris_render::{
    Histogram, RenderOptions, downscale_to_fit, estimate_white_balance, render, render_mask_coverage,
    sample_white_balance,
};

use crate::texture::{TiledImage, tile_size};

const DRAFT_LONG_EDGE: usize = 1280;
/// Large enough for a 4K window in fit mode.
const PREVIEW_LONG_EDGE: usize = 3200;
/// Pause after the last edit before rendering the sharp preview.
const SETTLE_DELAY: Duration = Duration::from_millis(200);
const OVERLAY_COLOR: [u8; 3] = [255, 48, 64];
const OVERLAY_OPACITY: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Draft = 0,
    Preview = 1,
    Full = 2,
}

/// How quickly an edit should reach the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    /// A slider is moving: draft now, sharp preview once changes settle.
    Interactive,
    /// A one-off change (load, undo, preset): draft and sharp preview now.
    Immediate,
}

/// What happened since the last [`PhotoSession::poll`].
pub enum SessionEvent {
    MetadataReady(PhotoMetadata),
    /// A rendering of the whole photo; `full_size` is the full-resolution size.
    PreviewReady {
        image: TiledImage,
        full_size: [usize; 2],
        histogram: Box<Histogram>,
    },
    BeforePreviewReady(TiledImage),
    BeforeFullReady(TiledImage),
    /// Tinted coverage of the mask being edited; `None` when there is no overlay.
    MaskOverlayReady(Option<TiledImage>),
    FullImageReady(TiledImage),
    FullImageInvalidated,
    LoadFailed {
        path: PathBuf,
        message: String,
    },
    ExportFinished {
        path: PathBuf,
        error: Option<String>,
    },
}

struct Decoded {
    metadata: PhotoMetadata,
    draft: Arc<ImageF>,
    preview: Arc<ImageF>,
    full: Option<Arc<ImageF>>,
}

enum WorkerMessage {
    Decoded { generation: u64, full_resolution: bool, result: Result<Decoded, Option<String>> },
    Rendered { level: Level, generation: u64, version: u64, image: TiledImage, histogram: Option<Box<Histogram>> },
    Before { level: Level, generation: u64, image: TiledImage },
    Overlay { generation: u64, image: TiledImage },
    Exported { path: PathBuf, error: Option<String> },
}

#[derive(Default)]
struct RenderSlot {
    busy: bool,
    pending: bool,
    /// Edits being rendered.
    version: u64,
    /// Source being rendered (pointer identity).
    source: usize,
}

fn identity(image: &Arc<ImageF>) -> usize {
    Arc::as_ptr(image) as usize
}

pub struct PhotoSession {
    path: Option<PathBuf>,
    metadata: PhotoMetadata,
    edits: EditState,
    edits_ready: bool,
    full_loaded: bool,

    draft_source: Option<Arc<ImageF>>,
    preview_source: Option<Arc<ImageF>>,
    full_source: Option<Arc<ImageF>>,

    slots: [RenderSlot; 3],
    /// Bumped on every edit.
    edit_version: u64,
    /// Version of the preview on screen.
    shown_version: u64,
    shown_level: Option<Level>,
    /// Version of the full-resolution image on screen (0 = none).
    full_version: u64,
    full_needed: bool,
    settle_deadline: Option<Instant>,

    before_needed: bool,
    /// Source each "before" image was made from.
    before_rendered: [usize; 3],

    overlay_mask: Option<usize>,
    overlay_busy: bool,
    overlay_pending: bool,

    exports_running: usize,

    /// Shared with worker threads so they can notice they have been superseded.
    generation: Arc<AtomicU64>,
    sender: Sender<WorkerMessage>,
    receiver: Receiver<WorkerMessage>,
    repaint: egui::Context,
    events: Vec<SessionEvent>,
}

impl PhotoSession {
    /// `repaint` is asked to repaint whenever a worker finishes.
    pub fn new(repaint: egui::Context) -> Self {
        let (sender, receiver) = channel();
        Self {
            path: None,
            metadata: PhotoMetadata::default(),
            edits: EditState::default(),
            edits_ready: false,
            full_loaded: false,
            draft_source: None,
            preview_source: None,
            full_source: None,
            slots: Default::default(),
            edit_version: 1,
            shown_version: 0,
            shown_level: None,
            full_version: 0,
            full_needed: false,
            settle_deadline: None,
            before_needed: false,
            before_rendered: [0; 3],
            overlay_mask: None,
            overlay_busy: false,
            overlay_pending: false,
            exports_running: 0,
            generation: Arc::new(AtomicU64::new(0)),
            sender,
            receiver,
            repaint,
            events: Vec::new(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_loaded(&self) -> bool {
        self.preview_source.is_some()
    }

    pub fn exports_running(&self) -> usize {
        self.exports_running
    }

    fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Runs `job` on a worker thread and sends its message back.
    fn spawn(&self, job: impl FnOnce() -> Option<WorkerMessage> + Send + 'static) {
        let sender = self.sender.clone();
        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            if let Some(message) = job() {
                let _ = sender.send(message);
                repaint.request_repaint();
            }
        });
    }

    pub fn open(&mut self, path: &Path) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.path = Some(path.to_owned());
        self.metadata = PhotoMetadata::default();
        self.edits = EditState::default();
        self.edits_ready = false;
        self.full_loaded = false;
        self.draft_source = None;
        self.preview_source = None;
        self.full_source = None;
        self.edit_version += 1;
        self.shown_version = 0;
        self.shown_level = None;
        self.full_version = 0;
        self.settle_deadline = None;
        self.before_rendered = [0; 3];
        self.overlay_mask = None;

        for full_resolution in [false, true] {
            let path = path.to_owned();
            let token = self.generation.clone();
            self.spawn(move || {
                let cancelled = move || token.load(Ordering::SeqCst) != generation;
                let quality = if full_resolution { DecodeQuality::Full } else { DecodeQuality::Preview };
                let result = match decode(&path, quality, &cancelled) {
                    Ok(decoded) => {
                        let image = Arc::new(decoded.image);
                        let preview = Arc::new(downscale_to_fit(&image, PREVIEW_LONG_EDGE));
                        let draft = Arc::new(downscale_to_fit(&preview, DRAFT_LONG_EDGE));
                        Ok(Decoded {
                            metadata: decoded.metadata,
                            draft,
                            preview,
                            full: full_resolution.then_some(image),
                        })
                    }
                    Err(iris_raw::DecodeError::Cancelled) => Err(None),
                    Err(e) => Err(Some(e.to_string())),
                };
                Some(WorkerMessage::Decoded { generation, full_resolution, result })
            });
        }
    }

    /// Handles finished work and returns what happened. Call once per frame.
    pub fn poll(&mut self) -> Vec<SessionEvent> {
        while let Ok(message) = self.receiver.try_recv() {
            self.handle(message);
        }
        if self.settle_deadline.is_some_and(|d| Instant::now() >= d) {
            self.settle_deadline = None;
            self.request_render(Level::Preview);
        }
        if let Some(deadline) = self.settle_deadline {
            self.repaint.request_repaint_after(deadline.saturating_duration_since(Instant::now()));
        }
        std::mem::take(&mut self.events)
    }

    fn handle(&mut self, message: WorkerMessage) {
        let current = self.current_generation();
        match message {
            WorkerMessage::Decoded { generation, full_resolution, result } => {
                if generation == current {
                    self.handle_decoded(full_resolution, result);
                }
            }
            WorkerMessage::Rendered { level, generation, version, image, histogram } => {
                self.slots[level as usize].busy = false;
                if generation == current {
                    self.handle_rendered(level, version, image, histogram);
                }
                if self.slots[level as usize].pending {
                    self.request_render(level);
                }
            }
            WorkerMessage::Before { level, generation, image } => {
                if generation == current {
                    self.events.push(match level {
                        Level::Full => SessionEvent::BeforeFullReady(image),
                        _ => SessionEvent::BeforePreviewReady(image),
                    });
                }
            }
            WorkerMessage::Overlay { generation, image } => {
                self.overlay_busy = false;
                // Show it even if the mask changed meanwhile (it is still the newest
                // available), then catch up.
                if generation == current && self.overlay_mask.is_some() {
                    self.events.push(SessionEvent::MaskOverlayReady(Some(image)));
                }
                if self.overlay_pending {
                    self.request_overlay();
                }
            }
            WorkerMessage::Exported { path, error } => {
                self.exports_running -= 1;
                self.events.push(SessionEvent::ExportFinished { path, error });
            }
        }
    }

    fn handle_decoded(&mut self, full_resolution: bool, result: Result<Decoded, Option<String>>) {
        let decoded = match result {
            Ok(decoded) => decoded,
            Err(None) => return, // cancelled
            Err(Some(message)) => {
                // Report once: from the half-size decode, or from the full one if the first
                // succeeded.
                if !full_resolution || self.is_loaded() {
                    let path = self.path.clone().unwrap_or_default();
                    self.events.push(SessionEvent::LoadFailed { path, message });
                }
                return;
            }
        };
        // A late half-size decode must not replace sources derived from the full decode.
        if !full_resolution && self.full_loaded {
            return;
        }
        let first_result = self.metadata.width == 0;
        self.draft_source = Some(decoded.draft);
        self.preview_source = Some(decoded.preview);
        self.metadata = decoded.metadata;
        if let Some(full) = decoded.full {
            self.metadata.width = full.width;
            self.metadata.height = full.height;
            self.full_source = Some(full);
            self.full_loaded = true;
        }
        // The app responds by calling set_edits() with the photo's saved edits; nothing is
        // rendered before that.
        if first_result {
            self.events.push(SessionEvent::MetadataReady(self.metadata.clone()));
        }

        self.request_render(Level::Preview);
        if full_resolution && self.full_needed {
            self.request_render(Level::Full);
        }
        if self.before_needed {
            self.render_before(Level::Preview);
            if full_resolution && self.full_needed {
                self.render_before(Level::Full);
            }
        }
    }

    /// Sets the edits to render. Until the first call for a photo, nothing is rendered.
    pub fn set_edits(&mut self, edits: &EditState, update: Update) {
        if self.edits_ready && *edits == self.edits {
            return;
        }
        self.edits = edits.clone();
        self.edits_ready = true;
        self.edit_version += 1;
        if self.full_version != 0 {
            self.full_version = 0;
            self.events.push(SessionEvent::FullImageInvalidated);
        }
        self.request_render(Level::Draft);
        self.request_overlay();
        match update {
            Update::Interactive => {
                self.settle_deadline = Some(Instant::now() + SETTLE_DELAY);
                self.repaint.request_repaint_after(SETTLE_DELAY);
            }
            Update::Immediate => {
                self.settle_deadline = None;
                self.request_render(Level::Preview);
            }
        }
    }

    pub fn set_full_resolution_needed(&mut self, needed: bool) {
        if needed == self.full_needed {
            return;
        }
        self.full_needed = needed;
        if needed && self.full_version != self.edit_version {
            self.request_render(Level::Full);
        }
        if needed && self.before_needed {
            self.render_before(Level::Full);
        }
    }

    /// The "before" (unedited) rendering is produced only while it is needed.
    pub fn set_before_needed(&mut self, needed: bool) {
        self.before_needed = needed;
        if needed {
            self.render_before(Level::Preview);
            if self.full_needed {
                self.render_before(Level::Full);
            }
        }
    }

    /// Shows the coverage of `edits.masks[index]` as an overlay. It follows the edits.
    pub fn set_mask_overlay(&mut self, index: Option<usize>) {
        if index == self.overlay_mask {
            return;
        }
        self.overlay_mask = index;
        self.request_overlay();
    }

    fn request_overlay(&mut self) {
        if self.overlay_busy {
            self.overlay_pending = true;
            return;
        }
        let mask: Option<Mask> = self.overlay_mask.and_then(|i| self.edits.masks.get(i).cloned());
        let (Some(mask), Some(source)) = (mask, &self.draft_source) else {
            self.events.push(SessionEvent::MaskOverlayReady(None));
            return;
        };
        self.overlay_busy = true;
        self.overlay_pending = false;
        let (width, height) = (source.width, source.height);
        let generation = self.current_generation();
        let tile = tile_size(&self.repaint);
        self.spawn(move || {
            let coverage = render_mask_coverage(&mask, width, height);
            let pixels: Vec<Color32> = coverage
                .iter()
                .map(|&c| {
                    let a = (f32::from(c) * OVERLAY_OPACITY + 0.5) as u32;
                    let [r, g, b] = OVERLAY_COLOR.map(|v| (u32::from(v) * a / 255) as u8);
                    Color32::from_rgba_premultiplied(r, g, b, a as u8)
                })
                .collect();
            Some(WorkerMessage::Overlay { generation, image: TiledImage::from_pixels(width, height, &pixels, tile) })
        });
    }

    fn source(&self, level: Level) -> Option<&Arc<ImageF>> {
        match level {
            Level::Draft => self.draft_source.as_ref(),
            Level::Preview => self.preview_source.as_ref(),
            Level::Full => self.full_source.as_ref(),
        }
    }

    fn render_before(&mut self, level: Level) {
        // The unedited photo does not change while editing; render each source only once.
        let Some(image) = self.source(level).cloned() else { return };
        if self.before_rendered[level as usize] == identity(&image) {
            return;
        }
        self.before_rendered[level as usize] = identity(&image);
        let as_shot = self.metadata.as_shot;
        let generation = self.current_generation();
        let tile = tile_size(&self.repaint);
        self.spawn(move || {
            let encoded = render(&image, &as_shot, &EditState::new(as_shot), &RenderOptions::default());
            Some(WorkerMessage::Before { level, generation, image: TiledImage::from_encoded(&encoded, tile) })
        });
    }

    fn request_render(&mut self, level: Level) {
        if !self.edits_ready {
            return;
        }
        let Some(source) = self.source(level).map(identity) else { return };
        let slot = &mut self.slots[level as usize];
        if !slot.busy {
            self.start_render(level);
        } else if slot.version != self.edit_version || slot.source != source {
            slot.pending = true; // re-render once the current one finishes
        }
    }

    fn start_render(&mut self, level: Level) {
        let Some(image) = self.source(level).cloned() else { return };
        let slot = &mut self.slots[level as usize];
        slot.busy = true;
        slot.pending = false;
        slot.version = self.edit_version;
        slot.source = identity(&image);

        let as_shot = self.metadata.as_shot;
        let edits = self.edits.clone();
        let generation = self.current_generation();
        let version = self.edit_version;
        let tile = tile_size(&self.repaint);
        self.spawn(move || {
            let encoded = render(&image, &as_shot, &edits, &RenderOptions::default());
            let histogram = (level != Level::Full).then(|| Box::new(Histogram::compute(&encoded)));
            Some(WorkerMessage::Rendered {
                level,
                generation,
                version,
                image: TiledImage::from_encoded(&encoded, tile),
                histogram,
            })
        });
    }

    fn handle_rendered(&mut self, level: Level, version: u64, image: TiledImage, histogram: Option<Box<Histogram>>) {
        if level == Level::Full {
            if version == self.edit_version {
                self.full_version = version;
                self.events.push(SessionEvent::FullImageReady(image));
            }
            return;
        }
        // Show a result unless something newer, or the same edits at higher quality, is on
        // screen.
        if version > self.shown_version || (version == self.shown_version && Some(level) >= self.shown_level) {
            self.shown_version = version;
            self.shown_level = Some(level);
            let full_size = [self.metadata.width, self.metadata.height];
            self.events.push(SessionEvent::PreviewReady { image, full_size, histogram: histogram.unwrap_or_default() });
        }
        if level == Level::Preview && version == self.edit_version && self.full_needed && self.full_version != version {
            self.request_render(Level::Full);
        }
    }

    pub fn auto_white_balance(&self) -> Option<WhiteBalance> {
        Some(estimate_white_balance(self.draft_source.as_ref()?, &self.metadata.as_shot))
    }

    /// Eyedropper at a normalised image position.
    pub fn white_balance_at(&self, x: f64, y: f64) -> Option<WhiteBalance> {
        sample_white_balance(self.preview_source.as_ref()?, x, y, &self.metadata.as_shot)
    }

    /// Exports the current edits. Export always uses the full-resolution decode of the
    /// original RAW.
    pub fn export_to(&mut self, output: PathBuf, settings: ExportSettings) {
        let source = self.full_source.clone();
        let Some(raw_path) = self.path.clone() else { return };
        let as_shot = self.metadata.as_shot;
        let edits = self.edits.clone();
        self.exports_running += 1;
        self.spawn(move || {
            let result = (|| -> Result<(), String> {
                let image = match source {
                    Some(image) => image,
                    None => {
                        Arc::new(decode(&raw_path, DecodeQuality::Full, &|| false).map_err(|e| e.to_string())?.image)
                    }
                };
                export_image(&image, &as_shot, &edits, &settings, &output).map_err(|e| e.to_string())
            })();
            Some(WorkerMessage::Exported { path: output, error: result.err() })
        });
    }
}

impl Drop for PhotoSession {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst); // cancel in-flight decodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_raw() -> Option<PathBuf> {
        std::env::var_os("IRIS_TEST_RAW").map(Into::into)
    }

    /// Polls until `done` returns true for an event, or panics after a timeout.
    fn wait_for(session: &mut PhotoSession, mut done: impl FnMut(&SessionEvent) -> bool) -> Vec<SessionEvent> {
        let start = Instant::now();
        let mut seen = Vec::new();
        while start.elapsed() < Duration::from_secs(60) {
            for event in session.poll() {
                let finished = done(&event);
                seen.push(event);
                if finished {
                    return seen;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("timed out");
    }

    #[test]
    fn loads_renders_and_exports() {
        let Some(raw) = test_raw() else {
            eprintln!("Set IRIS_TEST_RAW to a RAW file to run this test");
            return;
        };
        let mut session = PhotoSession::new(egui::Context::default());
        session.open(&raw);
        let events = wait_for(&mut session, |e| matches!(e, SessionEvent::MetadataReady(_)));
        let SessionEvent::MetadataReady(metadata) = events.last().unwrap() else { unreachable!() };
        assert!(metadata.width > 0);
        assert!(session.is_loaded());

        // Nothing renders until the edits are set; then a preview arrives.
        let mut edits = EditState::new(metadata.as_shot);
        edits.basic.exposure = 0.5;
        session.set_edits(&edits, Update::Immediate);
        wait_for(&mut session, |e| matches!(e, SessionEvent::PreviewReady { .. }));

        // Full resolution only when asked for.
        session.set_full_resolution_needed(true);
        wait_for(&mut session, |e| matches!(e, SessionEvent::FullImageReady(_)));

        // An interactive edit invalidates the full image and settles into a preview.
        edits.basic.exposure = 0.7;
        session.set_edits(&edits, Update::Interactive);
        let events = session.poll();
        assert!(events.iter().any(|e| matches!(e, SessionEvent::FullImageInvalidated)));

        // Mask overlay.
        edits.masks = vec![Mask::new(iris_core::MaskType::Radial, &[])];
        session.set_edits(&edits, Update::Immediate);
        session.set_mask_overlay(Some(0));
        wait_for(&mut session, |e| matches!(e, SessionEvent::MaskOverlayReady(Some(_))));

        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.jpg");
        session.export_to(out.clone(), ExportSettings { long_edge: 800, ..Default::default() });
        let events = wait_for(&mut session, |e| matches!(e, SessionEvent::ExportFinished { .. }));
        let Some(SessionEvent::ExportFinished { error, .. }) = events.last() else { unreachable!() };
        assert!(error.is_none(), "{error:?}");
        assert!(out.exists());
        assert_eq!(session.exports_running(), 0);
    }

    #[test]
    fn missing_file_reports_a_failure() {
        let mut session = PhotoSession::new(egui::Context::default());
        session.open(Path::new("/nonexistent/photo.ARW"));
        let events = wait_for(&mut session, |e| matches!(e, SessionEvent::LoadFailed { .. }));
        assert!(matches!(events.last(), Some(SessionEvent::LoadFailed { .. })));
        assert!(!session.is_loaded());
    }
}
