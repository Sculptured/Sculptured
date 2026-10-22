//! How much of each signed-in plan is used, as the CLIs report it: the latest report from any agent's
//! session (Claude `rate_limit_event`, Codex `account/rateLimits/updated`), or Codex's own session files,
//! which also count use outside Sculptured, whichever is newer. Claude keeps no such file, so its
//! reading exists only once a Claude agent has run.
//!
//! New work (starting a team, spawning a subagent) is held while a window that has not reset is at or
//! past `HOLD_PERCENT`. Work already running carries on; token limits bound it.
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router};
use model::PlanUsage;
use rusqlite::params;
use std::path::PathBuf;

pub const HOLD_PERCENT: f64 = 90.0;

pub use localops::plan::PlanUsageView;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/plan-usage", axum::routing::get(list))
}

async fn list(State(st): State<AppState>) -> Json<Vec<PlanUsageView>> {
    let st2 = st.clone();
    let mut out = tokio::task::spawn_blocking(move || current(&st2))
        .await
        .unwrap_or_default();
    // Hosted: Codex's session files are on the user's computer (when exactly one is connected).
    if crate::ops::hosted(&st)
        && st.hosts.connected().len() == 1
        && let Ok(v) = crate::ops::run(
            &st,
            crate::ops::Target::Default,
            localops::LocalOp::PlanUsage,
        )
        .await
        && let Ok(Some(file)) = serde_json::from_value::<Option<PlanUsageView>>(v)
    {
        merge_newer(&mut out, file);
    }
    Json(out)
}

/// Keeps the newer of `file` and the provider's current reading.
fn merge_newer(out: &mut Vec<PlanUsageView>, file: PlanUsageView) {
    match out
        .iter_mut()
        .find(|v| v.usage.provider == file.usage.provider)
    {
        Some(v) if v.observed_ms >= file.observed_ms => {}
        Some(v) => *v = file,
        None => out.push(file),
    }
}

/// Records a report, keeping the newest per provider. Called by the projection.
pub fn project(
    tx: &rusqlite::Transaction,
    usage: &PlanUsage,
    at_ms: i64,
    seat: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO plan_usage (provider, json, at_ms, seat) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(provider) DO UPDATE SET json = excluded.json, at_ms = excluded.at_ms, seat = excluded.seat
         WHERE excluded.at_ms >= plan_usage.at_ms",
        params![usage.provider, serde_json::to_string(usage).expect("usage serializes"), at_ms, seat],
    )?;
    Ok(())
}

/// The newest reading per provider (Claude first).
#[must_use]
pub fn current(st: &AppState) -> Vec<PlanUsageView> {
    let mut out: Vec<PlanUsageView> = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let mut stmt = match store
            .connection()
            .prepare("SELECT json, at_ms, seat FROM plan_usage ORDER BY provider")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map(|rows| {
            rows.flatten()
                .filter_map(|(json, at, seat)| {
                    let usage = serde_json::from_str(&json).ok()?;
                    Some(PlanUsageView {
                        usage,
                        observed_ms: at,
                        source: "agent-session".into(),
                        seat,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
    };
    // This machine's Codex files mean something only in local mode (hosted: the user's computer, in `list`).
    if !crate::ops::hosted(st)
        && let Some(file) = codex_home(st).and_then(|home| localops::plan::codex_files(&home))
    {
        merge_newer(&mut out, file);
    }
    out
}

fn codex_home(st: &AppState) -> Option<PathBuf> {
    match std::env::var_os("CODEX_HOME") {
        Some(h) => Some(PathBuf::from(h)),
        None => st.home.as_deref().map(|h| h.join(".codex")),
    }
}

pub use localops::plan::codex_files;

/// "claude" or "codex" for an agent type; None for the others (they use no plan).
#[must_use]
pub fn provider_of(runtime: rigspec::Runtime) -> Option<&'static str> {
    match runtime {
        rigspec::Runtime::ClaudeCode => Some("claude"),
        rigspec::Runtime::Codex => Some("codex"),
        _ => None,
    }
}

/// Why new work on `provider` is held, or None.
#[must_use]
pub fn held(st: &AppState, provider: &str) -> Option<String> {
    let now_ms = model::now_ms();
    let all = current(st);
    let v = all.iter().find(|v| v.usage.provider == provider)?;
    let w = v.usage.fullest(now_ms / 1000)?;
    let used = w.used_percent?;
    if used < HOLD_PERCENT {
        return None;
    }
    let name = if provider == "claude" {
        "Claude"
    } else {
        "Codex"
    };
    let window = match w.name.as_str() {
        "five_hour" => "5-hour".to_string(),
        "seven_day" => "weekly".to_string(),
        other => other.replace('_', " "),
    };
    let resets = w
        .resets_at
        .map(|r| format!(", resets in {}", span(r * 1000 - now_ms)))
        .unwrap_or_default();
    let credits = match (&v.usage.credits, used >= 100.0) {
        (Some(c), true) => format!("; more use spends paid credits (balance {c})"),
        _ => String::new(),
    };
    Some(format!(
        "{name} plan is at {used:.0}% of its {window} limit{resets}{credits}. New work is held at {HOLD_PERCENT:.0}%."
    ))
}

fn span(ms: i64) -> String {
    let m = (ms.max(0) / 60_000) as u64;
    match (m / 1440, (m % 1440) / 60, m % 60) {
        (0, 0, m) => format!("{m} min"),
        (0, h, m) => format!("{h} h {m} min"),
        (d, h, _) => format!("{d} d {h} h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_work_is_held_at_the_threshold_until_the_window_resets() {
        let st = AppState::new(
            crate::store::Store::open_in_memory().unwrap(),
            "t".into(),
            vec![],
        );
        let later = model::now_ms() / 1000 + 3600;
        let put = |p: f64, resets: i64| {
            let usage = PlanUsage {
                provider: "codex".into(),
                status: None,
                credits: Some("12".into()),
                windows: vec![model::PlanWindow {
                    name: "seven_day".into(),
                    used_percent: Some(p),
                    resets_at: Some(resets),
                }],
            };
            let store = st.store.lock().unwrap();
            let tx = rusqlite::Transaction::new_unchecked(
                store.connection(),
                rusqlite::TransactionBehavior::Deferred,
            )
            .unwrap();
            project(&tx, &usage, model::now_ms(), Some("a@t")).unwrap();
            tx.commit().unwrap();
        };
        put(50.0, later);
        assert_eq!(held(&st, "codex"), None);
        put(100.0, later);
        let why = held(&st, "codex").expect("held");
        assert!(
            why.starts_with("Codex plan is at 100% of its weekly limit, resets in 59 min")
                || why.contains("resets in 1 h 0 min"),
            "{why}"
        );
        assert!(why.contains("paid credits (balance 12)"), "{why}");
        assert_eq!(held(&st, "claude"), None, "no reading is not a hold");
        put(100.0, 1);
        assert_eq!(
            held(&st, "codex"),
            None,
            "a window that has reset holds nothing"
        );
    }

    #[test]
    fn spans_read_plainly() {
        assert_eq!(span(59 * 60_000), "59 min");
        assert_eq!(span((3 * 60 + 5) * 60_000), "3 h 5 min");
        assert_eq!(span((2 * 1440 + 4 * 60) * 60_000), "2 d 4 h");
    }
}
