use super::{project::append_local_in, rows::json_parse, Store};
use crate::views::StreamItem;
use model::{Actor, DispatchRecord, EventBody, MessageIntent, Provenance, Subject, WorkflowRun};
use rusqlite::{params, OptionalExtension};
use std::collections::BTreeMap;

impl Store {
    pub fn workflow_run(&self, rig: &str, start: i64) -> rusqlite::Result<Option<WorkflowRun>> {
        let json: Option<String> = self.conn.query_row("SELECT json FROM workflow_runs WHERE rig=?1 AND start_seq=?2", params![rig, start], |r| r.get(0)).optional()?;
        json.map(|j| json_parse(&j)).transpose()
    }

    pub fn latest_workflow(&self, rig: &str) -> rusqlite::Result<Option<WorkflowRun>> {
        let json: Option<String> = self.conn.query_row("SELECT json FROM workflow_runs WHERE rig=?1 ORDER BY start_seq DESC LIMIT 1", [rig], |r| r.get(0)).optional()?;
        json.map(|j| json_parse(&j)).transpose()
    }

    /// State and optional outbox message commit together. Never lose an assignment after a restart.
    pub fn save_workflow(&mut self, run: &WorkflowRun, message: Option<(&str, &str, &str)>) -> rusqlite::Result<Vec<StreamItem>> {
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO workflow_runs(rig,start_seq,json) VALUES(?1,?2,?3) ON CONFLICT(rig,start_seq) DO UPDATE SET json=excluded.json",
            params![run.rig, run.start_seq, serde_json::to_string(run).expect("workflow serializes")])?;
        let mut items = Vec::new();
        if let Some((seat, id, text)) = message {
            let body = EventBody::MessageQueued { message_id: id.into(), intent: MessageIntent::Inform, text: text.into() };
            items.push(append_local_in(&tx, Actor::System("workflow".into()), Subject::Seat(seat.into()), Provenance::Operator, body)?);
        }
        tx.commit()?;
        Ok(items)
    }

    /// What came of the dispatch command for this start, once it finished.
    pub fn dispatch_record(&self, rig: &str, start: i64) -> rusqlite::Result<Option<DispatchRecord>> {
        let json: Option<String> = self.conn.query_row("SELECT json FROM workflow_dispatches WHERE rig=?1 AND start_seq=?2", params![rig, start], |r| r.get(0)).optional()?;
        json.map(|j| json_parse(&j)).transpose()
    }

    /// Records the dispatch command's result; with `run`, the batch it made, in the same transaction.
    pub fn save_dispatch(&mut self, rig: &str, start: i64, record: &DispatchRecord, run: Option<&WorkflowRun>) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO workflow_dispatches(rig,start_seq,json) VALUES(?1,?2,?3) ON CONFLICT(rig,start_seq) DO UPDATE SET json=excluded.json",
            params![rig, start, serde_json::to_string(record).expect("record serializes")])?;
        if let Some(run) = run {
            tx.execute("INSERT INTO workflow_runs(rig,start_seq,json) VALUES(?1,?2,?3)", params![run.rig, run.start_seq, serde_json::to_string(run).expect("workflow serializes")])?;
        }
        tx.commit()
    }

    /// Forgets a failed dispatch command's result, so it runs again.
    pub fn clear_dispatch(&mut self, rig: &str, start: i64) -> rusqlite::Result<bool> {
        Ok(self.conn.execute("DELETE FROM workflow_dispatches WHERE rig=?1 AND start_seq=?2", params![rig, start])? > 0)
    }

    /// Tokens of the turns that took these assignments. An assignment's message id is the assignment id,
    /// and usage reports are per-turn amounts. One with no report is absent (never zero).
    /// Only events from `since_seq` on (the run's start) are read.
    pub fn assignment_usage(&self, assignments: &[String], since_seq: i64) -> rusqlite::Result<BTreeMap<String, crate::views::TurnTokens>> {
        let ids = serde_json::to_string(assignments).expect("ids serialize");
        let mut q = self.conn.prepare(
            "SELECT m.id, COALESCE(SUM(json_extract(e.json,'$.body.input_tokens')),0), COALESCE(SUM(json_extract(e.json,'$.body.output_tokens')),0),
                    COALESCE(SUM(json_extract(e.json,'$.body.cache_read_tokens')),0), COALESCE(SUM(json_extract(e.json,'$.body.cache_write_tokens')),0),
                    SUM(json_extract(e.json,'$.body.cost.usd')), MAX(json_extract(e.json,'$.body.cost.source')),
                    COUNT(json_extract(e.json,'$.body.cost.usd')) < SUM(COALESCE(json_extract(e.json,'$.body.in_turn'), 0) = 0)
             FROM messages m JOIN events e ON e.subject = 'seat:' || m.seat AND e.seq >= ?2
               AND json_extract(e.json,'$.body.type') = 'usage-reported' AND json_extract(e.json,'$.body.turn_id') = m.turn_id
             WHERE m.id IN (SELECT value FROM json_each(?1)) GROUP BY m.id",
        )?;
        let rows = q.query_map(params![ids, since_seq], |r| {
            let n = |i| r.get::<_, i64>(i).map(|v| v.max(0) as u64);
            let cost_usd: Option<f64> = r.get(5)?;
            let source: Option<String> = r.get(6)?;
            let cost_source = source.and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok());
            Ok((r.get::<_, String>(0)?, crate::views::TurnTokens { input: n(1)?, output: n(2)?, cache_read: n(3)?, cache_write: n(4)?,
                cost_partial: cost_usd.is_some() && r.get::<_, bool>(7)?, cost_usd, cost_source }))
        })?;
        rows.collect()
    }
}
