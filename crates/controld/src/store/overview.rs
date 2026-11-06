//! Reads for the organisation overview (`control_overview`): recent and last tool calls per team,
//! open questions, agents by state, and the latest run's usage. Summed by SQLite; no event parsed here.
use super::Store;
use rusqlite::params;

/// One tool call as the `actions` projection keeps it.
pub struct ActionRow {
    pub rig: String,
    pub at_ms: i64,
    pub seat: String,
    pub tool: String,
    /// The call's result: true ok, false an error, None not back yet.
    pub ok: Option<bool>,
}

/// A team's agents by state, from the seats table.
#[derive(Default)]
pub struct SeatCounts {
    pub total: u32,
    pub running: u32,
    pub attention: u32,
    pub failed: u32,
    pub open_questions: u32,
}

/// The usage reports of a run: tokens, the priced cost, and how many reports had no price.
pub struct RunUsage {
    pub tokens: u64,
    pub cost_usd: Option<f64>,
    pub unpriced: u64,
}

impl Store {
    /// Every tool call made at or after `since_ms`, oldest first.
    pub fn actions_since(&self, since_ms: i64) -> rusqlite::Result<Vec<ActionRow>> {
        let mut q = self.conn.prepare_cached(
            "SELECT rig, at_ms, seat, tool, ok FROM actions WHERE at_ms >= ?1 ORDER BY at_ms",
        )?;
        q.query_map([since_ms], action_row)?.collect()
    }

    /// Each team's latest tool call.
    pub fn last_actions(&self) -> rusqlite::Result<Vec<ActionRow>> {
        // SQLite returns the other columns from the row that holds the MAX.
        let mut q = self
            .conn
            .prepare_cached("SELECT rig, MAX(at_ms), seat, tool, ok FROM actions GROUP BY rig")?;
        q.query_map([], action_row)?.collect()
    }

    /// `rig`'s agents by state and its open questions.
    pub fn seat_counts(&self, rig: &str) -> rusqlite::Result<SeatCounts> {
        self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(lifecycle = 'running'), 0), COALESCE(SUM(lifecycle IN ('attention', 'stale')), 0),
                    COALESCE(SUM(lifecycle = 'failed'), 0), COALESCE(SUM(open_questions), 0)
             FROM seats WHERE rig = ?1",
            [rig],
            |r| {
                Ok(SeatCounts {
                    total: r.get::<_, i64>(0)? as u32,
                    running: r.get::<_, i64>(1)? as u32,
                    attention: r.get::<_, i64>(2)? as u32,
                    failed: r.get::<_, i64>(3)? as u32,
                    open_questions: r.get::<_, i64>(4)? as u32,
                })
            },
        )
    }

    /// The usage reports of `team`'s run from event `from` on (turn totals only; a report counted
    /// within a turn is never priced and is part of its turn's total).
    pub fn run_usage(&self, team: &str, from: i64) -> rusqlite::Result<RunUsage> {
        let sql = format!(
            "SELECT COALESCE(SUM(json_extract(json, '$.body.input_tokens') + json_extract(json, '$.body.output_tokens')
                        + json_extract(json, '$.body.cache_read_tokens') + json_extract(json, '$.body.cache_write_tokens')), 0),
                    SUM(json_extract(json, '$.body.cost.usd')),
                    COALESCE(SUM(json_extract(json, '$.body.cost.usd') IS NULL), 0)
             FROM events WHERE {} AND json_extract(json, '$.body.type') = 'usage-reported'
               AND COALESCE(json_extract(json, '$.body.in_turn'), 0) = 0",
            super::runs::IN_RUN
        );
        let (rig, suffix) = (format!("rig:{team}"), format!("@{team}"));
        self.conn
            .query_row(&sql, params![from, i64::MAX, rig, suffix], |r| {
                Ok(RunUsage {
                    tokens: r.get::<_, i64>(0)? as u64,
                    cost_usd: r.get(1)?,
                    unpriced: r.get::<_, i64>(2)? as u64,
                })
            })
    }
}

fn action_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ActionRow> {
    Ok(ActionRow {
        rig: r.get(0)?,
        at_ms: r.get(1)?,
        seat: r.get(2)?,
        tool: r.get(3)?,
        ok: r.get::<_, Option<i64>>(4)?.map(|v| v != 0),
    })
}
