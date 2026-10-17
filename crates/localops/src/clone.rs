//! Copy-on-write copies: on a filesystem that clones files (APFS, btrfs, XFS, ReFS), an agent's copy
//! of the project shares disk with it until either side changes a file, build output included. Where
//! cloning is impossible nothing is written, and the caller checks the copy out the usual way. A file
//! that cannot be cloned is skipped, never copied in full, so a copy never costs more than a checkout.
use std::io;
use std::path::Path;

/// Clones one file; `reflink_copy::reflink` in use, a plain copy in tests.
pub(crate) type Cloner = fn(&Path, &Path) -> io::Result<()>;

pub(crate) fn reflink(from: &Path, to: &Path) -> io::Result<()> {
    reflink_copy::reflink(from, to)
}

/// True when files under `src` can be cloned into `dst_dir` (one real file tried, then removed).
pub(crate) fn can_clone(src: &Path, dst_dir: &Path, clone: Cloner) -> bool {
    let Some(sample) = first_file(src, 0) else { return false };
    let probe = dst_dir.join(".sculptured-clone-probe");
    let ok = clone(&sample, &probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

fn first_file(dir: &Path, depth: u32) -> Option<std::path::PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in &entries {
        if e.file_name() != ".git" && e.file_type().is_ok_and(|t| t.is_file()) {
            return Some(e.path());
        }
    }
    entries.iter().filter(|e| depth < 3 && e.file_name() != ".git" && e.file_type().is_ok_and(|t| t.is_dir())).find_map(|e| first_file(&e.path(), depth + 1))
}

/// Clones the tree at `src` into `dst`, leaving out every `.git` and `skip` (the copies folder, when it
/// is inside the project). Returns how many files were cloned.
pub(crate) fn clone_tree(src: &Path, dst: &Path, skip: &Path, clone: Cloner) -> io::Result<u64> {
    let mut cloned = 0;
    let mut stack = vec![std::path::PathBuf::new()];
    while let Some(rel) = stack.pop() {
        std::fs::create_dir_all(dst.join(&rel))?;
        for e in std::fs::read_dir(src.join(&rel))?.flatten() {
            let (name, from) = (e.file_name(), e.path());
            if name == ".git" || from == skip {
                continue;
            }
            let (to, rel) = (dst.join(&rel).join(&name), rel.join(&name));
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                stack.push(rel);
            } else if t.is_symlink() {
                link(&from, &to);
            } else if t.is_file() && !to.exists() && clone(&from, &to).is_ok() {
                if let Ok(m) = e.metadata() {
                    let _ = std::fs::set_permissions(&to, m.permissions());
                }
                cloned += 1;
            }
        }
    }
    Ok(cloned)
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) {
    if let Ok(target) = std::fs::read_link(from) {
        let _ = std::os::unix::fs::symlink(target, to);
    }
}

#[cfg(not(unix))]
fn link(_from: &Path, _to: &Path) {}
