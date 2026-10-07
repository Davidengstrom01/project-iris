//! Modal dialogs. Each one is a small state machine drawn inside an [`egui::Modal`].

use std::path::{Path, PathBuf};

use egui::{RichText, Ui};
use iris_core::EditState;
use iris_export::{ExportFormat, ExportSettings};
use iris_persist::preset::{HSL_KEY, NOISE_REDUCTION_KEY, SHARPENING_KEY, TONE_CURVE_KEY};
use iris_persist::{Preset, PresetEntry};

use crate::settings::Settings;
use crate::theme;

/// What happens once unsaved edits have been saved or discarded.
#[derive(Clone, Debug)]
pub enum AfterSave {
    Open(PathBuf),
    /// Move the favorites into this folder.
    MoveFavorites(PathBuf),
    Quit,
}

pub enum Dialog {
    Message { title: String, text: String },
    Unsaved { then: AfterSave },
    QuitWhileExporting,
    Export(ExportDialog),
    SavePreset(SavePresetDialog),
    ReplacePreset { preset: Preset, folder: String },
    RenamePreset { entry: PresetEntry, name: String },
    MovePreset { entry: PresetEntry, folder: String, folders: Vec<String> },
    DeletePreset { entry: PresetEntry },
}

impl Dialog {
    pub fn message(title: &str, text: impl Into<String>) -> Self {
        Dialog::Message { title: title.into(), text: text.into() }
    }
}

pub fn heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(15.0).strong().color(theme::TEXT));
    ui.add_space(8.0);
}

