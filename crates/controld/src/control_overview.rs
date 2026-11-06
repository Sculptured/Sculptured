//! The organisation overview (display level 1 of Control): every team as a unit with its state, its
//! alarms (agents failed or needing attention, open questions, refused or failed tool calls) and its
//! key figures (tool calls per minute over the last 15 minutes, the latest call, the latest run's
//! tokens and cost). `GET /api/control/overview`. Read from the store's projections only.
use crate::error::{ApiError, respond};
use crate::state::AppState;
use crate::store::{RigSource, Store, overview::ActionRow};
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use model::Desired;
use serde::Serialize;
use ts_rs::TS;

/// Minutes the per-minute rate covers.
pub const RATE_MINUTES: usize = 15;

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ControlOverview {
    #[ts(type = "number")]
    pub at_ms: i64,
    pub teams: Vec<OverviewTeam>,
}

/// One tool call: when, by which agent, which tool.
#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct OverviewAction {
    #[ts(type = "number")]
    pub at_ms: i64,
    pub seat: String,
    pub tool: String,
}

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct OverviewTeam {
    pub id: String,
    pub name: String,
    /// Whether it was ever started; `desired` is what it was last set to (None: never started).
    pub started: bool,
    pub desired: Option<Desired>,
    pub agents: u32,
    pub running: u32,
    pub attention: u32,
    pub failed: u32,
    pub open_questions: u32,
    /// Tool calls in each of the last 15 minutes, oldest first.
    pub actions_per_min: Vec<u32>,
    /// Tool calls in those minutes whose result was an error.
    pub failed_actions: u32,
    pub last_action: Option<OverviewAction>,
    /// The latest run's usage: tokens, and the cost of the reports that had one.
    #[ts(type = "number")]
    pub run_tokens: u64,
    pub run_cost_usd: Option<f64>,
    /// True when every report of the run had a price (else the cost is a floor).
    pub run_cost_complete: bool,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/control/overview", get(overview))
}

async fn overview(State(st): State<AppState>) -> Response {
    respond(read(&st, model::now_ms()))
}

/// Calls per minute for the last RATE_MINUTES before `now`, oldest first, and how many failed.
fn rate(actions: &[&ActionRow], now: i64) -> (Vec<u32>, u32) {
    let mut out = vec![0u32; RATE_MINUTES];
    let mut failed = 0;
    for a in actions {
        let minute = ((now - a.at_ms).max(0) / 60_000) as usize;
        if minute < RATE_MINUTES {
            out[RATE_MINUTES - 1 - minute] += 1;
            failed += u32::from(a.ok == Some(false));
        }
    }
    (out, failed)
}

/// The overview at `now`: one unit per team design, in the order the store lists them.
pub fn read(st: &AppState, now: i64) -> Result<ControlOverview, ApiError> {
    let summaries = crate::teams::summaries(st)?;
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let recent = store.actions_since(now - (RATE_MINUTES as i64) * 60_000)?;
    let last = store.last_actions()?;
    let teams = summaries
        .into_iter()
        .map(|t| team(&store, t, &recent, &last, now))
        .collect::<Result<_, _>>()?;
    Ok(ControlOverview { at_ms: now, teams })
}

fn team(
    store: &Store,
    t: crate::views::TeamSummary,
    recent: &[ActionRow],
    last: &[ActionRow],
    now: i64,
) -> Result<OverviewTeam, ApiError> {
    let counts = store.seat_counts(&t.id)?;
    let mine: Vec<&ActionRow> = recent.iter().filter(|a| a.rig == t.id).collect();
    let (actions_per_min, failed_actions) = rate(&mine, now);
    let usage = match (store.rig_source(&t.id)?, store.latest_run_start(&t.id)?) {
        (Some(RigSource::Team { .. }), Some(from)) => Some(store.run_usage(&t.id, from)?),
        _ => None,
    };
    Ok(OverviewTeam {
        started: t.started.is_some(),
        desired: t.started.as_ref().map(|s| s.desired),
        agents: counts.total.max(t.agent_count),
        running: counts.running,
        attention: counts.attention,
        failed: counts.failed,
        open_questions: counts.open_questions,
        actions_per_min,
        failed_actions,
        last_action: last.iter().find(|a| a.rig == t.id).map(|a| OverviewAction {
            at_ms: a.at_ms,
            seat: a.seat.clone(),
            tool: a.tool.clone(),
        }),
        run_tokens: usage.as_ref().map_or(0, |u| u.tokens),
        run_cost_usd: usage.as_ref().and_then(|u| u.cost_usd),
        run_cost_complete: usage.as_ref().is_some_and(|u| u.unpriced == 0),
        id: t.id,
        name: t.name,
    })
}

#[cfg(test)]
mod tests {
    use super::{RATE_MINUTES, rate};
    use crate::store::overview::ActionRow;

    fn at(at_ms: i64, ok: Option<bool>) -> ActionRow {
        ActionRow {
            rig: "r".into(),
            at_ms,
            seat: "s".into(),
            tool: "Read".into(),
            ok,
        }
    }

    #[test]
    fn calls_fall_in_their_minute_and_failed_ones_are_counted() {
        let now = 20 * 60_000;
        let rows = [
            at(now - 10_000, Some(true)),
            at(now - 70_000, Some(false)),
            at(now - 16 * 60_000, Some(false)),
        ];
        let refs: Vec<&ActionRow> = rows.iter().collect();
        let (per_min, failed) = rate(&refs, now);
        assert_eq!(per_min.len(), RATE_MINUTES);
        assert_eq!(&per_min[RATE_MINUTES - 2..], &[1, 1]);
        assert_eq!(per_min.iter().sum::<u32>(), 2);
        assert_eq!(failed, 1);
    }
}
