//! `<data_dir>/community.json` (C5): the community-scope device key, readable by this user only and
//! never sent to the browser.
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FILE: &str = "community.json";

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub origin: String,
    pub device_id: String,
    pub key: String,
}

impl Credentials {
    /// What goes after `Bearer `: `<device_id>.<key>`.
    #[must_use]
    pub fn bearer(&self) -> String {
        format!("{}.{}", self.device_id, self.key)
    }
}

/// The saved key; an unreadable or invalid file counts as none.
#[must_use]
pub fn load(dir: &Path) -> Option<Credentials> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(FILE)).ok()?).ok()
}

/// Writes a temporary owner-only file and renames it over `community.json`.
pub fn save(dir: &Path, c: &Credentials) -> std::io::Result<()> {
    let tmp = dir.join(format!("{FILE}.tmp"));
    localops::private::write_file(
        &tmp,
        serde_json::to_string(c)
            .expect("credentials serialize")
            .as_bytes(),
    )?;
    std::fs::rename(&tmp, dir.join(FILE))
}

/// Forgets the key; a missing file is fine.
pub fn remove(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(dir.join(FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
