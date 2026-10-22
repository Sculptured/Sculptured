//! The project map: a wide, shallow picture of a folder that agents get at their start, generated
//! with no one maintaining it. Its areas (folders up to two levels deep), what kind of files they
//! hold, what their notes say they are, and each Markdown file's sections with line ranges, so an
//! agent learns what already exists and can open exactly the part it needs. Written to
//! `.sculptured/map.md` in the folder; nothing else in the folder is changed.
use crate::LocalError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Files looked at, at most; a larger folder is mapped from its first files (`truncated`).
const MAX_FILES: usize = 20_000;
/// Areas listed, at most.
const MAX_AREAS: usize = 80;
/// Markdown files whose sections are listed, and sections per file.
const MAX_DOCS: usize = 150;
const MAX_SECTIONS: usize = 12;
/// A Markdown file larger than this is listed without its sections.
const MAX_DOC_BYTES: u64 = 512 * 1024;
const SKIP: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".sculptured",
    ".next",
    ".cache",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectMap {
    pub root: String,
    pub files: u32,
    pub areas: Vec<Area>,
    pub truncated: bool,
    /// Where the full map was written (relative to `root`).
    pub file: String,
    /// Lessons people confirmed in earlier teams, kept in `.sculptured/lessons.md` in this folder.
    #[serde(default)]
    pub kept: Vec<String>,
    /// The contracts' shared values, as found in their files at this start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<SharedCheck>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharedCheck {
    pub name: String,
    pub value: String,
    pub file: String,
    /// `found` (with its first line), `missing` (the file does not contain the value) or `no-file`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Each shared value looked up in its file: observed, not reported. Paths that leave the folder are
/// not read.
fn check_shared(root: &Path, shared: &[model::SharedValue]) -> Vec<SharedCheck> {
    shared
        .iter()
        .map(|v| {
            let rel = Path::new(&v.file);
            let inside = !rel.is_absolute()
                && !rel
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir));
            let text = inside
                .then(|| std::fs::read_to_string(root.join(rel)).ok())
                .flatten();
            let line = text.as_ref().and_then(|t| {
                t.lines()
                    .position(|l| l.contains(&v.value))
                    .map(|i| i as u32 + 1)
            });
            let status = match (&text, line) {
                (None, _) => "no-file",
                (Some(_), None) => "missing",
                (Some(_), Some(_)) => "found",
            };
            SharedCheck {
                name: v.name.clone(),
                value: v.value.clone(),
                file: v.file.clone(),
                status: status.into(),
                line,
            }
        })
        .collect()
}

/// The map of `root` with the team's shared values checked in it.
pub fn build_checked(root: &Path, shared: &[model::SharedValue]) -> Result<ProjectMap, LocalError> {
    let mut map = build(root)?;
    map.shared = check_shared(root, shared);
    Ok(map)
}

/// Confirmed lessons kept with the project, one `- ` line each, so later teams start knowing them.
pub const LESSONS_FILE: &str = ".sculptured/lessons.md";
const LESSONS_HEADER: &str = "# Lessons kept with this project\n\nConfirmed by a person in Sculptured. Every agent that works in this folder is told them. Delete a line to drop it.\n\n";
/// Kept lessons read into a briefing, at most.
const MAX_KEPT: usize = 30;

fn kept(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(LESSONS_FILE)).unwrap_or_default();
    text.lines()
        .filter_map(|l| l.strip_prefix("- "))
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .take(MAX_KEPT)
        .collect()
}

/// Adds `text` to the folder's kept lessons unless it is there already.
pub fn keep_lesson(root: &Path, text: &str) -> Result<bool, LocalError> {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !root.is_dir() || line.is_empty() {
        return Err(LocalError::not_found(
            "folder_missing",
            format!("{} is not a folder", root.display()),
        ));
    }
    let file = root.join(LESSONS_FILE);
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    if old
        .lines()
        .any(|l| l.strip_prefix("- ").is_some_and(|l| l.trim() == line))
    {
        return Ok(false);
    }
    let next = format!(
        "{}- {line}\n",
        if old.is_empty() {
            LESSONS_HEADER.to_string()
        } else if old.ends_with('\n') {
            old
        } else {
            format!("{old}\n")
        }
    );
    std::fs::create_dir_all(root.join(".sculptured"))
        .and_then(|()| std::fs::write(&file, next))
        .map_err(|e| {
            LocalError::conflict("lesson_not_kept", format!("could not keep the lesson: {e}"))
        })?;
    Ok(true)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Area {
    /// Relative folder; "" for the files at the top.
    pub path: String,
    pub files: u32,
    /// The commonest file kinds, by extension, with counts.
    pub kinds: Vec<(String, u32)>,
    /// What the area's own notes say it is: its README's (or first doc's) title or first line.
    pub about: Option<String>,
    pub docs: Vec<Doc>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Doc {
    pub file: String,
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub line: u32,
    pub end: u32,
}

/// The folder's files, relative: git's view when it is a repository (ignored files left out), else a walk.
fn list_files(root: &Path) -> (Vec<PathBuf>, bool) {
    let git = std::process::Command::new("git")
        .args([
            "-C",
            &root.display().to_string(),
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .stderr(std::process::Stdio::null())
        .output();
    if let Some(out) = git.ok().filter(|o| o.status.success()) {
        let all: Vec<PathBuf> = out
            .stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| PathBuf::from(String::from_utf8_lossy(s).into_owned()))
            .collect();
        let kept: Vec<PathBuf> = all
            .into_iter()
            .filter(|p| {
                !p.components()
                    .any(|c| SKIP.contains(&c.as_os_str().to_string_lossy().as_ref()))
            })
            .collect();
        let truncated = kept.len() > MAX_FILES;
        return (kept.into_iter().take(MAX_FILES).collect(), truncated);
    }
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&dir)) else {
            continue;
        };
        let flattened_entries = entries.flatten();
        for e in flattened_entries {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            let rel = dir.join(&name);
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(rel),
                Ok(t) if t.is_file() => out.push(rel),
                _ => {}
            }
            if out.len() > MAX_FILES {
                return (out, true);
            }
        }
    }
    out.sort();
    (out, false)
}

