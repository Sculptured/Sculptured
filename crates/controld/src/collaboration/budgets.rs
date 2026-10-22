//! Per-agent output budgets, inherited by every managed descendant. Usage is delayed CLI evidence.
use crate::error::ApiError;
use crate::state::AppState;
use crate::store::Store;
use model::SeatLifecycle;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct AgentBudget {
    pub seat: String,
    /// The agent's id in the team design (for raising its limits); empty for agents it spawned.
    pub member: String,
    pub output_used: Option<u32>,
    pub exhausted: bool,
    /// The agent's own token limit, if set, and what counts against it in this run.
    pub token_limit: Option<model::TokenLimit>,
    #[ts(type = "number | null")]
    pub token_used: Option<u64>,
    /// The agent's own turn and minute limits, as raised for this run.
    pub turn_limit: Option<model::SoftHard<u32>>,
    pub minute_limit: Option<model::SoftHard<u32>>,
}
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TeamBudget {
    pub output_limit: Option<u32>,
    pub agents: Vec<AgentBudget>,
    /// The latest run's budget and where it stands; absent when that run had none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub run: Option<crate::budget::RunStatus>,
    /// Agents with a message waiting for a free slot ("most agents working at once").
    pub waiting: Vec<String>,
}
impl Store {
    pub fn output_limit(&self, rig: &str) -> rusqlite::Result<Option<u32>> {
        self.connection()
            .query_row(
                "SELECT output_limit FROM team_output_budgets WHERE rig=?1",
                [rig],
                |r| r.get(0),
            )
            .optional()
    }
    pub fn budget_used(&self, seat: &str) -> rusqlite::Result<Option<u32>> {
        self.connection().query_row("SELECT MAX(0,s.tokens_out-COALESCE(b.baseline,0)) FROM seats s LEFT JOIN agent_budget_baselines b ON b.seat=s.id WHERE s.id=?1", [seat], |r| r.get::<_, Option<i64>>(0)).optional().map(|v| v.flatten().map(|n| n.min(u32::MAX as i64) as u32))
    }
    pub fn budget_exhausted(&self, seat: &str) -> rusqlite::Result<bool> {
        let Some(view) = self.seat(seat)? else {
            return Ok(false);
        };
        let output = self
            .output_limit(&view.rig)?
            .zip(self.budget_used(seat)?)
            .is_some_and(|(limit, used)| used >= limit);
        Ok(output || super::token_limits::reached(self, &view)?)
    }
}
pub fn status(st: &AppState, rig: &str) -> Result<TeamBudget, ApiError> {
    let run = crate::budget::status(st, rig)?;
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let waiting = crate::budget::gate::seats_waiting(&store, rig)?;
    let output_limit = store.output_limit(rig)?;
    let mut agents = Vec::new();
    if let Some(snap) = store.rig_snapshot(rig)? {
        for seat in snap.seats {
            let used = store.budget_used(&seat.id)?;
            let token_limit = super::token_limits::limit_of(&store, &seat)?;
            let token_used = match &token_limit {
                Some(l) => super::token_limits::counted(&store, &seat, l)?,
                None => None,
            };
            let reached = token_limit
                .zip(token_used)
                .is_some_and(|(l, u)| u >= l.tokens);
            let exhausted = reached
                || output_limit
                    .zip(used)
                    .is_some_and(|(limit, used)| used >= limit);
            let adv = store
                .draft(rig)?
                .and_then(|d| d.agents.into_iter().find(|a| a.id == seat.member))
                .map(|a| a.advanced);
            let adv = match adv {
                Some(a) => Some(super::token_limits::effective(
                    &store,
                    rig,
                    &seat.member,
                    &a,
                )?),
                None => None,
            };
            let (turn_limit, minute_limit) = adv
                .map(|a| (a.turn_limit, a.minute_limit))
                .unwrap_or_default();
            agents.push(AgentBudget {
                seat: seat.id,
                member: seat.member,
                output_used: used,
                exhausted,
                token_limit,
                token_used,
                turn_limit,
                minute_limit,
            });
        }
    }
    Ok(TeamBudget {
        output_limit,
        agents,
        run,
        waiting,
    })
}
pub async fn check(st: &AppState) -> Result<(), ApiError> {
    let due = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let mut due = Vec::new();
        for rig in store.list_rigs()? {
            if store.output_limit(&rig.name)?.is_none() {
                continue;
            }
            if let Some(snap) = store.rig_snapshot(&rig.name)? {
                for view in snap.seats {
                    if matches!(
                        view.lifecycle,
                        SeatLifecycle::Running
                            | SeatLifecycle::Launching
                            | SeatLifecycle::Stale
                            | SeatLifecycle::Attention
                    ) && store.budget_exhausted(&view.id)?
                    {
                        due.push(view.id);
                    }
                }
            }
        }
        due
    };
    for seat in due {
        // Hard stop includes descendants, preventing a depleted coordinator from leaving work behind.
        if let Err(e) = super::control_agent(st, &seat, super::Control::Hard).await {
            tracing::warn!(%seat,error=%e.message,"budget stop will retry");
            continue;
        }
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .set_control_mode(&seat, crate::store::ControlMode::Budget)?;
    }
    Ok(())
}
