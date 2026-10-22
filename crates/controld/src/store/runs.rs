//! Runs as files: the team as each run started, a run's events for export, and imported runs.
//! Imported runs live in their own tables and never enter `events`, so nothing that drives
//! agents sees them.
use super::Store;
use crate::portable::session::TokenTotals;
use rusqlite::{Connection, OptionalExtension, params};

/// The rows of one team's run: its rig events and its seats' events (`?3` = `rig:<team>`, `?4` = `@<team>`).
const IN_RUN: &str = "seq >= ?1 AND seq < ?2 AND (subject = ?3 OR (substr(subject, 1, 5) = 'seat:' AND substr(subject, -length(?4)) = ?4))";

/// Keeps the team as it is when a run starts (with folders), in the event's own transaction. A rig
/// without a team design (a spec file), or a draft this build can't read, keeps nothing.
pub(super) fn snapshot_team(conn: &Connection, rig: &str, start_seq: i64) -> rusqlite::Result<()> {
    let Some(json) = conn
        .query_row("SELECT json FROM drafts WHERE id = ?1", [rig], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
    else {
        return Ok(());
    };
    let Ok(draft) = serde_json::from_str::<model::TeamDraft>(&json) else {
        return Ok(());
    };
    let file = crate::portable::team::portable(
        draft,
        &crate::settings::team_settings_in(conn, rig)?,
        true,
    );
    conn.execute(
        "INSERT OR REPLACE INTO deployment_drafts (rig, start_seq, json) VALUES (?1, ?2, ?3)",
        params![
            rig,
            start_seq,
            serde_json::to_string(&file).expect("team file serializes")
        ],
    )?;
    Ok(())
}

fn keys(team: &str) -> (String, String) {
    (format!("rig:{team}"), format!("@{team}"))
}

impl Store {
    /// The team (a [`crate::portable::team::PortableTeam`], JSON) as `team`'s run at `start_seq` started, if kept.
    pub fn run_team_snapshot(
        &self,
        team: &str,
        start_seq: i64,
    ) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT json FROM deployment_drafts WHERE rig = ?1 AND start_seq = ?2",
                params![team, start_seq],
                |r| r.get(0),
            )
            .optional()
    }

    /// Where `team`'s latest run starts (its last `rig-booted`), if it ever ran.
    pub fn latest_run_start(&self, team: &str) -> rusqlite::Result<Option<i64>> {
        self.conn.query_row(
            "SELECT MAX(seq) FROM events WHERE subject = ?1 AND json_extract(json, '$.body.type') = 'rig-booted'",
            [format!("rig:{team}")],
            |r| r.get(0),
        )
    }

    /// When the event `seq` happened, if it exists.
    pub fn event_at_ms(&self, seq: i64) -> rusqlite::Result<Option<i64>> {
        self.conn
            .query_row("SELECT at_ms FROM events WHERE seq = ?1", [seq], |r| {
                r.get(0)
            })
            .optional()
    }

    /// Token totals of a run's usage reports, summed by SQLite (no event is parsed here).
    pub fn run_tokens(&self, team: &str, from: i64, end: i64) -> rusqlite::Result<TokenTotals> {
        let (rig, suffix) = keys(team);
        let sql = format!(
            "SELECT COALESCE(SUM(json_extract(json, '$.body.input_tokens')), 0), COALESCE(SUM(json_extract(json, '$.body.output_tokens')), 0),
                    COALESCE(SUM(json_extract(json, '$.body.cache_read_tokens')), 0), COALESCE(SUM(json_extract(json, '$.body.cache_write_tokens')), 0)
             FROM events WHERE {IN_RUN} AND json_extract(json, '$.body.type') = 'usage-reported'"
        );
        self.conn
            .query_row(&sql, params![from, end, rig, suffix], |r| {
                Ok(TokenTotals {
                    input: r.get::<_, i64>(0)? as u64,
                    output: r.get::<_, i64>(1)? as u64,
                    cache_read: r.get::<_, i64>(2)? as u64,
                    cache_write: r.get::<_, i64>(3)? as u64,
                })
            })
    }

    /// Up to `limit` of a run's events from `from` on, as `(seq, stored JSON)`, in order.
    pub fn run_events(
        &self,
        team: &str,
        from: i64,
        end: i64,
        limit: i64,
    ) -> rusqlite::Result<Vec<(i64, String)>> {
        let (rig, suffix) = keys(team);
        let mut q = self.conn.prepare_cached(&format!(
            "SELECT seq, json FROM events WHERE {IN_RUN} ORDER BY seq LIMIT ?5"
        ))?;

        q.query_map(params![from, end, rig, suffix, limit], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect()
    }

    /// Saves an imported run (header JSON and `(seq, subject key, line)` per event) in one
    /// transaction, apart from the live `events` log. Returns its id.
    pub fn insert_imported_run(
        &mut self,
        rig: &str,
        header: &str,
        lines: &[(i64, String, &str)],
    ) -> rusqlite::Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO imported_runs (rig, header, created_ms) VALUES (?1, ?2, ?3)",
            params![rig, header, model::now_ms()],
        )?;
        let run = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare_cached("INSERT INTO imported_events (run, n, seq, subject, line) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            let indexed_lines = lines.iter().enumerate();
            for (n, (seq, subject, line)) in indexed_lines {
                insert.execute(params![run, n as i64, seq, subject, line])?;
            }
        }
        tx.commit()?;
        Ok(run)
    }

    /// An imported run's stored header, if it belongs to `rig`.
    pub fn imported_run_header(&self, rig: &str, id: i64) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT header FROM imported_runs WHERE id = ?1 AND rig = ?2",
                params![id, rig],
                |r| r.get(0),
            )
            .optional()
    }

    /// Every imported run of `rig`, as `(id, header JSON)`.
    pub fn imported_run_headers(&self, rig: &str) -> rusqlite::Result<Vec<(i64, String)>> {
        let mut q = self
            .conn
            .prepare_cached("SELECT id, header FROM imported_runs WHERE rig = ?1")?;

        q.query_map([rig], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect()
    }

    /// Every stored line of an imported run, in file order.
    pub fn imported_lines(&self, run: i64) -> rusqlite::Result<Vec<String>> {
        let mut q = self
            .conn
            .prepare_cached("SELECT line FROM imported_events WHERE run = ?1 ORDER BY n")?;

        q.query_map([run], |r| r.get(0))?.collect()
    }

    /// Up to `limit` stored lines of an imported run before `before` (by seq), newest first;
    /// only `seat`'s when given.
    pub fn imported_lines_page(
        &self,
        run: i64,
        before: i64,
        seat: Option<&str>,
        limit: i64,
    ) -> rusqlite::Result<Vec<String>> {
        let mut q = self.conn.prepare_cached(
            "SELECT line FROM imported_events WHERE run = ?1 AND seq < ?2 AND (?3 IS NULL OR subject = 'seat:' || ?3) ORDER BY seq DESC LIMIT ?4",
        )?;

        q.query_map(params![run, before, seat, limit], |r| r.get(0))?
            .collect()
    }
}