/// OK / Cancel style buttons on the right; returns which was clicked (index into `labels`).
pub fn buttons(ui: &mut Ui, labels: &[(&str, bool)]) -> Option<usize> {
    ui.add_space(10.0);
    let mut clicked = None;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for (i, (label, enabled)) in labels.iter().enumerate().rev() {
            if ui.add_enabled(*enabled, egui::Button::new(*label).min_size(egui::vec2(72.0, 24.0))).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

// --- Export -------------------------------------------------------------------------

/// `path` with the extension of `format` (keeping .jpeg / .tiff spellings).
pub fn with_extension(path: &Path, format: ExportFormat) -> PathBuf {
    let ext = format.extension();
    let suffix = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let matches = suffix == ext
        || (format == ExportFormat::Jpeg && suffix == "jpeg")
        || (format == ExportFormat::Tiff && suffix == "tiff");
    if matches { path.to_owned() } else { path.with_extension(ext) }
}

pub struct ExportDialog {
    raw_path: PathBuf,
    pub format: ExportFormat,
    pub quality: u8,
    pub bits: u32,
    pub resize: bool,
    pub long_edge: usize,
    pub path: String,
    error: Option<String>,
    confirm_replace: bool,
    /// Exporting several photos (the favorites) into the folder in `path`.
    batch: Option<Vec<PathBuf>>,
}

pub enum ExportOutcome {
    Cancel,
    Browse,
    Export(PathBuf, ExportSettings),
    /// Into a folder: each photo and the file to write.
    ExportBatch(PathBuf, Vec<(PathBuf, PathBuf)>, ExportSettings),
}

impl ExportDialog {
    pub fn new(raw_path: &Path, settings: &Settings) -> Self {
        let format = settings.export_format();
        let path = with_extension(&raw_path.with_extension(""), format);
        Self {
            raw_path: raw_path.to_owned(),
            format,
            quality: settings.export_quality.clamp(1, 100),
            bits: if settings.export_bits == 16 { 16 } else { 8 },
            resize: settings.export_resize,
            long_edge: settings.export_long_edge.clamp(64, 20000),
            path: path.display().to_string(),
            error: None,
            confirm_replace: false,
            batch: None,
        }
    }

    /// Exports `photos` into a folder (by default "Export" next to them).
    pub fn new_batch(photos: Vec<PathBuf>, settings: &Settings) -> Self {
        let folder = photos.first().and_then(|p| p.parent()).map(|d| d.join("Export")).unwrap_or_default();
        let raw_path = photos.first().cloned().unwrap_or_default();
        Self { path: folder.display().to_string(), batch: Some(photos), ..Self::new(&raw_path, settings) }
    }

    pub fn is_batch(&self) -> bool {
        self.batch.is_some()
    }

    /// The batch's photos and the files they are written to. Photos that share a name
    /// (photo.ARW, photo.CR2) keep their extension in it so they do not overwrite each other.
    pub fn batch_outputs(&self) -> Vec<(PathBuf, PathBuf)> {
        let folder = self.output_path();
        let photos = self.batch.as_deref().unwrap_or_default();
        let stem = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        photos
            .iter()
            .map(|photo| {
                let shared = photos.iter().filter(|other| stem(other) == stem(photo)).count() > 1;
                let name = if shared {
                    let ext = photo.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
                    format!("{}_{ext}", stem(photo))
                } else {
                    stem(photo)
                };
                (photo.clone(), folder.join(name).with_extension(self.format.extension()))
            })
            .collect()
    }

    pub fn settings(&self) -> ExportSettings {
        ExportSettings {
            format: self.format,
            jpeg_quality: self.quality,
            long_edge: if self.resize { self.long_edge } else { 0 },
            bits_per_channel: self.bits,
        }
    }

    /// The extension always matches the export format, so the RAW file can never be the
    /// target.
    pub fn output_path(&self) -> PathBuf {
        let path = if self.is_batch() {
            PathBuf::from(self.path.trim())
        } else {
            with_extension(Path::new(self.path.trim()), self.format)
        };
        std::path::absolute(&path).unwrap_or(path)
    }

    pub fn set_path(&mut self, path: &Path) {
        self.path = if self.is_batch() {
            path.display().to_string()
        } else {
            with_extension(path, self.format).display().to_string()
        };
        self.confirm_replace = false;
    }

    fn accept_batch(&mut self) -> Option<ExportOutcome> {
        let folder = self.output_path();
        if self.path.trim().is_empty() {
            return None;
        }
        if let Err(e) = std::fs::create_dir_all(&folder) {
            self.error = Some(format!("Cannot create the folder {}: {e}", folder.display()));
            return None;
        }
        let outputs = self.batch_outputs();
        if outputs.iter().any(|(_, out)| out.exists()) && !self.confirm_replace {
            self.confirm_replace = true;
            return None;
        }
        Some(ExportOutcome::ExportBatch(folder, outputs, self.settings()))
    }

    fn accept(&mut self) -> Option<ExportOutcome> {
        if self.is_batch() {
            return self.accept_batch();
        }
        let path = self.output_path();
        if self.path.trim().is_empty() || path == self.raw_path {
            return None;
        }
        if !path.parent().is_some_and(Path::is_dir) {
            let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
            self.error = Some(format!("The folder {folder} does not exist."));
            return None;
        }
        if path.exists() && !self.confirm_replace {
            self.confirm_replace = true;
            return None;
        }
        Some(ExportOutcome::Export(path, self.settings()))
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<ExportOutcome> {
        match &self.batch {
            Some(photos) if photos.len() == 1 => heading(ui, "Export 1 Favorite"),
            Some(photos) => heading(ui, &format!("Export {} Favorites", photos.len())),
            None => heading(ui, "Export"),
        }
        if self.confirm_replace {
            if self.is_batch() {
                let existing = self.batch_outputs().iter().filter(|(_, out)| out.exists()).count();
                let files = if existing == 1 { "1 file" } else { &format!("{existing} files") };
                ui.label(format!(
                    "{files} with these names already exist in {}. Replace them?",
                    self.output_path().display()
                ));
            } else {
                let name = self.output_path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(format!("{name} already exists. Replace it?"));
            }
            return match buttons(ui, &[("Replace", true), ("Cancel", true)]) {
                Some(0) => self.accept(),
                Some(_) => {
                    self.confirm_replace = false;
                    None
                }
                None => None,
            };
        }

        let mut outcome = None;
        egui::Grid::new("export-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Format");
            let before = self.format;
            egui::ComboBox::from_id_salt("export-format")
                .selected_text(match self.format {
                    ExportFormat::Jpeg => "JPEG",
                    ExportFormat::Png => "PNG",
                    ExportFormat::Tiff => "TIFF",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.format, ExportFormat::Jpeg, "JPEG");
                    ui.selectable_value(&mut self.format, ExportFormat::Png, "PNG");
                    ui.selectable_value(&mut self.format, ExportFormat::Tiff, "TIFF");
                });
            if self.format != before && !self.is_batch() {
                self.path = with_extension(Path::new(&self.path), self.format).display().to_string();
            }
            ui.end_row();

            if self.format == ExportFormat::Jpeg {
                ui.label("Quality");
                ui.add(egui::Slider::new(&mut self.quality, 1..=100));
            } else {
                ui.label("Bit depth");
                egui::ComboBox::from_id_salt("export-bits")
                    .selected_text(format!("{} bits per channel", self.bits))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.bits, 8, "8 bits per channel");
                        ui.selectable_value(&mut self.bits, 16, "16 bits per channel");
                    });
            }
            ui.end_row();

            ui.label("Color space");
            ui.label("sRGB");
            ui.end_row();

            ui.label("Size");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.resize, false, "Original resolution");
                ui.add_space(12.0);
                ui.radio_value(&mut self.resize, true, "Long edge");
                ui.add_enabled(self.resize, egui::DragValue::new(&mut self.long_edge).range(64..=20000).suffix(" px"));
            });
            ui.end_row();

            ui.label(if self.is_batch() { "Folder" } else { "File" });
            ui.horizontal(|ui| {
                let edit = ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(320.0));
                if edit.changed() {
                    self.error = None;
                }
                if ui.button("Browse…").clicked() {
                    outcome = Some(ExportOutcome::Browse);
                }
            });
            ui.end_row();
        });
        if let Some(error) = &self.error {
            ui.add_space(6.0);
            ui.colored_label(egui::Color32::from_rgb(0xe0, 0x6c, 0x6c), error);
        }
        match buttons(ui, &[("Export", !self.path.trim().is_empty()), ("Cancel", true)]) {
            Some(0) => self.accept(),
            Some(_) => Some(ExportOutcome::Cancel),
            None => outcome,
        }
    }
}

