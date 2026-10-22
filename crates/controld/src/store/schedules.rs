use super::Store;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use ts_rs::TS;

/// One time a team's schedule came due: `sent`, `not delivered: <why>` or `skipped: busy`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ScheduleRun {
    pub schedule: String,
    #[ts(type = "number")]
    pub at_ms: i64,
    pub outcome: String,
}

impl Store {
    pub fn last_schedule_run(&self, team: &str, schedule: &str) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT MAX(at_ms) FROM schedule_runs WHERE team=?1 AND schedule=?2",
                params![team, schedule],
                |r| r.get(0),
            )
            .optional()
            .map(Option::flatten)
    }

    /// Records a run and answers its row, for `set_schedule_outcome`.
    pub fn record_schedule_run(
        &self,
        team: &str,
        schedule: &str,
        outcome: &str,
    ) -> rusqlite::Result<i64> {
        self.conn.execute(
            "INSERT INTO schedule_runs(team, schedule, at_ms, outcome) VALUES (?1,?2,?3,?4)",
            params![team, schedule, model::now_ms(), outcome],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_schedule_outcome(&self, run: i64, outcome: &str) -> rusqlite::Result<()> {
        self.conn
            .execute(
                "UPDATE schedule_runs SET outcome=?2 WHERE rowid=?1",
                params![run, outcome],
            )
            .map(|_| ())
    }

    /// The team's latest schedule runs, newest first.
    pub fn schedule_runs(&self, team: &str, limit: usize) -> rusqlite::Result<Vec<ScheduleRun>> {
        self.conn.prepare("SELECT schedule, at_ms, outcome FROM schedule_runs WHERE team=?1 ORDER BY at_ms DESC, rowid DESC LIMIT ?2")?
            .query_map(params![team, limit as i64], |r| Ok(ScheduleRun { schedule: r.get(0)?, at_ms: r.get(1)?, outcome: r.get(2)? }))?
            .collect()
    }

    /// Whether a message to `seat` is waiting to be taken (queued or delivered, not yet consumed).
    pub fn has_pending_messages(&self, seat: &str) -> rusqlite::Result<bool> {
        self.conn.query_row("SELECT EXISTS(SELECT 1 FROM messages WHERE seat=?1 AND state IN ('queued','delivered'))", [seat], |r| r.get(0))
    }
}