/// A file's area: its folder, cut to two levels.
fn area_of(file: &Path) -> String {
    let parts: Vec<String> = file
        .parent()
        .into_iter()
        .flat_map(|p| p.components())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .take(2)
        .collect();
    parts.join("/")
}

/// A Markdown file's headings (levels 1–3) with the line ranges they cover.
fn sections(text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.lines().collect();
    let mut heads: Vec<(u32, String)> = Vec::new();
    let mut fenced = false;
    let indexed_lines = lines.iter().enumerate();
    for (i, l) in indexed_lines {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let hashes = l.chars().take_while(|c| *c == '#').count();
        if !fenced && (1..=3).contains(&hashes) && l[hashes..].starts_with(' ') {
            heads.push((i as u32 + 1, l[hashes..].trim().to_string()));
        }
    }
    let total = lines.len() as u32;
    heads
        .iter()
        .enumerate()
        .map(|(k, (line, title))| Section {
            title: title.clone(),
            line: *line,
            end: heads.get(k + 1).map_or(total, |n| n.0 - 1),
        })
        .collect()
}

fn about(root: &Path, docs: &[PathBuf]) -> Option<String> {
    let readme = docs
        .iter()
        .find(|d| {
            d.file_stem()
                .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("readme"))
        })
        .or(docs.first())?;
    let text = std::fs::read_to_string(root.join(readme)).ok()?;
    let line = text
        .lines()
        .map(|l| l.trim_start_matches('#').trim())
        .find(|l| !l.is_empty() && !l.starts_with('<') && !l.starts_with('!'))?;
    Some(line.chars().take(120).collect())
}

/// The map of `root`; writes it to `root/.sculptured/map.md`.
pub fn build(root: &Path) -> Result<ProjectMap, LocalError> {
    if !root.is_dir() {
        return Err(LocalError::not_found(
            "folder_missing",
            format!("{} is not a folder", root.display()),
        ));
    }
    let (files, truncated) = list_files(root);
    let mut by_area: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for f in &files {
        by_area.entry(area_of(f)).or_default().push(f.clone());
    }
    let mut docs_left = MAX_DOCS;
    let areas: Vec<Area> = by_area
        .into_iter()
        .take(MAX_AREAS)
        .map(|(path, fs)| area(root, path, &fs, &mut docs_left))
        .collect();
    let map = ProjectMap {
        root: root.display().to_string(),
        files: files.len() as u32,
        areas,
        truncated: truncated || files.len() > MAX_FILES,
        file: ".sculptured/map.md".into(),
        kept: kept(root),
        shared: vec![],
    };
    let dir = root.join(".sculptured");
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join("map.md"), markdown(&map)))
        .map_err(|e| {
            LocalError::conflict(
                "map_not_written",
                format!("could not write the project map: {e}"),
            )
        })?;
    Ok(map)
}

fn area(root: &Path, path: String, fs: &[PathBuf], docs_left: &mut usize) -> Area {
    let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
    for f in fs {
        *kinds
            .entry(
                f.extension()
                    .map_or_else(|| "other".into(), |e| e.to_string_lossy().to_lowercase()),
            )
            .or_default() += 1;
    }
    let mut kinds: Vec<(String, u32)> = kinds.into_iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    kinds.truncate(4);
    let md: Vec<PathBuf> = fs
        .iter()
        .filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")))
        .cloned()
        .collect();
    let mut docs = Vec::new();
    for d in &md {
        if *docs_left == 0 {
            break;
        }
        let small = std::fs::metadata(root.join(d)).is_ok_and(|m| m.len() <= MAX_DOC_BYTES);
        let mut secs = if small {
            std::fs::read_to_string(root.join(d))
                .map(|t| sections(&t))
                .unwrap_or_default()
        } else {
            vec![]
        };
        secs.truncate(MAX_SECTIONS);
        *docs_left -= 1;
        docs.push(Doc {
            file: d.display().to_string(),
            sections: secs,
        });
    }
    Area {
        about: about(root, &md),
        path,
        files: fs.len() as u32,
        kinds,
        docs,
    }
}

