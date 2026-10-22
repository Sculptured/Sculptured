//! Plan usage the Codex CLI recorded in its own session files (`$CODEX_HOME/sessions/YYYY/MM/DD/*.jsonl`).
//! Claude keeps no such file; its reading comes from agent sessions.
use model::PlanUsage;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlanUsageView {
    pub usage: PlanUsage,
    /// When the CLI reported it (ms).
    #[ts(type = "number")]
    pub observed_ms: i64,
    /// `agent-session`: an agent's CLI reported it; `codex-session-files`: read from `~/.codex/sessions`.
    pub source: String,
    /// The agent whose session reported it, for `agent-session`.
    pub seat: Option<String>,
}

/// The newest `rate_limits` in Codex's session files (`sessions/YYYY/MM/DD/*.jsonl`): the two newest
/// day folders, their four most recently written files, the last 256 KB of each. The file's write
/// time stands for when it was reported.
#[must_use]
pub fn codex_files(codex_home: &Path) -> Option<PlanUsageView> {
    recent_files(&recent_days(codex_home))
        .into_iter()
        .take(4)
        .find_map(|(modified, path)| {
            let usage = last_rate_limits(&path)?;
            let observed_ms = modified.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
            Some(PlanUsageView {
                usage,
                observed_ms,
                source: "codex-session-files".into(),
                seat: None,
            })
        })
}

fn sorted_dirs(path: &Path) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    directories.reverse();
    directories
}

fn recent_days(codex_home: &Path) -> Vec<PathBuf> {
    let mut days = Vec::new();
    'outer: for y in sorted_dirs(&codex_home.join("sessions")) {
        for m in sorted_dirs(&y) {
            for d in sorted_dirs(&m) {
                days.push(d);
                if days.len() == 2 {
                    break 'outer;
                }
            }
        }
    }
    days
}

fn recent_files(days: &[PathBuf]) -> Vec<(SystemTime, PathBuf)> {
    let mut files: Vec<(SystemTime, PathBuf)> = days
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|(modified, _)| Reverse(*modified));
    files
}

fn last_rate_limits(path: &Path) -> Option<PlanUsage> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(256 * 1024)))
        .ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .rev()
        .filter(|l| l.contains("\"rate_limits\""))
        .find_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            PlanUsage::from_codex(&v["payload"]["rate_limits"])
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_newest_rate_limits_from_codex_session_files() {
        let dir = tempfile::tempdir().unwrap();
        let day = dir.path().join("sessions/2026/10/11");
        std::fs::create_dir_all(&day).unwrap();
        let line = |p: f64| {
            format!(
                r#"{{"type":"event_msg","payload":{{"type":"token_count","rate_limits":{{"primary":{{"used_percent":{p},"window_minutes":10080,"resets_at":1}},"secondary":null,"credits":null}}}}}}"#
            )
        };
        std::fs::write(
            day.join("a.jsonl"),
            format!("{}\n{}\n{{\"type\":\"other\"}}\n", line(40.0), line(96.0)),
        )
        .unwrap();
        let v = codex_files(dir.path()).expect("found");
        assert_eq!(
            (v.usage.windows[0].used_percent, v.source.as_str()),
            (Some(96.0), "codex-session-files")
        );
        assert!(codex_files(&dir.path().join("missing")).is_none());
    }
}
