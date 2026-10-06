use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};

use crate::preset::Preset;
use crate::{Error, read_json_object, write_atomically};

/// The built-in presets: ordinary preset files, bundled into the binary.
static BUILT_IN: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../presets/builtin");

pub const BUILT_IN_FOLDER: &str = "Built-in";
pub const DEFAULT_USER_FOLDER: &str = "My Presets";

#[derive(Clone, Debug, PartialEq)]
pub struct PresetEntry {
    pub preset: Preset,
    /// Display folder ("Built-in", "My Presets", ...).
    pub folder: String,
    /// JSON file the preset was loaded from (relative to the bundle for built-ins).
    pub file_path: PathBuf,
    pub built_in: bool,
}

/// Presets on disk. Built-in presets are bundled with the application; user presets
/// live in `<user_directory>/<folder>/<name>.json`.
pub struct PresetLibrary {
    user_directory: PathBuf,
    presets: Vec<PresetEntry>,
    warnings: Vec<String>,
}

/// "Warm Film" -> "warm-film"
fn slug(name: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if dash && !s.is_empty() {
                s.push('-');
            }
            dash = false;
            s.push(c);
        } else {
            dash = true;
        }
    }
    if s.is_empty() { "preset".into() } else { s }
}

/// Folder names become directory names: keep them to one safe path component.
fn clean_folder(folder: &str) -> String {
    let f = folder.trim().replace(['/', '\\'], "-");
    let f = f.trim_start_matches(|c: char| c == '.' || c.is_whitespace()); // no hidden folders, "." or ".."
    if f.is_empty() || f == BUILT_IN_FOLDER { DEFAULT_USER_FOLDER.into() } else { f.into() }
}

fn sorted_dir_entries(dir: &Path, want_dirs: bool) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() == want_dirs)
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    entries.sort();
    entries
}

impl PresetLibrary {
    pub fn new(user_directory: impl Into<PathBuf>) -> Self {
        let mut library = Self { user_directory: user_directory.into(), presets: Vec::new(), warnings: Vec::new() };
        library.reload();
        library
    }

    /// `$XDG_DATA_HOME/project-iris/iris/presets`, the location the Qt version used.
    pub fn default_user_directory() -> PathBuf {
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."));
        data.join("project-iris/iris/presets")
    }

    pub fn presets(&self) -> &[PresetEntry] {
        &self.presets
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Re-reads all presets. Unreadable files are skipped and reported by [`Self::warnings`].
    pub fn reload(&mut self) {
        self.presets.clear();
        self.warnings.clear();

        let mut built_in: Vec<_> =
            BUILT_IN.files().filter(|f| f.path().extension().is_some_and(|e| e == "json")).collect();
        built_in.sort_by_key(|f| f.path());
        for file in built_in {
            let parsed = serde_json::from_slice::<serde_json::Value>(file.contents())
                .map_err(Error::from)
                .and_then(|v| v.as_object().cloned().ok_or_else(|| Error::Invalid("not a JSON object".into())))
                .and_then(|o| Preset::from_json(&o));
            match parsed {
                Ok(preset) => self.presets.push(PresetEntry {
                    preset,
                    folder: BUILT_IN_FOLDER.into(),
                    file_path: file.path().to_owned(),
                    built_in: true,
                }),
                Err(e) => self.warnings.push(format!("{}: {e}", file.path().display())),
            }
        }

        for dir in sorted_dir_entries(&self.user_directory, true) {
            let folder = dir.file_name().unwrap_or_default().to_string_lossy().into_owned();
            for path in sorted_dir_entries(&dir, false) {
                if path.extension().is_none_or(|e| e != "json") {
                    continue;
                }
                match read_json_object(&path).and_then(|o| Preset::from_json(&o)) {
                    Ok(preset) => self.presets.push(PresetEntry {
                        preset,
                        folder: folder.clone(),
                        file_path: path,
                        built_in: false,
                    }),
                    Err(e) => self.warnings.push(format!("{}: {e}", path.display())),
                }
            }
        }

        self.presets.sort_by(|a, b| {
            b.built_in
                .cmp(&a.built_in)
                .then_with(|| a.folder.to_lowercase().cmp(&b.folder.to_lowercase()))
                .then_with(|| a.preset.order.cmp(&b.preset.order))
                .then_with(|| a.preset.name.to_lowercase().cmp(&b.preset.name.to_lowercase()))
        });
    }

    /// The user folders on disk, always including the default one.
    pub fn user_folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = sorted_dir_entries(&self.user_directory, true)
            .iter()
            .map(|d| d.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .collect();
        if !folders.iter().any(|f| f == DEFAULT_USER_FOLDER) {
            folders.insert(0, DEFAULT_USER_FOLDER.into());
        }
        folders
    }

    fn write_preset(&mut self, preset: &Preset, folder: &str, replacing: Option<&Path>) -> Result<(), Error> {
        let dir = self.user_directory.join(clean_folder(folder));
        fs::create_dir_all(&dir)?;

        let base = slug(&preset.name);
        let mut path = dir.join(format!("{base}.json"));
        let mut i = 2;
        while path.exists() && replacing != Some(path.as_path()) {
            path = dir.join(format!("{base}-{i}.json"));
            i += 1;
        }

        write_atomically(&path, &preset.to_json())?;
        if let Some(old) = replacing
            && old != path
        {
            let _ = fs::remove_file(old);
        }
        self.reload();
        Ok(())
    }

    /// Saves a user preset. Saving under an existing name in the same folder replaces it.
    pub fn save(&mut self, preset: &Preset, folder: &str) -> Result<(), Error> {
        let target = clean_folder(folder);
        let existing = self
            .presets
            .iter()
            .find(|e| !e.built_in && e.folder == target && e.preset.name == preset.name)
            .map(|e| e.file_path.clone());
        self.write_preset(preset, &target, existing.as_deref())
    }

    pub fn rename(&mut self, entry: &PresetEntry, new_name: &str) -> Result<(), Error> {
        if entry.built_in {
            return Err(Error::Invalid("Built-in presets cannot be renamed".into()));
        }
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(Error::Invalid("The name cannot be empty".into()));
        }
        let preset = Preset { name: new_name.to_owned(), ..entry.preset.clone() };
        let (folder, path) = (entry.folder.clone(), entry.file_path.clone());
        self.write_preset(&preset, &folder, Some(&path))
    }

