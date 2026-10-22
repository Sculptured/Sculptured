use super::Store;
use rusqlite::params;
use serde::Serialize;
use ts_rs::TS;

/// One precedent in a team's decision log. Written by people (`source`: "person", or "decision" when it
/// came from a decision card); agents only read it. Hiding keeps the entry and records when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct DecisionEntry {
    pub id: String,
    #[ts(type = "number")]
    pub at_ms: i64,
    pub author: String,
    pub text: String,
    pub source: String,
    #[ts(type = "number | null")]
    pub hidden_ms: Option<i64>,
    /// When the person confirmed an agent's lesson: it is then treated as established.
    #[ts(type = "number | null")]
    pub confirmed_ms: Option<i64>,
}

impl Store {
    pub fn add_decision(
        &self,
        team: &str,
        author: &str,
        text: &str,
        source: &str,
    ) -> rusqlite::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute("INSERT INTO team_decisions(team, id, at_ms, author, text, source) VALUES (?1,?2,?3,?4,?5,?6)", params![team, id, model::now_ms(), author, text, source])?;
        Ok(id)
    }

    /// Newest first; `query` matches the text without regard to case.
    pub fn decisions(
        &self,
        team: &str,
        query: Option<&str>,
        limit: usize,
        include_hidden: bool,
    ) -> rusqlite::Result<Vec<DecisionEntry>> {
        let like = query.map(|q| {
            format!(
                "%{}%",
                q.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            )
        });
        self.conn.prepare("SELECT id, at_ms, author, text, source, hidden_ms, confirmed_ms FROM team_decisions WHERE team=?1 AND (?2 OR hidden_ms IS NULL)
                           AND (?3 IS NULL OR text LIKE ?3 ESCAPE '\\') ORDER BY at_ms DESC, rowid DESC LIMIT ?4")?
            .query_map(params![team, include_hidden, like, limit as i64], |r| Ok(DecisionEntry { id: r.get(0)?, at_ms: r.get(1)?, author: r.get(2)?, text: r.get(3)?, source: r.get(4)?, hidden_ms: r.get(5)?, confirmed_ms: r.get(6)? }))?
            .collect()
    }

    /// Hides an entry from agents and the list; false when there is no such visible entry.
    pub fn hide_decision(&self, team: &str, id: &str) -> rusqlite::Result<bool> {
        Ok(self.conn.execute(
            "UPDATE team_decisions SET hidden_ms=?3 WHERE team=?1 AND id=?2 AND hidden_ms IS NULL",
            params![team, id, model::now_ms()],
        )? == 1)
    }

    /// Marks an entry confirmed by the person; false when there is no such visible entry.
    pub fn confirm_decision(&self, team: &str, id: &str) -> rusqlite::Result<bool> {
        Ok(self.conn.execute("UPDATE team_decisions SET confirmed_ms=?3 WHERE team=?1 AND id=?2 AND hidden_ms IS NULL", params![team, id, model::now_ms()])? == 1)
    }

    pub fn decision_count(&self, team: &str) -> rusqlite::Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM team_decisions WHERE team=?1 AND hidden_ms IS NULL",
                [team],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as usize)
    }
}
