//! One run's budget, as recorded at Start, and what has been measured against it since.
use crate::store::Store;
use model::RunBudget;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// A plan reading during the run: the window's percent and the run's tokens on that plan when it came.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub percent: f64,
    pub tokens: u64,
    pub at_ms: i64,
}

/// One budgeted plan window, followed through the run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanTrack {
    pub provider: String,
    pub window: String,
    /// The window's percent when counting began: at Start, or at the first reading after it.
    pub baseline: Option<f64>,
    /// The run's tokens on this plan before the baseline was read (counted with the rate).
    pub base_tokens: u64,
    /// When the window being counted resets (a later value means a new window began).
    pub resets_at: Option<i64>,
    pub first: Option<Reading>,
    pub last: Option<Reading>,
    /// The highest share measured, in percent of the window.
    pub peak: f64,
}

/// Where the run stands against its budget.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "kebab-case")]
#[ts(export, export_to = "BudgetState.ts")]
pub enum BudgetState {
    #[default]
    Normal,
    /// The soft value was reached: no new work, every agent wraps up.
    WrapUp {
        reason: String,
        #[ts(type = "number")]
        at_ms: i64,
    },
    /// The hard value was reached: every agent was stopped at once.
    Stopped {
        reason: String,
        #[ts(type = "number")]
        at_ms: i64,
    },
    /// A plan budget's hard value was reached: every agent was stopped, and the run continues by itself
    /// at `until_ms`, after the window resets, in window `window + 1` of `max_windows`.
    Paused {
        reason: String,
        #[ts(type = "number")]
        at_ms: i64,
        #[ts(type = "number")]
        until_ms: i64,
        window: u32,
        max_windows: u32,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub budget: RunBudget,
    pub start_seq: i64,
    pub start_ms: i64,
    pub plans: Vec<PlanTrack>,
    pub state: BudgetState,
    /// When the run wrapped up, kept after it is stopped (the history shows both).
    pub wrap_up_ms: Option<i64>,
    /// Highest token count measured.
    pub peak_tokens: u64,
    /// Windows the run has continued into after a pause.
    #[serde(default)]
    pub windows: u32,
    /// Time spent paused, which the time budget does not count.
    #[serde(default)]
    pub paused_ms: i64,
    /// The configured agents that were working when the run paused: the ones that continue.
    #[serde(default)]
    pub paused_seats: Vec<String>,
    /// How they continue, when the person changed it while paused (else the budget's choice).
    #[serde(default)]
    pub continue_with: Option<model::ContinueMode>,
    /// Agents' own limits a person raised for this run, by the agent's id in the design (the design
    /// keeps the planned values).
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub agent_limits: std::collections::BTreeMap<String, AgentLimits>,
}

/// An agent's raised limits for one run; absent ones are the design's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentLimits {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_limit: Option<model::TokenLimit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_limit: Option<model::SoftHard<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute_limit: Option<model::SoftHard<u32>>,
}

impl Store {
    pub fn put_run_budget(&self, rig: &str, r: &Record) -> rusqlite::Result<()> {
        self.connection().execute(
            "INSERT INTO run_budgets (rig, start_seq, json) VALUES (?1, ?2, ?3)
             ON CONFLICT (rig, start_seq) DO UPDATE SET json = excluded.json",
            params![
                rig,
                r.start_seq,
                serde_json::to_string(r).expect("budget record serializes")
            ],
        )?;
        Ok(())
    }

    /// The latest run's budget record of `rig`, if that run had a budget.
    /// The cap a computer's owner set: most agents working at once on it, across its teams.
    pub fn host_limit(&self, host: &str) -> rusqlite::Result<Option<u32>> {
        self.connection()
            .query_row(
                "SELECT max_working FROM host_limits WHERE host = ?1",
                [host],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|n| n.map(|n| n.max(1) as u32))
    }

    /// Sets or clears (`None`) a computer's cap.
    pub fn set_host_limit(&self, host: &str, max_working: Option<u32>) -> rusqlite::Result<()> {
        match max_working {
            Some(n) => self.connection().execute("INSERT INTO host_limits (host, max_working) VALUES (?1, ?2) ON CONFLICT(host) DO UPDATE SET max_working = excluded.max_working", rusqlite::params![host, n as i64])?,
            None => self.connection().execute("DELETE FROM host_limits WHERE host = ?1", [host])?,
        };
        Ok(())
    }

    pub fn run_budget(&self, rig: &str) -> rusqlite::Result<Option<Record>> {
        let json: Option<String> = self
            .connection()
            .query_row(
                "SELECT json FROM run_budgets WHERE rig = ?1 ORDER BY start_seq DESC LIMIT 1",
                [rig],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }

    /// Every rig whose latest run has a budget.
    pub fn budgeted_rigs(&self) -> rusqlite::Result<Vec<String>> {
        let mut q = self
            .connection()
            .prepare("SELECT DISTINCT rig FROM run_budgets")?;
        let rows = q.query_map([], |r| r.get(0))?;
        rows.collect()
    }
}
