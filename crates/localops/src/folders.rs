//! Folder picker backend. Rooted at the home folder, with symlinks resolved before every
//! check. It lists directories, and file names only when asked (to attach files to a team's
//! Input); file contents are never returned.
//!
//! Paths are resolved one part at a time (`resolve_inside`), never looking up anything
//! outside home or its ancestors. The checks are path-based, so they are not race-free: a process running as the same user that swaps a folder for a symlink between
//! the check and the `read_dir`/`create_dir` can race them. Sculptured assumes a
//! single-user machine, where such a process could already do anything the user can. A
//! descriptor-based walk (`openat` with `O_NOFOLLOW` on every component) is deferred.
use crate::LocalError;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::collections::VecDeque;
#[cfg(unix)]
use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderEntry {
    pub name: String,
    /// Real (symlink-resolved) absolute path.
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FolderListing {
    pub path: String,
    /// None at the home folder: the picker cannot go higher.
    pub parent: Option<String>,
    pub home: String,
    /// Sub-folders only. Files and dot-folders are never listed.
    pub entries: Vec<FolderEntry>,
    /// More than MAX_ENTRIES sub-folders: the rest are not shown.
    pub truncated: bool,
    /// The folder's files (names and paths, never contents, no dot-files), only when asked for.
    /// Absent from an older runner, which cannot list files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub files: Option<Vec<FolderEntry>>,
}

pub const MAX_ENTRIES: usize = 2000;

/// The home folder in its two spellings: `real` (canonical, what every answer uses) and
/// `given` (HOME as configured, which may pass through links, such as `/home -> /var/home`
/// or an automounted home).
#[derive(Clone, Debug)]
pub struct Home {
    pub real: PathBuf,
    pub given: PathBuf,
}

impl Home {
    /// The home folder from HOME as configured; `home_unknown` when it can't be resolved.
    pub fn of(given: &Path) -> Result<Home, LocalError> {
        let real = plain(given.canonicalize().map_err(|e| {
            LocalError::failed(
                "home_unknown",
                format!("Your home folder can't be read: {e}"),
            )
        })?);
        Ok(Home {
            real,
            given: given.to_path_buf(),
        })
    }
}

fn outside_home() -> LocalError {
    LocalError::forbidden(
        "outside_home",
        "Only folders inside your home folder can be chosen here",
    )
}

fn bad_path(message: impl Into<String>) -> LocalError {
    LocalError::bad("bad_path", message)
}

fn unreadable() -> LocalError {
    LocalError::forbidden("folder_unreadable", "Sculptured can't read that folder")
}

#[cfg(unix)]
/// Splits on `/` without the normalisation `Path::components` applies (which drops `.`).
fn segments(p: &Path) -> impl Iterator<Item = &[u8]> {
    p.as_os_str().as_bytes().split(|b| *b == b'/')
}

#[cfg(unix)]
/// Refuses any path with a `.` or `..` component, before the filesystem is asked anything.
/// `realpath` must find each folder before it can apply a following `..`, so resolving such
/// a path would reveal whether places outside home exist. The picker only sends real paths.
/// The answer depends on the text alone: `outside_home` when the path, read literally,
/// leaves home, otherwise `bad_path`.
fn refuse_dot_segments(home: &Home, requested: &Path) -> Result<(), LocalError> {
    if !segments(requested).any(|s| s == b"." || s == b"..") {
        return Ok(());
    }
    let mut literal = PathBuf::from("/");
    for s in segments(requested) {
        match s {
            b"" | b"." => {}
            b".." => {
                literal.pop();
            }
            s => literal.push(std::ffi::OsStr::from_bytes(s)),
        }
    }
    if !literal.starts_with(&home.real) && !literal.starts_with(&home.given) {
        return Err(outside_home());
    }
    Err(bad_path("Use a folder path without . or .."))
}

#[cfg(unix)]
/// Most symlinks followed while resolving one path, like the kernel's own limit.
const MAX_LINKS: usize = 40;

#[cfg(unix)]
/// True for places the walk may look up: inside home, or on the way to it (an ancestor
/// of home, or of HOME as configured, or HOME itself). Decided from the text alone,
/// before anything is looked up. HOME's own spelling is the user's configured path, so
/// following its links reveals nothing.
fn may_look_up(home: &Home, p: &Path) -> bool {
    p.starts_with(&home.real) || home.real.starts_with(p) || home.given.starts_with(p)
}

fn lookup_error(e: &std::io::Error) -> LocalError {
    match e.kind() {
        ErrorKind::NotFound => {
            LocalError::not_found("folder_missing", "That folder does not exist")
        }
        ErrorKind::PermissionDenied => unreadable(),
        ErrorKind::NotADirectory => LocalError::bad("not_a_folder", "That is a file, not a folder"),
        // NUL bytes, names that are too long, …
        _ => bad_path(format!("That folder path can't be used: {e}")),
    }
}

/// Resolves `requested` (following every symlink) and refuses anything outside `home`.
///
/// This is `realpath` done one part at a time, so the filesystem is never asked about a
/// place outside home: before each part is looked up, the place it names must be inside
/// home or one of home's own ancestors (see `may_look_up`), or the answer is
/// `outside_home` straight away. A link's target, `..` included, is walked by the same
/// rule. So a missing, existing or unreadable place outside home, a dangling link to
/// one, and a link that passes through one and comes back with `..`, all get the same
/// answer, and only places inside home can be reported as missing, unreadable or files.
#[cfg(unix)]
pub fn resolve_inside(home: &Home, requested: &Path) -> Result<PathBuf, LocalError> {
    if !requested.is_absolute() {
        return Err(LocalError::bad(
            "path_not_absolute",
            "Folder paths must be full paths",
        ));
    }
    refuse_dot_segments(home, requested)?;
    let cur = walk(home, requested)?;
    if !cur.starts_with(&home.real) {
        return Err(outside_home());
    }
    let meta = std::fs::metadata(&cur).map_err(|e| lookup_error(&e))?;
    if !meta.is_dir() {
        return Err(LocalError::bad(
            "not_a_folder",
            "That is a file, not a folder",
        ));
    }
    Ok(cur)
}

/// Windows: resolves `requested` with the system's canonical path (links followed) and refuses
/// anything outside `home`. Unlike the Unix walk this asks Windows about the requested place before
/// checking it, so a place outside home may be told apart as missing or not; nothing about it is shown.
#[cfg(windows)]
pub fn resolve_inside(home: &Home, requested: &Path) -> Result<PathBuf, LocalError> {
    use std::path::Component;
    if !requested.is_absolute() {
        return Err(LocalError::bad(
            "path_not_absolute",
            "Folder paths must be full paths",
        ));
    }
    if requested
        .components()
        .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
    {
        return Err(bad_path("Use a folder path without . or .."));
    }
    let inside = requested.starts_with(&home.real) || requested.starts_with(&home.given);
    let cur = plain(requested.canonicalize().map_err(|e| {
        if inside {
            lookup_error(&e)
        } else {
            outside_home()
        }
    })?);
    if !cur.starts_with(&home.real) {
        return Err(outside_home());
    }
    let meta = std::fs::metadata(&cur).map_err(|e| lookup_error(&e))?;
    if !meta.is_dir() {
        return Err(LocalError::bad(
            "not_a_folder",
            "That is a file, not a folder",
        ));
    }
    Ok(cur)
}

/// `\\?\C:\Users\a` → `C:\Users\a`: Windows' canonical form, as people and tools write it. Other
/// verbatim paths (network shares) and every Unix path are kept as they are.
#[must_use]
pub fn plain(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => p,
    }
}

