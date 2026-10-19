//! Where a run's tokens went: MAIS runs on the computer that ran the team, over that run's Claude Code
//! and Codex session files, and answers a `mais.report/v1` report. The report is kept per run,
//! refreshed every minute while the run goes and once after it ends.
use crate::error::{respond, ApiError};
use crate::extract::{Path, Query};
use crate::state::AppState;
use crate::store::Store;
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

/// The run events MAIS reads: which CLI session each agent ran, handoffs, plan readings, budget changes.
const LINKS: &[&str] = &["tenure-started", "handed-over", "plan-usage-reported", "budget-state-changed"];
/// A running run's report is refreshed when it is older than this.
const FRESH_MS: i64 = 90_000;
const EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// A run's report, as the board reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunReport {
    #[ts(type = "number")]
    pub start_seq: i64,
    /// When it was made.
    #[ts(type = "number")]
    pub at_ms: i64,
    /// An agent of the run was still alive then.
    pub running: bool,
    /// `mais.report/v1`.
    #[ts(type = "import('../tokensReport').TokensReport")]
    pub report: serde_json::Value,
    /// Why it could not be brought up to date, when it is older than the run.
    pub stale: Option<String>,
}

impl Store {
    fn put_run_report(&self, rig: &str, r: &RunReport) -> rusqlite::Result<()> {
        let json = serde_json::to_string(r).expect("a report serializes");
        self.connection().execute(
            "INSERT INTO run_reports (rig, start_seq, json, at_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (rig, start_seq) DO UPDATE SET json = excluded.json, at_ms = excluded.at_ms",
            params![rig, r.start_seq, json, r.at_ms],
        )?;
        Ok(())
    }

    fn run_report(&self, rig: &str, start_seq: i64) -> rusqlite::Result<Option<RunReport>> {
        let json: Option<String> = self
            .connection()
            .query_row("SELECT json FROM run_reports WHERE rig = ?1 AND start_seq = ?2", params![rig, start_seq], |r| r.get(0))
            .optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/report", get(get_report))
}

#[derive(Deserialize)]
struct ReportQuery {
    start: Option<i64>,
}

async fn get_report(State(st): State<AppState>, Path(id): Path<String>, Query(q): Query<ReportQuery>) -> Response {
    respond(report(&st, &id, q.start).await)
}

/// The report of `team`'s run starting at `start` (its latest run when None): the kept one while it is
/// current, else a new one; the kept one, marked stale, when a new one cannot be made.
pub async fn report(st: &AppState, team: &str, start: Option<i64>) -> Result<RunReport, ApiError> {
    let seq = match start {
        Some(s) => s,
        None => st.store.lock().unwrap().latest_run_start(team)?.ok_or_else(|| ApiError::not_found("This team has not run yet."))?,
    };
    let (kept, ended, going) = {
        let store = st.store.lock().unwrap();
        (store.run_report(team, seq)?, ended_ms(&store, team, seq)?, live(&store)?.contains(team))
    };
    let current = kept.as_ref().is_some_and(|k| match ended {
        Some(e) => k.at_ms >= e,
        None if going => model::now_ms() - k.at_ms < FRESH_MS,
        // Every agent ended without a Stop of the team (a budget's hard stop): made after that, it holds.
        None => !k.running,
    });
    if current {
        return Ok(kept.expect("current"));
    }
    match refresh(st, team, seq).await {
        Ok(r) => Ok(r),
        Err(e) => kept.map(|k| RunReport { stale: Some(e.message.clone()), ..k }).ok_or(e),
    }
}

/// When the run ended: it was stopped, or the next run began; None while it goes.
fn ended_ms(store: &Store, team: &str, seq: i64) -> Result<Option<i64>, ApiError> {
    let (_, end) = crate::collaboration::history::bounds(store, team, seq)?;
    let detail = crate::collaboration::history::load_detail(store, team, seq)?;
    Ok(detail.stopped_ms.or(store.event_at_ms(end)?))
}

/// The CLI sessions the run's agents ran, from its `tenure-started` lines.
fn sessions(export: &str) -> Vec<localops::analyze::RunSession> {
    let mut seen = BTreeSet::new();
    let lines = export.lines().skip(1).filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok());
    lines
        .filter_map(|v| {
            let b = &v["event"]["body"];
            let id = b["native_session_id"].as_str()?.to_string();
            let runtime = if b["driver"].as_str()?.starts_with("codex") { "codex" } else { "claude" };
            seen.insert(id.clone()).then(|| localops::analyze::RunSession { runtime: runtime.into(), id })
        })
        .collect()
}

