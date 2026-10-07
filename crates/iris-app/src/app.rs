//! The main window: wires the photo session, the edit document, the panels and the view
//! together. All changes to the edits go through [`IrisApp::apply`].

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use egui::{Key, KeyboardShortcut, Modifiers, RichText, Ui, ViewportCommand};
use iris_core::edit_state::describe_change;
use iris_core::mask::MAX_MASKS;
use iris_core::{EditState, Mask, PhotoMetadata, WhiteBalance};
use iris_persist::PresetLibrary;
use iris_render::Histogram;

use crate::action::Action;
use crate::curve_editor::CurveEditor;
use crate::dialogs::{
    AfterSave, Dialog, ExportDialog, ExportOutcome, SavePresetDialog, SavePresetOutcome, buttons, heading,
};
use crate::document::EditDocument;
use crate::mask_editor::Tool;
use crate::panels::{self, HslProperty, Library, MaskPanel};
use crate::session::{PhotoSession, SessionEvent, Update};
use crate::settings::{STORAGE_KEY, Settings};
use crate::texture::{Magnify, TiledTexture};
use crate::theme;
use crate::view::{CompareMode, ImageView, ViewEvent};
use crate::widgets;

/// "\" toggles before/after on a tap and shows "before" only while held on a long press.
const HOLD_THRESHOLD: Duration = Duration::from_millis(350);

struct Status {
    text: String,
    until: Option<Instant>,
}

/// File dialogs run on their own thread so the window keeps drawing.
#[derive(Clone, Copy, Debug)]
enum FileRequest {
    OpenPhoto,
    SaveEditsAs,
    ExportTo,
}

/// A file dialog's answer: what it was for and the chosen path, if any.
type FileResult = (FileRequest, Option<PathBuf>);

pub struct IrisApp {
    ctx: egui::Context,
    session: PhotoSession,
    document: EditDocument,
    presets: PresetLibrary,
    settings: Settings,

    view: ImageView,
    curve_editor: CurveEditor,
    hsl_property: HslProperty,
    mask_panel: MaskPanel,
    library: Library,

    metadata: Option<PhotoMetadata>,
    histogram: Option<Box<Histogram>>,
    curve_histogram: Option<[f32; 256]>,
    export_enabled: bool,
    eyedropper: bool,
    /// Cropping on the photo (the view shows the whole straightened frame).
    cropping: bool,
    /// The selected mask is edited on the photo (None = normal viewing).
    selected_mask: Option<usize>,
    last_selected_mask: usize,
    /// The view and the overlay need to catch up with the selected mask.
    mask_sync: bool,
    panels_visible: bool,
    status: Option<Status>,
    size_label: String,
    dialog: Option<Dialog>,
    file_requests: (Sender<FileResult>, Receiver<FileResult>),
    file_dialog_open: bool,
    before_key: Option<(Instant, CompareMode)>,
    allow_close: bool,
    title: String,
}