#[cfg(unix)]
/// The path's non-empty parts.
fn parts(p: &Path) -> impl Iterator<Item = OsString> + '_ {
    segments(p)
        .filter(|s| !s.is_empty())
        .map(|s| OsStr::from_bytes(s).to_owned())
}

#[cfg(unix)]
/// `realpath`, one part at a time, never asking about a place outside home (see `resolve_inside`).
fn walk(home: &Home, requested: &Path) -> Result<PathBuf, LocalError> {
    let mut cur = PathBuf::from("/");
    let mut rest: VecDeque<OsString> = parts(requested).collect();
    let mut links = 0;
    while let Some(name) = rest.pop_front() {
        match name.as_bytes() {
            b"." => continue,
            // `cur` is real and inside home or an ancestor of it, so its parent is too.
            b".." => {
                cur.pop();
                continue;
            }
            _ => {}
        }
        let next = cur.join(&name);
        if !is_link(home, &next)? {
            cur = next;
            continue;
        }
        links += 1;
        if links > MAX_LINKS {
            return Err(bad_path("That folder path has too many links in it"));
        }
        let target = std::fs::read_link(&next).map_err(|e| lookup_error(&e))?;
        if target.is_absolute() {
            cur = PathBuf::from("/");
        }
        let reversed_items = parts(&target).collect::<Vec<_>>().into_iter().rev();
        for s in reversed_items {
            rest.push_front(s);
        }
    }
    Ok(cur)
}