    pub fn move_to(&mut self, entry: &PresetEntry, folder: &str) -> Result<(), Error> {
        if entry.built_in {
            return Err(Error::Invalid("Built-in presets cannot be moved".into()));
        }
        let target = clean_folder(folder);
        if target == entry.folder {
            return Ok(());
        }
        let entry = entry.clone();
        self.write_preset(&entry.preset, &target, None)?;
        fs::remove_file(&entry.file_path)?;
        if let Some(dir) = entry.file_path.parent() {
            let _ = fs::remove_dir(dir); // only succeeds if now empty
        }
        self.reload();
        Ok(())
    }

    pub fn remove(&mut self, entry: &PresetEntry) -> Result<(), Error> {
        if entry.built_in {
            return Err(Error::Invalid("Built-in presets cannot be deleted".into()));
        }
        fs::remove_file(&entry.file_path)?;
        if let Some(dir) = entry.file_path.parent() {
            let _ = fs::remove_dir(dir);
        }
        self.reload();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_presets_load() {
        let user = tempfile::tempdir().unwrap();
        let library = PresetLibrary::new(user.path());
        assert!(library.warnings().is_empty(), "{:?}", library.warnings());
        let names: Vec<&str> = library.presets().iter().map(|e| e.preset.name.as_str()).collect();
        assert!(library.presets().iter().all(|e| e.built_in));
        assert_eq!(
            names,
            [
                "Neutral",
                "Natural",
                "Soft Contrast",
                "High Contrast",
                "Warm Film",
                "Cool Film",
                "Golden Hour",
                "Muted",
                "Monochrome"
            ]
        );
    }

    #[test]
    fn user_preset_lifecycle() {
        let user = tempfile::tempdir().unwrap();
        let mut library = PresetLibrary::new(user.path());
        let built_ins = library.presets().len();

        let mut p =
            Preset { name: "Warm Film".into(), values: [("contrast".to_owned(), 8.0)].into(), ..Default::default() };
        library.save(&p, "My Presets").unwrap();
        assert!(user.path().join("My Presets/warm-film.json").exists());
        assert_eq!(library.presets().len(), built_ins + 1);

        // Saving the same name again replaces it.
        p.values = [("contrast".to_owned(), 9.0)].into();
        library.save(&p, "My Presets").unwrap();
        assert_eq!(library.presets().len(), built_ins + 1);

        let find_user = |l: &PresetLibrary| l.presets().iter().find(|e| !e.built_in).cloned().unwrap();
        assert_eq!(find_user(&library).preset.values["contrast"], 9.0);

        library.rename(&find_user(&library), "Golden").unwrap();
        assert_eq!(find_user(&library).preset.name, "Golden");
        assert!(user.path().join("My Presets/golden.json").exists());
        assert!(!user.path().join("My Presets/warm-film.json").exists());

        library.move_to(&find_user(&library), "Landscapes").unwrap();
        assert_eq!(find_user(&library).folder, "Landscapes");
        assert!(library.user_folders().contains(&"Landscapes".to_owned()));

        // Folder names cannot escape the presets directory.
        library.move_to(&find_user(&library), "../evil").unwrap();
        assert!(find_user(&library).file_path.starts_with(user.path()));
        assert_eq!(find_user(&library).folder, "-evil");

        library.remove(&find_user(&library)).unwrap();
        assert_eq!(library.presets().len(), built_ins);
        let first = library.presets()[0].clone();
        assert!(library.remove(&first).is_err()); // built-ins are read-only
    }

    #[test]
    fn slugs_and_folders() {
        assert_eq!(slug("Warm Film"), "warm-film");
        assert_eq!(slug("  --Moody/Landscape!  "), "moody-landscape");
        assert_eq!(slug("ÅÄÖ"), "preset");
        assert_eq!(clean_folder(".."), DEFAULT_USER_FOLDER);
        assert_eq!(clean_folder("Built-in"), DEFAULT_USER_FOLDER);
        assert_eq!(clean_folder("a/b"), "a-b");
    }
}