impl IrisApp {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        Self::with_preset_directory(cc, open, PresetLibrary::default_user_directory())
    }

    /// Like [`Self::new`], with user presets in `preset_directory`.
    pub fn with_preset_directory(
        cc: &eframe::CreationContext<'_>,
        open: Option<PathBuf>,
        preset_directory: PathBuf,
    ) -> Self {
        theme::apply(&cc.egui_ctx);
        // Ctrl +/- zoom the photo, not the interface.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, STORAGE_KEY)).unwrap_or_default();
        let presets = PresetLibrary::new(preset_directory);
        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            session: PhotoSession::new(cc.egui_ctx.clone()),
            document: EditDocument::default(),
            presets,
            mask_panel: MaskPanel::new(settings.mask_overlay),
            settings,
            view: ImageView::default(),
            curve_editor: CurveEditor::default(),
            hsl_property: HslProperty::default(),
            library: Library::default(),
            metadata: None,
            histogram: None,
            curve_histogram: None,
            export_enabled: false,
            eyedropper: false,
            cropping: false,
            selected_mask: None,
            last_selected_mask: 0,
            mask_sync: false,
            panels_visible: true,
            status: None,
            size_label: String::new(),
            dialog: None,
            file_requests: channel(),
            file_dialog_open: false,
            before_key: None,
            allow_close: false,
            title: String::new(),
        };
        for warning in app.presets.warnings() {
            eprintln!("iris: skipped preset {warning}");
        }
        if let Some(path) = open {
            app.open_photo(&path);
        }
        app
    }

    fn show_status(&mut self, text: impl Into<String>, duration: Option<Duration>) {
        self.status = Some(Status { text: text.into(), until: duration.map(|d| Instant::now() + d) });
    }

    fn clear_status(&mut self) {
        self.status = None;
    }

    fn message(&mut self, title: &str, text: impl Into<String>) {
        self.dialog = Some(Dialog::message(title, text));
    }

    fn file_name(path: &Path) -> String {
        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    // --- Session ---------------------------------------------------------------------

    fn handle_session_events(&mut self, ctx: &egui::Context) {
        for event in self.session.poll() {
            match event {
                SessionEvent::MetadataReady(metadata) => {
                    // Restore saved edits before anything is rendered.
                    let path = self.session.path().map(Path::to_owned).unwrap_or_default();
                    let warning = self.document.load(&path, metadata.as_shot);
                    self.metadata = Some(metadata);
                    self.session.set_edits(self.document.edits(), Update::Immediate);
                    self.edits_changed();
                    if let Some(warning) = warning {
                        self.message(
                            "Saved edits",
                            format!("The saved edits for this photo could not be read and were ignored.\n{warning}"),
                        );
                    }
                }
                SessionEvent::PreviewReady { image, framing, histogram } => {
                    let first_preview = !self.export_enabled;
                    let texture = TiledTexture::upload(ctx, "preview", image, Magnify::Smooth);
                    self.view.set_preview(texture, framing);
                    self.export_enabled = true;
                    let [w, h] = framing.result_size;
                    self.size_label = format!("{w} × {h}");
                    if first_preview {
                        self.clear_status();
                    }
                    self.set_histogram(histogram);
                }
                SessionEvent::FullImageReady(image) => {
                    self.view.set_full_image(Some(TiledTexture::upload(ctx, "full", image, Magnify::Pixels)));
                }
                SessionEvent::FullImageInvalidated => self.view.set_full_image(None),
                SessionEvent::BeforePreviewReady(image) => {
                    self.view.set_before_preview(TiledTexture::upload(ctx, "before", image, Magnify::Smooth));
                }
                SessionEvent::BeforeFullReady(image) => {
                    self.view.set_before_full(TiledTexture::upload(ctx, "before-full", image, Magnify::Pixels));
                }
                SessionEvent::MaskOverlayReady(image) => {
                    if self.selected_mask.is_some() {
                        let texture = image.map(|i| TiledTexture::upload(ctx, "overlay", i, Magnify::Smooth));
                        self.view.set_mask_overlay(texture);
                    }
                }
                SessionEvent::LoadFailed { path, message } => {
                    let name = Self::file_name(&path);
                    self.view.set_load_failed(format!("Cannot open {name}\n{message}"));
                    self.export_enabled = false;
                    self.show_status(format!("Cannot open {name}"), Some(Duration::from_secs(8)));
                }
                SessionEvent::ExportFinished { path, error } => match error {
                    None => self.show_status(format!("Exported {}", path.display()), Some(Duration::from_secs(8))),
                    Some(error) => {
                        self.clear_status();
                        self.message("Export failed", error);
                    }
                },
            }
        }
    }

    fn set_histogram(&mut self, histogram: Box<Histogram>) {
        let luminance = &histogram.luminance;
        let peak = luminance[1..255].iter().copied().max().unwrap_or(0).max(1);
        self.curve_histogram =
            Some(std::array::from_fn(|i| (f64::from(luminance[i]) / f64::from(peak)).sqrt().min(1.0) as f32));
        self.histogram = Some(histogram);
    }

    // --- Opening and saving ----------------------------------------------------------

    fn open_photo(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        if self.session.path() == Some(path.as_path()) {
            return;
        }
        if self.document.is_dirty() {
            self.dialog = Some(Dialog::Unsaved { then: AfterSave::Open(path) });
            return;
        }
        self.set_compare_mode(CompareMode::Off);
        self.set_cropping(false);
        self.select_mask(None);
        self.document.clear();
        self.library.show_folder_of(&path);
        self.session.open(&path);
        self.view.begin_loading();
        self.set_eyedropper(false);
        self.histogram = None;
        self.curve_histogram = None;
        self.export_enabled = false;
        self.metadata = None;
        self.size_label.clear();
        self.show_status(format!("Decoding {}…", Self::file_name(&path)), None);
        self.settings.last_directory = path.parent().map(Path::to_owned);
    }

    fn after_save(&mut self, then: AfterSave, ctx: &egui::Context) {
        match then {
            AfterSave::Open(path) => {
                // The edits were saved or discarded; open without asking again.
                let raw = self.document.raw_path().map(Path::to_owned);
                self.document.clear();
                if let Some(raw) = raw {
                    self.library.show_folder_of(&raw);
                }
                self.open_photo(&path);
            }
            AfterSave::Quit => {
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn save(&mut self) {
        if !self.document.is_loaded() {
            return;
        }
        match self.document.save() {
            Ok(()) => {
                if let Some(raw) = self.document.raw_path() {
                    let raw = raw.to_owned();
                    self.library.set_edited(&raw, true);
                }
                let name = self.document.sidecar_path().map(|p| Self::file_name(&p)).unwrap_or_default();
                self.show_status(format!("Saved edits to {name}"), Some(Duration::from_secs(4)));
            }
            Err(e) => self.message("Save failed", format!("Cannot save edits:\n{e}")),
        }
    }

    fn request_file(&mut self, kind: FileRequest, ctx: &egui::Context) {
        if self.file_dialog_open {
            return;
        }
        let mut dialog = rfd::FileDialog::new();
        match kind {
            FileRequest::OpenPhoto => {
                if let Some(dir) = &self.settings.last_directory {
                    dialog = dialog.set_directory(dir);
                }
                let mut extensions: Vec<String> = iris_raw::RAW_FILE_EXTENSIONS.iter().map(|e| e.to_string()).collect();
                extensions.extend(iris_raw::RAW_FILE_EXTENSIONS.iter().map(|e| e.to_uppercase()));
                dialog = dialog
                    .set_title("Open RAW Photo")
                    .add_filter("RAW photos", &extensions)
                    .add_filter("All files", &["*"]);
            }
            FileRequest::SaveEditsAs => {
                let Some(sidecar) = self.document.sidecar_path() else { return };
                if let Some(dir) = sidecar.parent() {
                    dialog = dialog.set_directory(dir);
                }
                dialog = dialog
                    .set_title("Save Edits As")
                    .set_file_name(Self::file_name(&sidecar))
                    .add_filter("Project Iris edits", &["json"]);
            }
            FileRequest::ExportTo => {
                let Some(Dialog::Export(export)) = &self.dialog else { return };
                let path = export.output_path();
                if let Some(dir) = path.parent() {
                    dialog = dialog.set_directory(dir);
                }
                let (name, extensions): (&str, &[&str]) = match export.format {
                    iris_export::ExportFormat::Jpeg => ("JPEG images", &["jpg", "jpeg"]),
                    iris_export::ExportFormat::Png => ("PNG images", &["png"]),
                    iris_export::ExportFormat::Tiff => ("TIFF images", &["tif", "tiff"]),
                };
                // Overwrite confirmation happens in the export dialog.
                dialog =
                    dialog.set_title("Export As").set_file_name(Self::file_name(&path)).add_filter(name, extensions);
            }
        }
        self.file_dialog_open = true;
        let sender = self.file_requests.0.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let path = match kind {
                FileRequest::OpenPhoto => dialog.pick_file(),
                FileRequest::SaveEditsAs | FileRequest::ExportTo => dialog.save_file(),
            };
            let _ = sender.send((kind, path));
            ctx.request_repaint();
        });
    }

    fn handle_file_results(&mut self) {
        while let Ok((kind, path)) = self.file_requests.1.try_recv() {
            self.file_dialog_open = false;
            let Some(path) = path else { continue };
            match kind {
                FileRequest::OpenPhoto => self.open_photo(&path),
                FileRequest::SaveEditsAs => {
                    let mut path = path;
                    if !path.to_string_lossy().ends_with(".iris.json") {
                        let mut name = path.file_name().unwrap_or_default().to_os_string();
                        if path.extension().is_some_and(|e| e == "json") {
                            name = path.file_stem().unwrap_or_default().to_os_string();
                        }
                        name.push(".iris.json");
                        path.set_file_name(name);
                    }
                    match self.document.save_as(&path) {
                        Ok(()) => {
                            self.show_status(format!("Saved edits to {}", path.display()), Some(Duration::from_secs(4)))
                        }
                        Err(e) => self.message("Save failed", format!("Cannot save edits:\n{e}")),
                    }
                }
                FileRequest::ExportTo => {
                    if let Some(Dialog::Export(export)) = &mut self.dialog {
                        export.set_path(&path);
                    }
                }
            }
        }
    }

    // --- Editing ---------------------------------------------------------------------

    /// Records an edit made by anything but a drag (presets, white balance, reset).
    fn commit(&mut self, state: EditState, label: &str) {
        if !self.document.is_loaded() {
            return;
        }
        self.document.edit(state, label, false);
        self.session.set_edits(self.document.edits(), Update::Immediate);
        self.edits_changed();
    }

    /// Records one step of a drag (a slider, a curve point, a brush stroke).
    fn edit_interactive(&mut self, state: EditState, label: &str) {
        if !self.document.is_loaded() {
            return;
        }
        self.document.edit(state, label, true);
        self.session.set_edits(self.document.edits(), Update::Interactive);
        self.edits_changed();
    }

    fn edits_changed(&mut self) {
        let masks = &self.document.edits().masks;
        if self.selected_mask.is_some_and(|i| i >= masks.len()) {
            // e.g. undoing "Add Mask"
            self.selected_mask = None;
        }
        self.mask_panel.follow_selection(masks, self.selected_mask);
        self.mask_sync = true;
        if self.cropping {
            self.view.set_crop_tool(Some((self.document.edits().crop, self.photo_size())));
        }
    }

    fn undo(&mut self) {
        if self.document.undo() {
            self.session.set_edits(self.document.edits(), Update::Immediate);
            self.edits_changed();
        }
    }

    fn redo(&mut self) {
        if self.document.redo() {
            self.session.set_edits(self.document.edits(), Update::Immediate);
            self.edits_changed();
        }
    }

    fn apply_white_balance(&mut self, wb: Option<WhiteBalance>) {
        let Some(wb) = wb else { return };
        let mut state = self.document.edits().clone();
        state.basic.white_balance = wb;
        self.commit(state, "White Balance");
    }

    fn set_eyedropper(&mut self, active: bool) {
        if active && !self.session.is_loaded() {
            return;
        }
        self.eyedropper = active;
        self.view.set_pick_mode(active);
        if active {
            self.show_status("Click a neutral grey or white area (Esc to cancel)", None);
        } else if self
            .status
            .as_ref()
            .is_some_and(|s| s.text.starts_with("Click a neutral") || s.text.starts_with("That area"))
        {
            self.clear_status();
        }
    }

    fn set_compare_mode(&mut self, mode: CompareMode) {
        let mode = if self.session.is_loaded() { mode } else { CompareMode::Off };
        self.view.set_compare_mode(mode);
        self.session.set_before_needed(mode != CompareMode::Off);
    }

    // --- Masks -----------------------------------------------------------------------

    fn select_mask(&mut self, index: Option<usize>) {
        let masks = &self.document.edits().masks;
        self.selected_mask = index.filter(|&i| self.document.is_loaded() && i < masks.len());
        if let Some(i) = self.selected_mask {
            self.last_selected_mask = i;
        }
        self.mask_panel.follow_selection(masks, self.selected_mask);
        self.mask_sync = true;
        if self.selected_mask.is_some() {
            let text = if self.mask_panel.tool == Tool::Brush {
                "Paint on the photo. [ ] change the brush size, Space + drag pans, Esc when done."
            } else {
                "Drag the handles, or drag on the photo to draw the gradient again. Esc when done."
            };
            self.show_status(text, None);
        } else if self.status.as_ref().is_some_and(|s| s.until.is_none() && !s.text.starts_with("Decoding")) {
            self.clear_status();
        }
    }

    /// The view and the overlay follow the selected mask and the panel's tool.
    fn sync_mask_editing(&mut self) {
        if !std::mem::take(&mut self.mask_sync) {
            return;
        }
        let masks = &self.document.edits().masks;
        let mask = self.selected_mask.and_then(|i| masks.get(i));
        self.view.set_mask_tool(self.mask_panel.tool);
        self.view.set_brush(self.mask_panel.brush);
        self.view.set_mask(mask);
        let overlay = if mask.is_some() && self.mask_panel.overlay { self.selected_mask } else { None };
        self.session.set_mask_overlay(overlay);
        self.settings.mask_overlay = self.mask_panel.overlay;
    }

    fn add_mask(&mut self, mask_type: iris_core::MaskType) {
        if !self.document.is_loaded() {
            return;
        }
        self.set_cropping(false);
        let mut state = self.document.edits().clone();
        if state.masks.len() >= MAX_MASKS {
            self.show_status(format!("A photo can have at most {MAX_MASKS} masks."), Some(Duration::from_secs(4)));
            return;
        }
        state.masks.push(Mask::new(mask_type, &state.masks));
        let index = state.masks.len() - 1;
        self.commit(state, &format!("Add {} Mask", mask_type.name()));
        self.select_mask(Some(index));
    }

    fn delete_mask(&mut self, index: usize) {
        let mut state = self.document.edits().clone();
        if index >= state.masks.len() {
            return;
        }
        let name = state.masks.remove(index).name;
        self.selected_mask = None;
        self.commit(state, &format!("Delete {name}"));
        self.select_mask(None);
    }

    fn toggle_mask_editing(&mut self) {
        if !self.document.is_loaded() {
            return;
        }
        self.set_cropping(false);
        let count = self.document.edits().masks.len();
        if self.selected_mask.is_some() {
            self.select_mask(None);
        } else if count == 0 {
            self.add_mask(iris_core::MaskType::Brush);
        } else {
            self.select_mask(Some(self.last_selected_mask.min(count - 1)));
        }
    }

    // --- Crop ------------------------------------------------------------------------

    /// Full-resolution size of the photo before any crop.
    fn photo_size(&self) -> [usize; 2] {
        self.metadata.as_ref().map_or([0, 0], |m| [m.width, m.height])
    }

    fn set_cropping(&mut self, on: bool) {
        let on = on && self.document.is_loaded() && self.session.is_loaded();
        if on == self.cropping {
            return;
        }
        self.cropping = on;
        if on {
            self.select_mask(None);
            self.set_eyedropper(false);
            self.view.set_crop_tool(Some((self.document.edits().crop, self.photo_size())));
            self.show_status(
                "Drag the corners or edges, or drag inside to move. Enter or double-click when done.",
                None,
            );
        } else {
            self.view.set_crop_tool(None);
            if self.status.as_ref().is_some_and(|s| s.text.starts_with("Drag the corners")) {
                self.clear_status();
            }
        }
        self.session.set_whole_frame(on);
    }

    /// Records a new crop as one step (rotate, aspect, reset).
    fn commit_crop(&mut self, crop: iris_core::Crop, label: &str) {
        let mut state = self.document.edits().clone();
        state.crop = crop;
        self.commit(state, label);
    }

    fn set_crop_aspect(&mut self, aspect: crate::crop_tool::Aspect) {
        let [w, h] = self.photo_size();
        let crop = self.document.edits().crop;
        let value = aspect.value(&crop, [w, h]);
        let next = if value > 0.0 { crop.with_aspect(value, w, h) } else { iris_core::Crop { aspect: 0.0, ..crop } };
        self.commit_crop(next, "Crop Aspect");
    }

    fn swap_crop_aspect(&mut self) {
        let [w, h] = self.photo_size();
        let crop = self.document.edits().crop;
        let next = if crop.aspect > 0.0 {
            crop.with_aspect(1.0 / crop.aspect, w, h)
        } else {
            // Free: turn the rectangle's own shape around.
            iris_core::Crop { aspect: 0.0, ..crop.with_aspect(1.0 / crop.pixel_aspect(w, h), w, h) }
        };
        self.commit_crop(next, "Crop Aspect");
    }

    fn rotate_quarter(&mut self, clockwise: bool) {
        let [w, h] = self.photo_size();
        let crop = self.document.edits().crop.rotated_quarter(clockwise).constrained(w, h);
        self.commit_crop(crop, if clockwise { "Rotate Right" } else { "Rotate Left" });
    }

    /// Straightening keeps the crop's shape and makes it as large as fits the rotated photo.
    fn straighten(&mut self, angle: f32) {
        let [w, h] = self.photo_size();
        let crop = self.document.edits().crop;
        let turned = iris_core::Crop { angle, ..crop };
        let mut next = turned.with_aspect(crop.pixel_aspect(w, h), w, h);
        next.aspect = crop.aspect;
        let mut state = self.document.edits().clone();
        state.crop = next;
        self.edit_interactive(state, "Straighten");
    }

    /// Replaces the selected mask (a slider, brush stroke or handle drag).
    fn edit_selected_mask(&mut self, mask: Mask, label: &str) {
        let mut state = self.document.edits().clone();
        let Some(slot) = self.selected_mask.and_then(|i| state.masks.get_mut(i)) else { return };
        *slot = mask;
        self.edit_interactive(state, label);
    }

    // --- Actions ---------------------------------------------------------------------

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        let loaded = self.document.is_loaded();
        match action {
            Action::EditBasic(adjustments) => {
                let mut state = self.document.edits().clone();
                let label = describe_change(&state.basic, &adjustments);
                state.basic = adjustments;
                self.edit_interactive(state, label);
            }
            Action::ResetBasic => {
                let mut state = self.document.edits().clone();
                state.basic = self.document.defaults().basic;
                self.commit(state, "Reset Basic");
            }
            Action::SetWhiteBalance(wb) => self.apply_white_balance(Some(wb)),
            Action::AutoWhiteBalance => self.apply_white_balance(self.session.auto_white_balance()),
            Action::SetEyedropper(active) => self.set_eyedropper(active),
            Action::EditCurve(curve) => {
                let mut state = self.document.edits().clone();
                state.tone_curve = curve;
                self.edit_interactive(state, "Tone Curve");
            }
            Action::ChooseCurve(curve) => {
                let mut state = self.document.edits().clone();
                state.tone_curve = curve;
                self.commit(state, "Tone Curve Preset");
            }
            Action::EditHsl(hsl, label) => {
                let mut state = self.document.edits().clone();
                state.hsl = hsl;
                self.edit_interactive(state, &label);
            }
            Action::ResetHsl => {
                let mut state = self.document.edits().clone();
                state.hsl = Default::default();
                self.commit(state, "Reset Color");
            }
            Action::AddMask(mask_type) => self.add_mask(mask_type),
            Action::DeleteMask(index) => self.delete_mask(index),
            Action::SelectMask(index) => self.select_mask(index),
            Action::EditMask(mask, label) => self.edit_selected_mask(mask, &label),
            Action::MaskToolChanged => self.mask_sync = true,
            Action::ApplyPreset(preset) => {
                if loaded {
                    let state = preset.apply(self.document.edits());
                    self.commit(state, &format!("Preset: {}", preset.name));
                    self.show_status(format!("Applied preset “{}”", preset.name), Some(Duration::from_secs(4)));
                }
            }
            Action::ShowSavePreset => {
                if loaded {
                    let dialog =
                        SavePresetDialog::new(self.document.edits(), self.presets.user_folders(), &self.settings);
                    self.dialog = Some(Dialog::SavePreset(dialog));
                }
            }
            Action::RenamePreset(entry) => {
                let name = entry.preset.name.clone();
                self.dialog = Some(Dialog::RenamePreset { entry, name });
            }
            Action::MovePreset(entry) => {
                let folder = entry.folder.clone();
                self.dialog = Some(Dialog::MovePreset { entry, folder, folders: self.presets.user_folders() });
            }
            Action::DeletePreset(entry) => self.dialog = Some(Dialog::DeletePreset { entry }),
            Action::OpenPhoto(path) => self.open_photo(&path),
            Action::ShowOpen => self.request_file(FileRequest::OpenPhoto, ctx),
            Action::Save => self.save(),
            Action::SaveAs => {
                if loaded {
                    self.request_file(FileRequest::SaveEditsAs, ctx);
                }
            }
            Action::ShowExport => {
                if self.export_enabled
                    && let Some(path) = self.session.path()
                {
                    self.dialog = Some(Dialog::Export(ExportDialog::new(path, &self.settings)));
                }
            }
            Action::Undo => self.undo(),
            Action::Redo => self.redo(),
            Action::Reset => {
                if loaded {
                    let defaults = self.document.defaults().clone();
                    self.commit(defaults, "Reset");
                }
            }
            Action::ToggleBefore => {
                let mode = if self.view.compare_mode() == CompareMode::Before {
                    CompareMode::Off
                } else {
                    CompareMode::Before
                };
                self.set_compare_mode(mode);
            }
            Action::ToggleSplit => {
                let mode =
                    if self.view.compare_mode() == CompareMode::Split { CompareMode::Off } else { CompareMode::Split };
                self.set_compare_mode(mode);
            }
            Action::Fit => self.view.fit_to_window(),
            Action::ActualSize => self.view.zoom_to_actual_pixels(),
            Action::ZoomIn => self.view.zoom_in(),
            Action::ZoomOut => self.view.zoom_out(),
            Action::ToggleMasks => self.toggle_mask_editing(),
            Action::ToggleOverlay => {
                if self.selected_mask.is_some() {
                    self.mask_panel.overlay = !self.mask_panel.overlay;
                    self.mask_sync = true;
                }
            }
            Action::ToggleCrop => self.set_cropping(!self.cropping),
            Action::Confirm => self.set_cropping(false),
            Action::SetCropAspect(aspect) => {
                if loaded {
                    self.set_crop_aspect(aspect);
                }
            }
            Action::SwapCropAspect => {
                if self.cropping {
                    self.swap_crop_aspect();
                }
            }
            Action::RotateQuarter(clockwise) => {
                if self.session.is_loaded() {
                    self.rotate_quarter(clockwise);
                }
            }
            Action::Straighten(angle) => {
                if self.session.is_loaded() {
                    self.straighten(angle);
                }
            }
            Action::ResetCrop => {
                if loaded {
                    self.commit_crop(iris_core::Crop::default(), "Reset Crop");
                }
            }
            Action::Escape => {
                if self.cropping {
                    self.set_cropping(false);
                } else if self.view.is_picking() {
                    self.set_eyedropper(false); // Esc cancels the eyedropper first
                } else {
                    self.select_mask(None);
                }
            }
            Action::BrushSize(steps) => {
                if self.selected_mask.is_some() {
                    self.mask_panel.step_brush_size(steps);
                    self.mask_sync = true;
                }
            }
            Action::TogglePanels => self.panels_visible = !self.panels_visible,
            Action::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
        }
    }

    fn apply_view_event(&mut self, event: ViewEvent) {
        match event {
            ViewEvent::PointPicked(x, y) => match self.session.white_balance_at(x, y) {
                None => self.show_status("That area is too dark or clipped. Pick a neutral, well-exposed area.", None),
                Some(wb) => {
                    self.set_eyedropper(false);
                    self.apply_white_balance(Some(wb));
                }
            },
            ViewEvent::PickCancelled => self.set_eyedropper(false),
            ViewEvent::MaskGestureStarted => self.document.begin_gesture(),
            ViewEvent::MaskEdited { mask, label } => self.edit_selected_mask(mask, label),
            ViewEvent::MaskGestureFinished => self.document.end_gesture(),
            ViewEvent::CropGestureStarted => self.document.begin_gesture(),
            ViewEvent::CropEdited(crop) => {
                let mut state = self.document.edits().clone();
                state.crop = crop;
                self.edit_interactive(state, "Crop");
            }
            ViewEvent::CropGestureFinished => self.document.end_gesture(),
            ViewEvent::CropDone => self.set_cropping(false),
        }
    }

    // --- Keyboard --------------------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if self.dialog.is_some() {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let command = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let command_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
        ctx.input_mut(|i| {
            // Most specific first: Ctrl+Shift+S before Ctrl+S.
            let with_modifiers = [
                (command_shift(Key::S), Action::SaveAs),
                (command_shift(Key::Z), Action::Redo),
                (command_shift(Key::R), Action::Reset),
                (command(Key::O), Action::ShowOpen),
                (command(Key::S), Action::Save),
                (command(Key::E), Action::ShowExport),
                (command(Key::Z), Action::Undo),
                (command(Key::Y), Action::Redo),
                (command(Key::Plus), Action::ZoomIn),
                (command(Key::Equals), Action::ZoomIn),
                (command(Key::Minus), Action::ZoomOut),
                (command(Key::Q), Action::Quit),
                (command(Key::OpenBracket), Action::RotateQuarter(false)),
                (command(Key::CloseBracket), Action::RotateQuarter(true)),
            ];
            for (shortcut, action) in with_modifiers {
                if i.consume_shortcut(&shortcut) {
                    actions.push(action);
                }
            }
            if typing {
                return;
            }
            let plain = [
                (Key::Num1, Action::Fit),
                (Key::Num2, Action::ActualSize),
                (Key::Y, Action::ToggleSplit),
                (Key::M, Action::ToggleMasks),
                (Key::O, Action::ToggleOverlay),
                (Key::Escape, Action::Escape),
                (Key::R, Action::ToggleCrop),
                (Key::Enter, Action::Confirm),
                (Key::X, Action::SwapCropAspect),
                (Key::OpenBracket, Action::BrushSize(-1)),
                (Key::CloseBracket, Action::BrushSize(1)),
                (Key::Tab, Action::TogglePanels),
            ];
            for (key, action) in plain {
                if i.consume_key(Modifiers::NONE, key) {
                    actions.push(action);
                }
            }
        });
        if !typing && self.session.is_loaded() {
            self.before_after_key(ctx);
        }
    }

    /// "\" toggles before/after; holding it only peeks. Some layouts (e.g. Swedish) produce
    /// "\" with AltGr, so the typed text counts too.
    fn before_after_key(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|i| i.events.clone());
        let mut saw_key = false;
        for event in &events {
            match event {
                egui::Event::Key { key: Key::Backslash, pressed, repeat: false, .. } => {
                    saw_key = true;
                    if *pressed {
                        let before = self.view.compare_mode();
                        self.before_key = Some((Instant::now(), before));
                        self.set_compare_mode(if before == CompareMode::Before {
                            CompareMode::Off
                        } else {
                            CompareMode::Before
                        });
                    } else if let Some((pressed_at, before)) = self.before_key.take()
                        && pressed_at.elapsed() > HOLD_THRESHOLD
                    {
                        self.set_compare_mode(before); // held: peek only
                    }
                }
                egui::Event::Text(text) if text == "\\" && !saw_key => {
                    let before = self.view.compare_mode();
                    self.set_compare_mode(if before == CompareMode::Before {
                        CompareMode::Off
                    } else {
                        CompareMode::Before
                    });
                }
                _ => {}
            }
        }
    }

    // --- Layout ----------------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let loaded = self.document.is_loaded();
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(12.0, 5.0);
            ui.add_space(4.0);
            ui.label(
                RichText::new("Project Iris").size(14.0).strong().color(egui::Color32::from_rgb(0xf0, 0xf0, 0xf2)),
            );
            ui.add_space(14.0);
            let mut button = |ui: &mut Ui, text: &str, enabled: bool, selected: bool, tip: String, action: Action| {
                let b = egui::Button::selectable(selected, text).frame_when_inactive(false);
                if ui.add_enabled(enabled, b).on_hover_text(tip).clicked() {
                    actions.push(action);
                }
            };
            button(ui, "Open", true, false, "Open a RAW photo (Ctrl+O)".into(), Action::ShowOpen);
            button(ui, "Save", loaded, false, "Save edits next to the photo (Ctrl+S)".into(), Action::Save);
            button(
                ui,
                "Export",
                self.export_enabled,
                false,
                "Export to JPEG, PNG or TIFF (Ctrl+E)".into(),
                Action::ShowExport,
            );
            ui.separator();
            let undo_tip = if self.document.can_undo() {
                format!("Undo {} (Ctrl+Z)", self.document.undo_label())
            } else {
                "Undo (Ctrl+Z)".into()
            };
            let redo_tip = if self.document.can_redo() {
                format!("Redo {} (Ctrl+Shift+Z)", self.document.redo_label())
            } else {
                "Redo (Ctrl+Shift+Z)".into()
            };
            button(ui, "Undo", self.document.can_undo(), false, undo_tip, Action::Undo);
            button(ui, "Redo", self.document.can_redo(), false, redo_tip, Action::Redo);
            ui.separator();
            let compare = self.view.compare_mode();
            let has_photo = self.session.is_loaded();
            button(
                ui,
                "Before / After",
                has_photo,
                compare == CompareMode::Before,
                "Show the original photo (\\ toggles, hold to peek)".into(),
                Action::ToggleBefore,
            );
            button(
                ui,
                "Split",
                has_photo,
                compare == CompareMode::Split,
                "Side-by-side before/after (Y)".into(),
                Action::ToggleSplit,
            );
            ui.separator();
            button(ui, "Crop", has_photo && loaded, self.cropping, "Crop and rotate (R)".into(), Action::ToggleCrop);
            button(ui, "Masks", loaded, self.selected_mask.is_some(), "Edit masks (M)".into(), Action::ToggleMasks);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                button(ui, "100%", has_photo, false, "View at 100% (2)".into(), Action::ActualSize);
                button(ui, "Fit", has_photo, false, "Fit image to window (1)".into(), Action::Fit);
            });
        });
    }

    fn right_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        egui::ScrollArea::vertical().id_salt("develop").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(6.0);
            widgets::histogram(ui, self.histogram.as_deref());
            let loaded = self.document.is_loaded();
            ui.add_enabled_ui(loaded, |ui| {
                panels::presets(ui, &self.presets, actions);
                widgets::separator(ui);
                let crop = self.document.edits().crop;
                panels::crop(ui, &crop, self.photo_size(), self.cropping, actions);
                widgets::separator(ui);
                let edits = self.document.edits().clone();
                let as_shot = self.document.defaults().basic.white_balance;
                panels::develop(ui, &edits.basic, as_shot, self.eyedropper, actions);
                widgets::separator(ui);
                panels::tone_curve(
                    ui,
                    &edits.tone_curve,
                    &mut self.curve_editor,
                    self.curve_histogram.as_ref(),
                    actions,
                );
                widgets::separator(ui);
                panels::hsl(ui, &edits.hsl, &mut self.hsl_property, actions);
                widgets::separator(ui);
                self.mask_panel.ui(ui, &edits.masks, self.selected_mask, actions);
            });
            widgets::separator(ui);
            let name = self.session.path().map(Self::file_name).unwrap_or_default();
            panels::info(ui, self.metadata.as_ref(), &name);
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        if self.status.as_ref().is_some_and(|s| s.until.is_some_and(|t| Instant::now() >= t)) {
            self.status = None;
        }
        if let Some(until) = self.status.as_ref().and_then(|s| s.until) {
            ui.ctx().request_repaint_after(until.saturating_duration_since(Instant::now()));
        }
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(self.view.zoom_label()).color(theme::TITLE_TEXT));
            if let Some(status) = &self.status {
                ui.add_space(16.0);
                ui.label(RichText::new(&status.text).color(theme::TITLE_TEXT));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new(&self.size_label).color(theme::TITLE_TEXT));
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let mut then: Option<AfterSave> = None;
        let modal = egui::Modal::new(egui::Id::new("dialog")).show(ctx, |ui| {
            ui.set_max_width(520.0);
            match &mut dialog {
                Dialog::Message { title, text } => {
                    heading(ui, title);
                    ui.label(text.as_str());
                    keep = buttons(ui, &[("OK", true)]).is_none();
                }
                Dialog::Unsaved { then: after } => {
                    heading(ui, "Unsaved edits");
                    let name = self.document.raw_path().map(Self::file_name).unwrap_or_default();
                    ui.label(format!("Save the edits to {name} before continuing?"));
                    match buttons(ui, &[("Save", true), ("Discard", true), ("Cancel", true)]) {
                        Some(0) => {
                            keep = false;
                            match self.document.save() {
                                Ok(()) => {
                                    if let Some(raw) = self.document.raw_path().map(Path::to_owned) {
                                        self.library.set_edited(&raw, true);
                                    }
                                    then = Some(after.clone());
                                }
                                Err(e) => self.message("Save failed", e),
                            }
                        }
                        Some(1) => {
                            keep = false;
                            then = Some(after.clone());
                        }
                        Some(_) => {
                            keep = false;
                            // Keep the current photo selected in the library.
                            if let Some(raw) = self.document.raw_path().map(Path::to_owned) {
                                self.library.show_folder_of(&raw);
                            }
                        }
                        None => {}
                    }
                }
                Dialog::QuitWhileExporting => {
                    heading(ui, "Export in progress");
                    ui.label("An export is still running. Quit anyway?");
                    match buttons(ui, &[("Quit", true), ("Cancel", true)]) {
                        Some(0) => {
                            keep = false;
                            if self.document.is_dirty() {
                                self.dialog = Some(Dialog::Unsaved { then: AfterSave::Quit });
                            } else {
                                then = Some(AfterSave::Quit);
                            }
                        }
                        Some(_) => keep = false,
                        None => {}
                    }
                }
                Dialog::Export(export) => match export.ui(ui) {
                    Some(ExportOutcome::Export(path, settings)) => {
                        keep = false;
                        self.settings.remember_export(&settings, export.resize, export.long_edge);
                        self.show_status(format!("Exporting {}…", Self::file_name(&path)), None);
                        self.session.export_to(path, settings);
                    }
                    Some(ExportOutcome::Browse) => self.request_file(FileRequest::ExportTo, ctx),
                    Some(ExportOutcome::Cancel) => keep = false,
                    None => {}
                },
                Dialog::SavePreset(save) => match save.ui(ui) {
                    Some(SavePresetOutcome::Save(preset, folder)) => {
                        let preset = *preset;
                        keep = false;
                        save.remember(&mut self.settings);
                        let exists = self
                            .presets
                            .presets()
                            .iter()
                            .any(|e| !e.built_in && e.folder == folder && e.preset.name == preset.name);
                        if exists {
                            self.dialog = Some(Dialog::ReplacePreset { preset, folder });
                        } else {
                            self.save_preset(&preset, &folder);
                        }
                    }
                    Some(SavePresetOutcome::Cancel) => keep = false,
                    None => {}
                },
                Dialog::ReplacePreset { preset, folder } => {
                    heading(ui, "Save Preset");
                    ui.label(format!("A preset named “{}” already exists in {folder}. Replace it?", preset.name));
                    match buttons(ui, &[("Replace", true), ("Cancel", true)]) {
                        Some(0) => {
                            keep = false;
                            let (preset, folder) = (preset.clone(), folder.clone());
                            self.save_preset(&preset, &folder);
                        }
                        Some(_) => keep = false,
                        None => {}
                    }
                }
                Dialog::RenamePreset { entry, name } => {
                    heading(ui, "Rename Preset");
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(name);
                    });
                    match buttons(ui, &[("Rename", !name.trim().is_empty()), ("Cancel", true)]) {
                        Some(0) => {
                            keep = false;
                            if name.trim() != entry.preset.name
                                && let Err(e) = self.presets.rename(entry, name)
                            {
                                self.message("Presets", e.to_string());
                            }
                        }
                        Some(_) => keep = false,
                        None => {}
                    }
                }
                Dialog::MovePreset { entry, folder, folders } => {
                    heading(ui, "Move Preset");
                    ui.horizontal(|ui| {
                        ui.label("Folder:");
                        ui.text_edit_singleline(folder);
                        ui.menu_button("▾", |ui| {
                            for f in folders.iter() {
                                if ui.button(f).clicked() {
                                    *folder = f.clone();
                                    ui.close();
                                }
                            }
                        });
                    });
                    match buttons(ui, &[("Move", true), ("Cancel", true)]) {
                        Some(0) => {
                            keep = false;
                            if let Err(e) = self.presets.move_to(entry, folder) {
                                self.message("Presets", e.to_string());
                            }
                        }
                        Some(_) => keep = false,
                        None => {}
                    }
                }
                Dialog::DeletePreset { entry } => {
                    heading(ui, "Delete Preset");
                    ui.label(format!("Delete the preset “{}”?", entry.preset.name));
                    match buttons(ui, &[("Delete", true), ("Cancel", true)]) {
                        Some(0) => {
                            keep = false;
                            if let Err(e) = self.presets.remove(entry) {
                                self.message("Presets", e.to_string());
                            }
                        }
                        Some(_) => keep = false,
                        None => {}
                    }
                }
            }
        });
        if modal.should_close() && !matches!(dialog, Dialog::Unsaved { .. }) {
            keep = false;
        }
        // A dialog may have opened another one (e.g. a failed save shows a message).
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
        if let Some(then) = then {
            self.after_save(then, ctx);
        }
    }

    fn save_preset(&mut self, preset: &iris_persist::Preset, folder: &str) {
        match self.presets.save(preset, folder) {
            Ok(()) => self.show_status(format!("Saved preset “{}”", preset.name), Some(Duration::from_secs(4))),
            Err(e) => self.message("Save Preset", e.to_string()),
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        if self.session.exports_running() > 0 {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::QuitWhileExporting);
        } else if self.document.is_dirty() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::Unsaved { then: AfterSave::Quit });
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.session.path() {
            Some(path) => {
                let dirty = if self.document.is_dirty() { " •" } else { "" };
                format!("{}{dirty} — Project Iris", Self::file_name(path))
            }
            None => "Project Iris".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn dropped_files(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned()));
        if let Some(path) = dropped.filter(|p| iris_raw::is_raw_file(p)) {
            actions.push(Action::OpenPhoto(path));
        }
    }
}