#[cfg(unix)]
/// Whether `next` is a link. A place outside home (and its ancestors) is refused before it is looked up.
fn is_link(home: &Home, next: &Path) -> Result<bool, LocalError> {
    if !may_look_up(home, next) {
        return Err(outside_home());
    }
    let meta = std::fs::symlink_metadata(next).map_err(|e| lookup_error(&e))?;
    Ok(meta.file_type().is_symlink())
}

fn text(p: &Path) -> Result<String, LocalError> {
    p.to_str()
        .map(str::to_string)
        .ok_or_else(|| bad_path("That folder's name can't be shown"))
}

/// Case-insensitive, then byte order so the result is total and stable.
fn by_name(a: &FolderEntry, b: &FolderEntry) -> std::cmp::Ordering {
    a.name
        .to_lowercase()
        .cmp(&b.name.to_lowercase())
        .then_with(|| a.name.cmp(&b.name))
}

/// A folder the picker can show: not hidden, a directory or a link to one inside home. None otherwise.
fn folder_entry(home: &Home, entry: &std::fs::DirEntry) -> Option<FolderEntry> {
    let name = entry.file_name().into_string().ok()?;
    if name.starts_with('.') {
        return None;
    }
    let kind = entry.file_type().ok()?;
    let real = if kind.is_dir() {
        // A real directory in a canonical folder: its path is already canonical.
        entry.path()
    } else if kind.is_symlink() {
        // Follow the link by the same rule as a request: only directories inside home.
        resolve_inside(home, &entry.path()).ok()?
    } else {
        return None; // plain files, sockets, devices, fifos
    };
    let path = real.to_str().map(str::to_string)?;
    Some(FolderEntry { name, path })
}

/// The plain files in `dir` (a canonical folder inside home), not dot-files, sorted.
fn file_entries(dir: &Path) -> Result<Vec<FolderEntry>, LocalError> {
    let read = std::fs::read_dir(dir).map_err(|_| unreadable())?;
    let mut files: Vec<FolderEntry> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|k| k.is_file()))
        .filter_map(|e| {
            let name = e
                .file_name()
                .into_string()
                .ok()
                .filter(|n| !n.starts_with('.'))?;
            Some(FolderEntry {
                path: e.path().to_str()?.to_string(),
                name,
            })
        })
        .collect();
    files.sort_by(by_name);
    files.truncate(MAX_ENTRIES);
    Ok(files)
}

pub fn listing(
    home: &Home,
    requested: Option<&str>,
    with_files: bool,
) -> Result<FolderListing, LocalError> {
    // Only an empty or all-blank `path` means home; a real path is used exactly as sent.
    let dir = match requested.filter(|p| !p.trim().is_empty()) {
        Some(p) => resolve_inside(home, Path::new(p))?,
        None => home.real.clone(),
    };
    let read = std::fs::read_dir(&dir).map_err(|_| unreadable())?;
    let mut entries: Vec<FolderEntry> = read
        .flatten()
        .filter_map(|entry| folder_entry(home, &entry))
        .collect();
    entries.sort_by(by_name);
    let truncated = entries.len() > MAX_ENTRIES;
    entries.truncate(MAX_ENTRIES);
    let parent = if dir == home.real {
        None
    } else {
        dir.parent().map(text).transpose()?
    };
    let files = if with_files {
        Some(file_entries(&dir)?)
    } else {
        None
    };
    Ok(FolderListing {
        path: text(&dir)?,
        parent,
        home: text(&home.real)?,
        entries,
        truncated,
        files,
    })
}