// --- Save preset -------------------------------------------------------------------

/// The groups the user can include, and the keys each selects. Presets never include crop,
/// rotation, masks or other photo-specific settings.
const GROUPS: [(&str, &[&str]); 13] = [
    ("Exposure", &["exposure"]),
    ("Contrast", &["contrast"]),
    ("Highlights", &["highlights"]),
    ("Shadows", &["shadows"]),
    ("Whites", &["whites"]),
    ("Blacks", &["blacks"]),
    ("White Balance", &["temperature", "tint"]),
    ("Vibrance", &["vibrance"]),
    ("Saturation", &["saturation"]),
    ("Tone Curve", &[TONE_CURVE_KEY]),
    ("Color (HSL)", &[HSL_KEY]),
    ("Sharpening", &[SHARPENING_KEY]),
    ("Noise Reduction", &[NOISE_REDUCTION_KEY]),
];

/// Groups left out unless the user included them last time: white balance and detail
/// depend on the photo (light, camera, ISO) more than on the look.
const OFF_BY_DEFAULT: [&str; 3] = ["temperature", SHARPENING_KEY, NOISE_REDUCTION_KEY];

pub struct SavePresetDialog {
    edits: EditState,
    folders: Vec<String>,
    pub name: String,
    pub folder: String,
    include: [bool; GROUPS.len()],
}

pub enum SavePresetOutcome {
    Cancel,
    Save(Box<Preset>, String),
}

impl SavePresetDialog {
    pub fn new(edits: &EditState, folders: Vec<String>, settings: &Settings) -> Self {
        let include = std::array::from_fn(|i| {
            let key = GROUPS[i].1[0];
            settings.preset_include.get(key).copied().unwrap_or(!OFF_BY_DEFAULT.contains(&key))
        });
        Self { edits: edits.clone(), folders, name: String::new(), folder: settings.preset_folder.clone(), include }
    }

    /// Remembers the choices for next time.
    pub fn remember(&self, settings: &mut Settings) {
        settings.preset_folder = self.folder.trim().to_owned();
        for (i, (_, keys)) in GROUPS.iter().enumerate() {
            settings.preset_include.insert(keys[0].to_owned(), self.include[i]);
        }
    }

