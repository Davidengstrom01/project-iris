//! Preferences remembered between runs (stored by eframe).

use std::collections::BTreeMap;
use std::path::PathBuf;

use iris_export::{ExportFormat, ExportSettings};
use serde::{Deserialize, Serialize};

pub const STORAGE_KEY: &str = "settings";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub last_directory: Option<PathBuf>,
    /// "jpeg", "png" or "tiff".
    pub export_format: String,
    pub export_quality: u8,
    pub export_bits: u32,
    pub export_resize: bool,
    pub export_long_edge: usize,
    pub preset_folder: String,
    /// Which groups the Save Preset dialog includes, by their first key.
    pub preset_include: BTreeMap<String, bool>,
    pub mask_overlay: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            last_directory: None,
            export_format: "jpeg".into(),
            export_quality: 92,
            export_bits: 8,
            export_resize: false,
            export_long_edge: 2048,
            preset_folder: iris_persist::library::DEFAULT_USER_FOLDER.into(),
            preset_include: BTreeMap::new(),
            mask_overlay: true,
        }
    }
}

impl Settings {
    pub fn export_format(&self) -> ExportFormat {
        match self.export_format.as_str() {
            "png" => ExportFormat::Png,
            "tiff" => ExportFormat::Tiff,
            _ => ExportFormat::Jpeg,
        }
    }

    pub fn remember_export(&mut self, settings: &ExportSettings, resize: bool, long_edge: usize) {
        self.export_format = match settings.format {
            ExportFormat::Jpeg => "jpeg",
            ExportFormat::Png => "png",
            ExportFormat::Tiff => "tiff",
        }
        .into();
        self.export_quality = settings.jpeg_quality;
        self.export_bits = settings.bits_per_channel;
        self.export_resize = resize;
        self.export_long_edge = long_edge;
    }
}