/// Makes and keeps a new report of the run.
pub async fn refresh(st: &AppState, team: &str, seq: i64) -> Result<RunReport, ApiError> {
    let (export, ended) = crate::portable::session::export_events(st, team, seq, Some(LINKS))?;
    let export = with_team_for_mais(&st.store.lock().unwrap(), team, seq, export)?;
    let sessions = sessions(&export);
    if sessions.is_empty() {
        return Err(ApiError::conflict("no_sessions", "No agent of this run has started a Claude Code or Codex session yet."));
    }
    let op = localops::LocalOp::AnalyzeRun { sessions, export };
    let report = crate::ops::run(st, crate::ops::Target::Team(team), op).await.map_err(older_runner)?;
    let going = ended.is_none() && live(&st.store.lock().unwrap())?.contains(team);
    let r = RunReport { start_seq: seq, at_ms: model::now_ms(), running: going, report, stale: None };
    st.store.lock().unwrap().put_run_report(team, &r)?;
    Ok(r)
}

/// A runner from before reports answers that it does not know the request.
fn older_runner(e: ApiError) -> ApiError {
    if e.code != "unknown_op" {
        return e;
    }
    ApiError::conflict("runner_without_reports", "This computer's Sculptured runner is older than reports of where tokens went. Reinstall it from Computers.")
}

/// Teams with an agent running or starting.
fn live(store: &Store) -> rusqlite::Result<BTreeSet<String>> {
    let mut q = store.connection().prepare("SELECT DISTINCT rig FROM seats WHERE lifecycle IN ('running', 'launching')")?;
    let rows = q.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// Every minute: refreshes the latest run's report of each live team, and once more after it stops.
pub async fn run(st: AppState) {
    let mut tick = tokio::time::interval(EVERY);
    let mut was = BTreeSet::new();
    loop {
        tick.tick().await;
        let Ok(now) = live(&st.store.lock().unwrap()) else { continue };
        for team in now.union(&was) {
            let Ok(Some(seq)) = st.store.lock().unwrap().latest_run_start(team) else { continue };
            if let Err(e) = refresh(&st, team, seq).await {
                tracing::debug!(%team, code = %e.code, "no run report: {}", e.message);
            }
        }
        was = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_session_is_asked_for_once_with_its_cli() {
        let line = |driver: &str, id: &str| format!(r#"{{"event":{{"body":{{"type":"tenure-started","driver":"{driver}","native_session_id":"{id}"}}}}}}"#);
        let export = ["{}".to_string(), line("claude-stream", "a"), line("codex-app", "b"), line("claude-stream", "a")].join("\n");
        let got: Vec<(String, String)> = sessions(&export).into_iter().map(|s| (s.runtime, s.id)).collect();
        assert_eq!(got, [("claude".into(), "a".into()), ("codex".into(), "b".into())]);
    }

    #[test]
    fn a_runner_that_does_not_know_the_request_is_told_to_update() {
        let e: ApiError = localops::LocalError::bad("unknown_op", "unknown variant").into();
        assert_eq!(older_runner(e).code, "runner_without_reports");
        assert_eq!(older_runner(ApiError::not_found("x")).code, "not_found");
    }
}

/// MAIS names a run's agents from the header's team in Sculptured's own shape (`team.team`), not
/// from MADL, so the export the runner analyses carries that shape. Only this message to the runner
/// does; a run file people download carries the team as MADL.
fn with_team_for_mais(store: &Store, team: &str, seq: i64, export: String) -> Result<String, ApiError> {
    let Some((first, rest)) = export.split_once('\n') else { return Ok(export) };
    let mut header: serde_json::Value = serde_json::from_str(first).map_err(|e| ApiError::internal(e.to_string()))?;
    let snapshot = store.run_team_snapshot(team, seq)?.and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok());
    header["team"] = snapshot.unwrap_or(serde_json::Value::Null);
    Ok(format!("{header}\n{rest}"))
}
