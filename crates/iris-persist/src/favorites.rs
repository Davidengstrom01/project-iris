//! Favorites, and moving photos together with their sidecars.
//!
//! The favorite flag lives in the photo's sidecar (`"favorite": true`), so it travels with
//! the photo. Favoriting a photo without edits writes a minimal sidecar; un-favoriting it
//! removes that sidecar again, so "has a sidecar" is not the same as "has edits": use
//! [`has_edits`] for that.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::sidecar::{SIDECAR_VERSION, sidecar_path_for};
use crate::{Error, read_json_object, write_atomically};

const FAVORITE_KEY: &str = "favorite";

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn read(raw_path: &Path) -> Option<Map<String, Value>> {
    let path = sidecar_path_for(raw_path);
    path.exists().then(|| read_json_object(&path).ok()).flatten()
}

/// Whether the photo is marked as a favorite.
pub fn is_favorite(raw_path: &Path) -> bool {
    read(raw_path).and_then(|j| j.get(FAVORITE_KEY).and_then(Value::as_bool)).unwrap_or(false)
}

/// Whether the photo has saved edits (not just a favorite flag).
pub fn has_edits(raw_path: &Path) -> bool {
    read(raw_path).is_some_and(|j| j.contains_key("adjustments"))
}

/// Marks or unmarks a photo as a favorite, keeping any saved edits as they are.
pub fn set_favorite(raw_path: &Path, favorite: bool) -> Result<(), Error> {
    let path = sidecar_path_for(raw_path);
    let mut json = if path.exists() { read_json_object(&path)? } else { Map::new() };
    if favorite {
        json.entry("version").or_insert(json!(SIDECAR_VERSION));
        json.entry("software").or_insert(json!("Project Iris"));
        json.entry("originalFilename").or_insert(json!(file_name(raw_path)));
        json.insert(FAVORITE_KEY.into(), json!(true));
    } else {
        json.remove(FAVORITE_KEY);
        // A sidecar that only held the flag is not needed any more.
        let bookkeeping = ["version", "software", "originalFilename"];
        if json.keys().all(|k| bookkeeping.contains(&k.as_str())) {
            if path.exists() {
                fs::remove_file(&path)?;
            }
            return Ok(());
        }
    }
    write_atomically(&path, &Value::Object(json))
}

/// Moves a file, also across file systems (copy, then delete).
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(from, to)?;
            fs::remove_file(from)
        }
    }
}

/// Moves a photo and its sidecar into `folder`; returns the photo's new path. Nothing is
/// overwritten: if a file of that name is already there, the photo stays where it is.
pub fn move_photo(raw_path: &Path, folder: &Path) -> Result<PathBuf, Error> {
    let target = folder.join(raw_path.file_name().ok_or_else(|| Error::Invalid("not a file".into()))?);
    if target.exists() {
        return Err(Error::Invalid(format!("{} already exists in {}", file_name(&target), folder.display())));
    }
    let sidecar = sidecar_path_for(raw_path);
    let target_sidecar = sidecar.exists().then(|| sidecar_path_for(&target));
    if let Some(t) = &target_sidecar
        && t.exists()
    {
        return Err(Error::Invalid(format!("{} already exists in {}", file_name(t), folder.display())));
    }
    move_file(raw_path, &target)?;
    if let Some(t) = target_sidecar
        && let Err(e) = move_file(&sidecar, &t)
    {
        // Keep the photo and its edits together.
        let _ = move_file(&target, raw_path);
        return Err(e.into());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_sidecar, write_sidecar};
    use iris_core::{EditState, WhiteBalance};

    const AS_SHOT: WhiteBalance = WhiteBalance { temperature: 5500.0, tint: 10.0 };

    fn photo(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, "raw").unwrap();
        path
    }

    #[test]
    fn favorites_without_edits_use_a_minimal_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let raw = photo(dir.path(), "a.ARW");
        assert!(!is_favorite(&raw));
        set_favorite(&raw, true).unwrap();
        assert!(is_favorite(&raw));
        assert!(!has_edits(&raw)); // only the flag
        // Reading edits from it gives the defaults.
        assert_eq!(read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap(), EditState::new(AS_SHOT));
        set_favorite(&raw, false).unwrap();
        assert!(!sidecar_path_for(&raw).exists()); // removed again
    }

    #[test]
    fn favorites_and_edits_keep_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let raw = photo(dir.path(), "b.ARW");
        let mut edits = EditState::new(AS_SHOT);
        edits.basic.exposure = 0.5;
        write_sidecar(&sidecar_path_for(&raw), &raw, &edits).unwrap();
        set_favorite(&raw, true).unwrap();
        assert!(has_edits(&raw) && is_favorite(&raw));
        assert_eq!(read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap(), edits);

        // Saving edits again keeps the flag.
        edits.basic.exposure = 1.0;
        write_sidecar(&sidecar_path_for(&raw), &raw, &edits).unwrap();
        assert!(is_favorite(&raw));
        // Un-favoriting keeps the edits.
        set_favorite(&raw, false).unwrap();
        assert!(!is_favorite(&raw));
        assert_eq!(read_sidecar(&raw, &EditState::new(AS_SHOT)).unwrap().unwrap().basic.exposure, 1.0);
    }

    #[test]
    fn photos_move_with_their_sidecars_and_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let picks = dir.path().join("picks");
        fs::create_dir(&picks).unwrap();
        let raw = photo(dir.path(), "c.ARW");
        set_favorite(&raw, true).unwrap();
        let moved = move_photo(&raw, &picks).unwrap();
        assert_eq!(moved, picks.join("c.ARW"));
        assert!(!raw.exists() && moved.exists());
        assert!(is_favorite(&moved));
        assert!(!dir.path().join("c.iris.json").exists());

        // A name that is taken: nothing moves.
        let again = photo(dir.path(), "c.ARW");
        assert!(move_photo(&again, &picks).is_err());
        assert!(again.exists());

        // A photo whose base name is taken by another photo's sidecar there.
        let other = photo(&picks, "d.CR2");
        set_favorite(&other, true).unwrap();
        let d = photo(dir.path(), "d.ARW");
        set_favorite(&d, true).unwrap();
        let moved = move_photo(&d, &picks).unwrap();
        assert!(is_favorite(&moved) && is_favorite(&other));
        assert!(picks.join("d.ARW.iris.json").exists());
    }
}