/// Unicode general category Cf (format characters, Unicode 16): invisible characters
/// such as bidi overrides, zero-width joiners, the BOM and tag characters, which can make
/// a folder name read as something it is not.
const FORMAT_CHARS: &[(char, char)] = &[
    ('\u{ad}', '\u{ad}'),
    ('\u{600}', '\u{605}'),
    ('\u{61c}', '\u{61c}'),
    ('\u{6dd}', '\u{6dd}'),
    ('\u{70f}', '\u{70f}'),
    ('\u{890}', '\u{891}'),
    ('\u{8e2}', '\u{8e2}'),
    ('\u{180e}', '\u{180e}'),
    ('\u{200b}', '\u{200f}'),
    ('\u{202a}', '\u{202e}'),
    ('\u{2060}', '\u{2064}'),
    ('\u{2066}', '\u{206f}'),
    ('\u{feff}', '\u{feff}'),
    ('\u{fff9}', '\u{fffb}'),
    ('\u{110bd}', '\u{110bd}'),
    ('\u{110cd}', '\u{110cd}'),
    ('\u{13430}', '\u{1343f}'),
    ('\u{1bca0}', '\u{1bca3}'),
    ('\u{1d173}', '\u{1d17a}'),
    ('\u{e0001}', '\u{e0001}'),
    ('\u{e0020}', '\u{e007f}'),
];

fn is_format_char(c: char) -> bool {
    FORMAT_CHARS.iter().any(|&(lo, hi)| (lo..=hi).contains(&c))
}

fn valid_name(name: &str) -> Result<&str, LocalError> {
    let n = name.trim();
    let unsafe_char = |c: char| {
        c == '/'
            || c.is_control()
            || is_format_char(c)
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{85}')
    };
    if n.is_empty() || n.starts_with('.') || n.chars().any(unsafe_char) || n.len() > 255 {
        return Err(LocalError::bad(
            "bad_folder_name",
            "Folder names can't be empty, start with a dot, or contain '/', control characters or invisible formatting characters",
        ));
    }
    Ok(n)
}

/// Makes folder `name` inside `parent` (which must be inside home).
pub fn create(home: &Home, parent: &str, name: &str) -> Result<FolderEntry, LocalError> {
    let parent = resolve_inside(home, Path::new(parent))?;
    let name = valid_name(name)?;
    let target = parent.join(name);
    std::fs::create_dir(&target).map_err(|e| match e.kind() {
        ErrorKind::AlreadyExists => LocalError::conflict(
            "folder_exists",
            format!("A folder called {name} already exists here"),
        ),
        ErrorKind::PermissionDenied => {
            LocalError::forbidden("folder_unwritable", "Sculptured can't create a folder here")
        }
        _ => LocalError::bad(
            "folder_not_created",
            format!("The folder could not be created: {e}"),
        ),
    })?;
    let real = resolve_inside(home, &target)?;
    Ok(FolderEntry {
        name: name.to_string(),
        path: text(&real)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn files_are_listed_only_when_asked_and_never_hidden_ones() {
        let root = tempfile::tempdir().unwrap();
        let home = Home::of(root.path()).unwrap();
        std::fs::create_dir_all(root.path().join("docs")).unwrap();
        std::fs::write(root.path().join("docs/spec.pdf"), "x").unwrap();
        std::fs::write(root.path().join("docs/.env"), "secret").unwrap();
        let docs = root.path().join("docs").display().to_string();
        assert_eq!(
            listing(&home, Some(&docs), false).unwrap().files,
            None,
            "folders only unless files are asked for"
        );
        let files = listing(&home, Some(&docs), true).unwrap().files.unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["spec.pdf"]);
        assert!(files[0].path.ends_with("/docs/spec.pdf"));
    }

    #[test]
    fn listing_shows_subfolders_inside_home_and_refuses_escapes() {
        let root = tempfile::tempdir().unwrap();
        let home = Home::of(root.path()).unwrap();
        std::fs::create_dir_all(root.path().join("proj/src")).unwrap();
        std::fs::create_dir_all(root.path().join(".hidden")).unwrap();
        std::fs::write(root.path().join("file.txt"), "").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        let l = listing(&home, None, false).unwrap();
        let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["proj"], "no files, no dot folders, no links out");
        assert!(l.parent.is_none());
        assert!(listing(&home, Some(outside.path().to_str().unwrap()), false).is_err());
        assert!(
            listing(
                &home,
                Some(&format!("{}/proj/../..", root.path().display())),
                false
            )
            .is_err()
        );
        let made = create(&home, root.path().join("proj").to_str().unwrap(), "new").unwrap();
        assert!(Path::new(&made.path).is_dir());
        assert_eq!(
            create(&home, root.path().join("proj").to_str().unwrap(), "../x")
                .unwrap_err()
                .code,
            "bad_folder_name"
        );
    }
}
