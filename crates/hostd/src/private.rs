//! Files and folders only this user may read. Unix: modes 0600 and 0700. Windows: files under the
//! user's profile already inherit access for that user only, so they are written plainly there.
use std::path::Path;

/// Writes `bytes` to `path` (replacing it, flushed to disk), readable by this user only.
pub use localops::private::write_file;

/// Creates `dir` (and its parents) for this user only when it is missing; never loosens an existing one.
pub fn create_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

/// `dir`'s permission bits that let other users in (`mode & 0o777`), when any do.
#[cfg(unix)]
pub fn open_to_others(dir: &Path) -> std::io::Result<Option<u32>> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(dir)?.permissions().mode();
    Ok((mode & 0o077 != 0).then_some(mode & 0o777))
}

/// Windows: the profile's own access rules apply; nothing to check beyond the folder being there.
#[cfg(not(unix))]
pub fn open_to_others(dir: &Path) -> std::io::Result<Option<u32>> {
    std::fs::metadata(dir).map(|_| None)
}

/// Narrows an existing folder to this user only (0700).
#[cfg(unix)]
pub fn narrow_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
}

/// Windows: the profile's own access rules apply.
#[cfg(not(unix))]
pub fn narrow_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}
