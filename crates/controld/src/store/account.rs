//! Raw rows for the account page's usage summary (`crate::account`): every token report with its team
//! and CLI, and a few activity counts. All read from the event journal and the team designs.
use super::Store;
use rusqlite::params;

/// One token report the CLI made, with the agent's team and runtime.
pub struct UsageRow {
    pub team: String,
    pub runtime: String,
    pub at_ms: i64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost_usd: Option<f64>,
    pub cost_source: Option<String>,
    /// Counted from one model call while a turn ran: never priced (the turn's last report is).
    pub in_turn: bool,
}

pub struct ActivityCounts {
    pub teams: u64,
    pub turns_completed: u64,
    pub agent_sessions: u64,
    pub messages_you_sent: u64,
}

impl Store {
    pub fn usage_rows(&self) -> rusqlite::Result<Vec<UsageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.rig, s.runtime, e.at_ms,
                    COALESCE(json_extract(e.json, '$.body.input_tokens'), 0), COALESCE(json_extract(e.json, '$.body.output_tokens'), 0),
                    COALESCE(json_extract(e.json, '$.body.cache_read_tokens'), 0), COALESCE(json_extract(e.json, '$.body.cache_write_tokens'), 0),
                    json_extract(e.json, '$.body.cost.usd'), json_extract(e.json, '$.body.cost.source'),
                    COALESCE(json_extract(e.json, '$.body.in_turn'), 0)
             FROM events e JOIN seats s ON e.subject = 'seat:' || s.id
             WHERE json_extract(e.json, '$.body.type') = 'usage-reported'",
        )?;
        let rows = stmt.query_map(params![], |r| {
            Ok(UsageRow {
                team: r.get(0)?,
                runtime: r.get(1)?,
                at_ms: r.get(2)?,
                input: r.get::<_, i64>(3)?.max(0) as u64,
                output: r.get::<_, i64>(4)?.max(0) as u64,
                cache_read: r.get::<_, i64>(5)?.max(0) as u64,
                cache_write: r.get::<_, i64>(6)?.max(0) as u64,
                cost_usd: r.get(7)?,
                cost_source: r.get(8)?,
                in_turn: r.get(9)?,
            })
        })?;
        rows.collect()
    }

    /// Team names by id, from the saved designs.
    pub fn team_names(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, COALESCE(json_extract(json, '$.name'), id) FROM drafts")?;
        let rows = stmt.query_map(params![], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    pub fn activity_counts(&self) -> rusqlite::Result<ActivityCounts> {
        let count = |sql: &str| {
            self.conn
                .query_row(sql, params![], |r| r.get::<_, i64>(0))
                .map(|n| n.max(0) as u64)
        };
        Ok(ActivityCounts {
            teams: count("SELECT COUNT(*) FROM drafts")?,
            turns_completed: count(
                "SELECT COUNT(*) FROM events WHERE json_extract(json, '$.body.type') = 'turn-ended' AND json_extract(json, '$.body.status') = 'completed'",
            )?,
            agent_sessions: count(
                "SELECT COUNT(*) FROM events WHERE json_extract(json, '$.body.type') = 'tenure-started'",
            )?,
            messages_you_sent: count(
                "SELECT COUNT(*) FROM events WHERE json_extract(json, '$.body.type') = 'message-queued' AND json_extract(json, '$.actor.kind') = 'human'",
            )?,
        })
    }
}
