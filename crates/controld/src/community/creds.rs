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
    pub fn bearer(&self) -> String {
        format!("{}.{}", self.device_id, self.key)
    }
}

/// The saved key; an unreadable or invalid file counts as none.
pub fn load(dir: &Path) -> Option<Credentials> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(FILE)).ok()?).ok()
}

/// Writes a temporary owner-only file and renames it over `community.json`.
pub fn save(dir: &Path, c: &Credentials) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = dir.join(format!("{FILE}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut f = options.open(&tmp)?;
    // `mode` applies only on create; a leftover file is narrowed too.
    #[cfg(unix)]
    f.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    f.write_all(serde_json::to_string(c).expect("credentials serialize").as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, dir.join(FILE))
}

/// Forgets the key; a missing file is fine.
pub fn remove(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(dir.join(FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