#[cfg(test)]
impl IrisApp {
    pub fn view(&self) -> &ImageView {
        &self.view
    }
    pub fn test_loaded(&self) -> bool {
        self.document.is_loaded() && self.export_enabled
    }
    pub fn test_edits(&self) -> &EditState {
        self.document.edits()
    }
    pub fn test_apply(&mut self, action: Action) {
        let ctx = self.ctx.clone();
        self.apply(action, &ctx);
    }
    pub fn test_title(&self) -> &str {
        &self.title
    }
    pub fn test_has_overlay(&self) -> bool {
        self.view.has_mask_overlay()
    }
    pub fn test_selected_mask(&self) -> Option<usize> {
        self.selected_mask
    }
    pub fn test_dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }
    pub fn test_dialog_mut(&mut self) -> Option<&mut Dialog> {
        self.dialog.as_mut()
    }
    pub fn test_exports_running(&self) -> usize {
        self.session.exports_running()
    }
    pub fn test_full_size(&self) -> [usize; 2] {
        self.metadata.as_ref().map_or([0, 0], |m| [m.width, m.height])
    }
    pub fn test_cropping(&self) -> bool {
        self.cropping
    }
    pub fn test_size_label(&self) -> &str {
        &self.size_label
    }
    pub fn test_path(&self) -> PathBuf {
        self.session.path().map(Path::to_owned).unwrap_or_default()
    }
}

