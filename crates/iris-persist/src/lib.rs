//! `.iris.json` sidecars and preset files. Both are JSON documents in the format the
//! C++ version wrote, so existing files keep working.

pub mod json;
pub mod library;
pub mod preset;
pub mod sidecar;

use std::fs;
use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value};

pub use library::{PresetEntry, PresetLibrary};
pub use preset::Preset;
pub use sidecar::{read_sidecar, read_sidecar_file, sidecar_path_for, write_sidecar};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("it was written by a newer version of Project Iris")]
    NewerVersion,
    #[error("{0}")]
    Invalid(String),
}

const MAX_JSON_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) fn read_json_object(path: &Path) -> Result<Map<String, Value>, Error> {
    if fs::metadata(path)?.len() > MAX_JSON_BYTES {
        return Err(Error::Invalid("file is too large".into()));
    }
    match serde_json::from_slice(&fs::read(path)?)? {
        Value::Object(object) => Ok(object),
        _ => Err(Error::Invalid("not a JSON object".into())),
    }
}

/// Writes pretty-printed JSON so that an existing file is only replaced once the new one
/// has been written completely.
pub(crate) fn write_atomically(path: &Path, json: &Value) -> Result<(), Error> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    // Four-space indentation, like the files the C++ version wrote.
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut file, serde_json::ser::PrettyFormatter::with_indent(b"    "));
    serde::Serialize::serialize(json, &mut serializer)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