    pub fn preset(&self) -> Preset {
        let keys: Vec<&str> = GROUPS
            .iter()
            .zip(self.include)
            .filter(|(_, on)| *on)
            .flat_map(|((_, keys), _)| keys.iter().copied())
            .collect();
        Preset::from_edits(self.name.trim(), &self.edits, &keys)
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<SavePresetOutcome> {
        heading(ui, "Save Preset");
        egui::Grid::new("preset-form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut self.name).hint_text("e.g. Warm Film").desired_width(260.0))
                .request_focus_once(ui);
            ui.end_row();
            ui.label("Folder");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.folder).desired_width(220.0));
                ui.menu_button("▾", |ui| {
                    for folder in &self.folders {
                        if ui.button(folder).clicked() {
                            self.folder = folder.clone();
                            ui.close();
                        }
                    }
                });
            });
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Include").strong());
        egui::Grid::new("preset-include").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
            for (i, (name, _)) in GROUPS.iter().enumerate() {
                ui.checkbox(&mut self.include[i], *name);
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Check All").clicked() {
                self.include = [true; GROUPS.len()];
            }
            if ui.button("Check None").clicked() {
                self.include = [false; GROUPS.len()];
            }
        });
        let can_save = !self.name.trim().is_empty() && self.include.iter().any(|&on| on);
        match buttons(ui, &[("Save", can_save), ("Cancel", true)]) {
            Some(0) => Some(SavePresetOutcome::Save(Box::new(self.preset()), self.folder.trim().to_owned())),
            Some(_) => Some(SavePresetOutcome::Cancel),
            None => None,
        }
    }
}

/// Focus a text field the first time it is shown.
trait RequestFocusOnce {
    fn request_focus_once(self, ui: &Ui);
}

impl RequestFocusOnce for egui::Response {
    fn request_focus_once(self, ui: &Ui) {
        let id = self.id.with("focused-once");
        if !ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
            self.request_focus();
            ui.ctx().data_mut(|d| d.insert_temp(id, true));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_follow_the_format() {
        assert_eq!(with_extension(Path::new("/a/photo"), ExportFormat::Jpeg), Path::new("/a/photo.jpg"));
        assert_eq!(with_extension(Path::new("/a/photo.jpeg"), ExportFormat::Jpeg), Path::new("/a/photo.jpeg"));
        assert_eq!(with_extension(Path::new("/a/photo.jpg"), ExportFormat::Png), Path::new("/a/photo.png"));
        assert_eq!(with_extension(Path::new("/a/photo.ARW"), ExportFormat::Tiff), Path::new("/a/photo.tif"));
    }

    #[test]
    fn export_never_targets_the_raw_file() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("photo.ARW");
        std::fs::write(&raw, "raw").unwrap();
        let mut dialog = ExportDialog::new(&raw, &Settings::default());
        assert_eq!(dialog.output_path(), dir.path().join("photo.jpg"));
        dialog.path = raw.display().to_string();
        assert_eq!(dialog.output_path(), dir.path().join("photo.jpg")); // extension replaced

        // An existing file asks first.
        std::fs::write(dir.path().join("photo.jpg"), "old").unwrap();
        assert!(dialog.accept().is_none());
        assert!(dialog.confirm_replace);
        assert!(matches!(dialog.accept(), Some(ExportOutcome::Export(..))));

        // A missing folder is reported.
        dialog.path = dir.path().join("missing/out.jpg").display().to_string();
        dialog.confirm_replace = false;
        assert!(dialog.accept().is_none());
        assert!(dialog.error.is_some());
    }

    #[test]
    fn preset_dialog_selects_groups() {
        let mut edits = EditState::default();
        edits.basic.contrast = 12.0;
        let mut dialog = SavePresetDialog::new(&edits, vec![], &Settings::default());
        dialog.name = "  Mine ".into();
        let preset = dialog.preset();
        assert_eq!(preset.name, "Mine");
        assert!(!preset.values.contains_key("temperature")); // off by default
        assert!(preset.values.contains_key("contrast"));
        assert!(preset.tone_curve.is_some() && preset.hsl.is_some());
        assert!(preset.sharpening.is_none() && preset.noise_reduction.is_none()); // off by default

        let mut settings = Settings::default();
        dialog.include = [false; GROUPS.len()];
        dialog.include[6] = true; // white balance
        dialog.remember(&mut settings);
        let again = SavePresetDialog::new(&edits, vec![], &settings);
        assert!(again.include[6] && !again.include[0]);
    }
}
