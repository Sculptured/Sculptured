//! Trigger addresses: the secret key in each one, by team and trigger.
use super::Store;
use rusqlite::{OptionalExtension, params};

impl Store {
    pub fn trigger_key(&self, team: &str, trigger: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT key FROM trigger_keys WHERE team=?1 AND trigger=?2",
                params![team, trigger],
                |r| r.get(0),
            )
            .optional()
    }

    /// Gives the trigger `key`, replacing its earlier one (which stops working).
    pub fn set_trigger_key(&self, team: &str, trigger: &str, key: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO trigger_keys(key, team, trigger, created_ms) VALUES (?1,?2,?3,?4)
             ON CONFLICT(team, trigger) DO UPDATE SET key=excluded.key, created_ms=excluded.created_ms",
            params![key, team, trigger, model::now_ms()],
        )?;
        Ok(())
    }

    /// The (team, trigger) whose address holds `key`.
    pub fn trigger_of_key(&self, key: &str) -> rusqlite::Result<Option<(String, String)>> {
        self.conn
            .query_row(
                "SELECT team, trigger FROM trigger_keys WHERE key=?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
    }
}