impl eframe::App for IrisApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = self.ctx.clone();
        self.handle_session_events(&ctx);
        self.handle_file_results();
        self.handle_close_request(&ctx);

        let mut actions = Vec::new();
        self.shortcuts(&ctx, &mut actions);
        self.dropped_files(&ctx, &mut actions);

        let bar_frame = egui::Frame::NONE.fill(theme::PANEL).inner_margin(egui::Margin::symmetric(8, 4));
        egui::Panel::top("toolbar").frame(bar_frame).show(ui, |ui| self.toolbar(ui, &mut actions));
        egui::Panel::bottom("status").frame(bar_frame).show(ui, |ui| self.status_bar(ui));
        if self.panels_visible {
            let side = egui::Frame::NONE.fill(theme::PANEL);
            egui::Panel::left("library").frame(side).resizable(true).default_size(190.0).min_size(150.0).show(
                ui,
                |ui| {
                    let current = self.session.path().map(Path::to_owned);
                    self.library.ui(ui, current.as_deref(), &mut actions);
                },
            );
            egui::Panel::right("develop").frame(side).resizable(true).default_size(330.0).min_size(310.0).show(
                ui,
                |ui| {
                    self.right_panel(ui, &mut actions);
                },
            );
        }
        let space_held = !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_down(Key::Space));
        let view_events = egui::CentralPanel::no_frame().show(ui, |ui| self.view.ui(ui, space_held)).inner;

        for action in actions {
            self.apply(action, &ctx);
        }
        for event in view_events {
            self.apply_view_event(event);
        }
        self.sync_mask_editing();
        self.session.set_full_resolution_needed(self.view.needs_full_resolution());
        self.dialogs(&ctx);
        self.update_title(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.settings);
    }
}
