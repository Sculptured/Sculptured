use super::Store;
use model::EventRecord;
use rusqlite::{OptionalExtension, params};

/// Why an agent is being stopped (`agent_controls.mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlMode {
    /// Asked to wrap up; stopped after its wrap-up turn, about 5,000 more output tokens, or 5 minutes.
    Soft,
    Hard,
    /// A soft stop that has finished.
    Finished,
    /// Stopped because its output budget ran out.
    Budget,
}

impl ControlMode {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Hard => "hard",
            Self::Finished => "finished",
            Self::Budget => "budget",
        }
    }
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Soft, Self::Hard, Self::Finished, Self::Budget]
            .into_iter()
            .find(|m| m.as_str() == s)
    }
}

pub struct SoftStop {
    pub seat: String,
    pub baseline: u64,
    pub requested_ms: i64,
    pub message_id: Option<String>,
}

impl Store {
    /// Holds a status note for an idle agent (see `team_context::with_context`).
    pub fn hold_note(
        &self,
        message_id: &str,
        seat: &str,
        sender: &str,
        text: &str,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO held_notes (message_id, seat, sender, text, at_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![message_id, seat, sender, text, model::now_ms()],
        )?;
        Ok(())
    }

    /// The notes held for `seat`, oldest first, as (message id, sender, text); they are taken (removed).
    pub fn take_held_notes(&self, seat: &str) -> rusqlite::Result<Vec<(String, String, String)>> {
        let mut q = self.conn.prepare_cached(
            "SELECT message_id, sender, text FROM held_notes WHERE seat = ?1 ORDER BY at_ms, rowid",
        )?;
        let notes: Vec<(String, String, String)> = q
            .query_map([seat], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        self.conn
            .execute("DELETE FROM held_notes WHERE seat = ?1", [seat])?;
        Ok(notes)
    }

    pub fn archived(&self, id: &str) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM archived_projects WHERE id=?1)",
            [id],
            |r| r.get(0),
        )
    }
    pub fn set_archived(&self, id: &str, archived: bool) -> rusqlite::Result<()> {
        if archived {
            self.conn.execute(
                "INSERT OR REPLACE INTO archived_projects VALUES (?1,?2)",
                params![id, model::now_ms()],
            )?;
        } else {
            self.conn
                .execute("DELETE FROM archived_projects WHERE id=?1", [id])?;
        }
        Ok(())
    }
    /// Why the agent is being stopped, if it is.
    pub fn control(&self, seat: &str) -> rusqlite::Result<Option<ControlMode>> {
        let mode: Option<String> = self
            .conn
            .query_row(
                "SELECT mode FROM agent_controls WHERE seat=?1",
                [seat],
                |r| r.get(0),
            )
            .optional()?;
        // An unknown word in an old database still means "stopped".
        Ok(mode.map(|m| ControlMode::parse(&m).unwrap_or(ControlMode::Hard)))
    }
    /// Starts a stop of `seat`, remembering its output so far (`baseline`) for a soft stop's budget.
    pub fn record_control(
        &self,
        seat: &str,
        mode: ControlMode,
        baseline: u64,
    ) -> rusqlite::Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO agent_controls VALUES (?1,?2,?3,?4,NULL)",
                params![seat, mode.as_str(), baseline as i64, model::now_ms()],
            )
            .map(|_| ())
    }
    pub fn set_control_mode(&self, seat: &str, mode: ControlMode) -> rusqlite::Result<()> {
        self.conn
            .execute(
                "UPDATE agent_controls SET mode=?2 WHERE seat=?1",
                params![seat, mode.as_str()],
            )
            .map(|_| ())
    }
    /// The wrap-up message a soft stop sent.
    pub fn set_control_message(&self, seat: &str, message_id: &str) -> rusqlite::Result<()> {
        self.conn
            .execute(
                "UPDATE agent_controls SET message_id=?2 WHERE seat=?1",
                params![seat, message_id],
            )
            .map(|_| ())
    }
    pub fn clear_control(&self, seat: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM agent_controls WHERE seat=?1", [seat])
            .map(|_| ())
    }
    pub fn clear_controls_of(&self, rig: &str) -> rusqlite::Result<()> {
        self.conn
            .execute(
                "DELETE FROM agent_controls WHERE seat IN (SELECT id FROM seats WHERE rig=?1)",
                [rig],
            )
            .map(|_| ())
    }
    /// Soft stops in progress.
    pub fn soft_stops(&self) -> rusqlite::Result<Vec<SoftStop>> {
        self.conn.prepare("SELECT seat,baseline,requested_ms,message_id FROM agent_controls WHERE mode='soft'")?
            .query_map([], |r| Ok(SoftStop { seat: r.get(0)?, baseline: r.get::<_, i64>(1)? as u64, requested_ms: r.get(2)?, message_id: r.get(3)? }))?
            .collect()
    }
    /// The saved answer to an agent's tool request (its arguments, then its result), if it ran before.
    pub fn tool_receipt(&self, key: &str) -> rusqlite::Result<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT payload,result FROM agent_tool_receipts WHERE id=?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }
    pub fn save_tool_receipt(
        &self,
        key: &str,
        payload: &str,
        result: &str,
    ) -> rusqlite::Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO agent_tool_receipts VALUES (?1,?2,?3)",
                params![key, payload, result],
            )
            .map(|_| ())
    }
    /// The child a spawn request made, if it ran before.
    pub fn child_for_request(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT child FROM agent_children WHERE request_key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()
    }
    pub fn parent(&self, child: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT parent FROM agent_children WHERE child=?1",
                [child],
                |r| r.get(0),
            )
            .optional()
    }
    pub fn children(&self, parent: &str) -> rusqlite::Result<Vec<String>> {
        self.conn.prepare("SELECT c.child FROM agent_children c JOIN seats s ON s.id=c.child WHERE c.parent=?1 ORDER BY c.created_ms,c.child")?.query_map([parent], |r| r.get(0))?.collect()
    }
    /// The team's latest message, handoff, start and capture-gap events, newest first; with `usage`,
    /// its model-usage reports too. Tool calls and replies are left out: they are most of a run's
    /// events and no communication view reads them.
    pub fn communication_events(
        &self,
        rig: &str,
        limit: usize,
        usage: bool,
        after_seq: i64,
    ) -> rusqlite::Result<Vec<EventRecord>> {
        let types = "'message-queued','message-delivered','message-consumed','message-failed','message-held','handed-over','tenure-started','events-lost'";
        let types = if usage {
            format!("{types},'usage-reported'")
        } else {
            types.to_string()
        };
        let sql = format!(
            "SELECT e.json FROM events e JOIN seats s ON e.subject='seat:'||s.id WHERE s.rig=?1 AND e.seq > ?3 AND json_extract(e.json, '$.body.type') IN ({types}) ORDER BY e.seq DESC LIMIT ?2"
        );
        self.conn
            .prepare(&sql)?
            .query_map(params![rig, limit as i64, after_seq], |r| {
                r.get::<_, String>(0)
            })?
            .map(|r| r.and_then(|s| crate::store::rows::json_parse(&s)))
            .collect()
    }
}
