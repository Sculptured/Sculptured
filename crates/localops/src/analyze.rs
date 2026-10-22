//! Where a team run's tokens went (MAIS), from the Claude Code and Codex session files on this
//! computer. Only the report leaves it: sizes, causes, commands and file names, never file contents.
use crate::{Context, LocalError};
use std::path::PathBuf;

/// One of the run's CLI sessions: `claude` or `codex`, and the CLI's own session id.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunSession {
    pub runtime: String,
    pub id: String,
}

/// `$CLAUDE_CONFIG_DIR/projects`, else `~/.claude/projects`.
fn claude_projects(cx: &Context) -> PathBuf {
    let root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map_or_else(|| cx.home.join(".claude"), PathBuf::from);
    root.join("projects")
}

fn files(cx: &Context, sessions: &[RunSession], started_ms: i64) -> Vec<PathBuf> {
    let (claude, codex) = (claude_projects(cx), cx.codex_home.join("sessions"));
    let find = |s: &RunSession| match s.runtime.as_str() {
        "codex" => mais::locate::codex_session(&codex, &s.id, started_ms),
        _ => mais::locate::claude_session(&claude, &s.id),
    };
    sessions.iter().filter_map(find).collect()
}

/// The `mais.report/v1` report of the run `export` describes, from `sessions`' files.
pub fn run(
    cx: &Context,
    sessions: &[RunSession],
    export: &str,
) -> Result<serde_json::Value, LocalError> {
    let x =
        mais::adapters::sculptured::read(export).map_err(|e| LocalError::bad("bad_export", e))?;
    let files = files(cx, sessions, x.started_ms);
    if files.is_empty() {
        return Err(LocalError::not_found(
            "no_logs",
            "None of this run's Claude Code or Codex session files are on this computer.",
        ));
    }
    let report = mais::report(&files, None, Some(&x), &mais::link::Selection::default())
        .map_err(|e| LocalError::not_found("no_logs", e))?;
    Ok(serde_json::to_value(report).expect("a report serializes"))
}