/// The map as Markdown, for agents to read in parts.
#[must_use]
pub fn markdown(m: &ProjectMap) -> String {
    let mut out = format!(
        "# Project map\n\nGenerated by Sculptured at each start, from the files in this folder ({} files{}). Areas are folders up to two levels deep. Open a section with the Read tool at its line range instead of reading the whole file.\n",
        m.files,
        if m.truncated {
            ", the first of many"
        } else {
            ""
        }
    );
    for a in &m.areas {
        let kinds: Vec<String> = a.kinds.iter().map(|(k, n)| format!("{k} ×{n}")).collect();
        let name = if a.path.is_empty() {
            "(top level)".to_string()
        } else {
            a.path.clone()
        };
        out.push_str(&format!(
            "\n## {name}\n\n{} files: {}{}\n",
            a.files,
            kinds.join(", "),
            a.about
                .as_ref()
                .map(|s| format!(". {s}"))
                .unwrap_or_default()
        ));
        for d in &a.docs {
            out.push_str(&format!("\n- `{}`\n", d.file));
            for s in &d.sections {
                out.push_str(&format!("  - {} (lines {}–{})\n", s.title, s.line, s.end));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_maps_to_areas_with_their_kinds_notes_and_sections() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("cad/wings")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::write(root.join("cad/wings/spar.step"), "x").unwrap();
        std::fs::write(root.join("cad/wings/rib.step"), "x").unwrap();
        std::fs::write(root.join("cad/wings/README.md"), "# Wing parts\n\nSpars and ribs.\n\n## Spar\n\nmain spar\n\n```\n# not a heading\n```\n## Rib\nrib\n").unwrap();
        std::fs::write(root.join("node_modules/x/a.js"), "x").unwrap();
        let m = build(root).unwrap();
        assert!(
            m.areas.iter().all(|a| !a.path.starts_with("node_modules")),
            "noise folders are left out"
        );
        let wings = m
            .areas
            .iter()
            .find(|a| a.path == "cad/wings")
            .expect("cad/wings");
        assert_eq!(
            (wings.files, wings.about.as_deref()),
            (3, Some("Wing parts"))
        );
        assert_eq!(wings.kinds[0], ("step".to_string(), 2));
        let titles: Vec<(&str, u32, u32)> = wings.docs[0]
            .sections
            .iter()
            .map(|s| (s.title.as_str(), s.line, s.end))
            .collect();
        assert_eq!(
            titles,
            [("Wing parts", 1, 4), ("Spar", 5, 11), ("Rib", 12, 13)]
        );
        let written = std::fs::read_to_string(root.join(".sculptured/map.md")).unwrap();
        assert!(
            written.contains("## cad/wings") && written.contains("Spar (lines 5–11)"),
            "{written}"
        );
    }

    #[test]
    fn shared_values_are_found_with_their_line_or_named_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.py"), "HOST = 'x'\nPORT = 8080\n").unwrap();
        let v = |value: &str, file: &str| model::SharedValue {
            name: "port".into(),
            value: value.into(),
            file: file.into(),
        };
        let m = build_checked(
            dir.path(),
            &[
                v("8080", "config.py"),
                v("9090", "config.py"),
                v("8080", "gone.py"),
                v("x", "../outside.py"),
            ],
        )
        .unwrap();
        let got: Vec<(&str, Option<u32>)> = m
            .shared
            .iter()
            .map(|c| (c.status.as_str(), c.line))
            .collect();
        assert_eq!(
            got,
            [
                ("found", Some(2)),
                ("missing", None),
                ("no-file", None),
                ("no-file", None)
            ]
        );
    }

    #[test]
    fn a_kept_lesson_is_written_once_and_read_back_by_the_next_map() {
        let dir = tempfile::tempdir().unwrap();
        assert!(keep_lesson(dir.path(), "Money helpers   live in money.py.").unwrap());
        assert!(
            !keep_lesson(dir.path(), "Money helpers live in money.py.").unwrap(),
            "the same lesson is not kept twice"
        );
        assert!(keep_lesson(dir.path(), "[constraint] Never use float for money.").unwrap());
        assert_eq!(
            build(dir.path()).unwrap().kept,
            [
                "Money helpers live in money.py.",
                "[constraint] Never use float for money."
            ]
        );
    }
}
