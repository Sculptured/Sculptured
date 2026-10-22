use host_proto::HostEvent;
use model::{Actor, EventBody, Provenance, Subject};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS events (host_seq INTEGER PRIMARY KEY, json TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS seats (
    seat TEXT PRIMARY KEY,
    driver TEXT NOT NULL,
    native_session_id TEXT,
    tenure INTEGER NOT NULL
);
INSERT OR IGNORE INTO meta (k, v) VALUES ('next_seq', 1);
CREATE TABLE IF NOT EXISTS processes (seat TEXT PRIMARY KEY, pid INTEGER NOT NULL, ident TEXT);
";

/// Durable, bounded buffer of events not yet acknowledged by controld, plus the
/// per-seat record (driver, native session id, tenure) needed for later restore.
pub struct Journal {
    conn: Connection,
    max_unacked: u64,
}

impl Journal {
    pub fn open(path: &Path, max_unacked: u64) -> rusqlite::Result<Journal> {
        let conn = Connection::open(path)?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Journal {
            conn,
            max_unacked: max_unacked.clamp(1, i64::MAX as u64),
        })
    }

    pub fn open_in_memory(max_unacked: u64) -> rusqlite::Result<Journal> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Journal {
            conn,
            max_unacked: max_unacked.clamp(1, i64::MAX as u64),
        })
    }

    /// Assigns the next host_seq (never reused, even after acks or restarts),
    /// stores the event, and drops the oldest entries beyond `max_unacked`.
    /// controld sees the resulting gap and records EventsLost.
    pub fn append(
        &mut self,
        at_ms: i64,
        actor: Actor,
        subject: Subject,
        provenance: Provenance,
        body: EventBody,
    ) -> rusqlite::Result<HostEvent> {
        let tx = self.conn.transaction()?;
        let seq: i64 = tx.query_row("SELECT v FROM meta WHERE k = 'next_seq'", [], |r| r.get(0))?;
        let ev = HostEvent {
            host_seq: seq as u64,
            at_ms,
            actor,
            subject,
            provenance,
            body,
        };
        let json = serde_json::to_string(&ev).expect("HostEvent always serializes");
        tx.execute(
            "INSERT INTO events (host_seq, json) VALUES (?1, ?2)",
            params![seq, json],
        )?;
        tx.execute("UPDATE meta SET v = v + 1 WHERE k = 'next_seq'", [])?;
        tx.execute(
            "DELETE FROM events WHERE host_seq <= ?1",
            params![seq - self.max_unacked as i64],
        )?;
        tx.commit()?;
        Ok(ev)
    }

    pub fn ack(&mut self, upto: u64) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM events WHERE host_seq <= ?1",
            params![upto as i64],
        )?;
        Ok(())
    }

    pub fn unacked_after(&self, after: u64) -> rusqlite::Result<Vec<HostEvent>> {
        let mut stmt = self
            .conn
            .prepare("SELECT json FROM events WHERE host_seq > ?1 ORDER BY host_seq")?;
        let rows = stmt.query_map(params![after as i64], |r| r.get::<_, String>(0))?;
        rows.map(|row| {
            let json = row?;
            serde_json::from_str(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .collect()
    }

    pub fn last_seq(&self) -> u64 {
        self.conn
            .query_row("SELECT v FROM meta WHERE k = 'next_seq'", [], |r| {
                r.get::<_, i64>(0)
            })
            .map(|next| (next - 1) as u64)
            .unwrap_or(0)
    }

    /// Moves `next_seq` forward to `next` (never backward). Used when controld's cursor
    /// for this host is ahead of the journal, e.g. after the journal file was reset:
    /// controld ignores every host_seq at or below its cursor.
    pub fn fast_forward(&mut self, next: u64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE meta SET v = MAX(v, ?1) WHERE k = 'next_seq'",
            params![next as i64],
        )?;
        Ok(())
    }

    pub fn record_seat(
        &mut self,
        seat: &str,
        driver: &str,
        native: Option<&str>,
        tenure: u32,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO seats (seat, driver, native_session_id, tenure) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(seat) DO UPDATE SET driver = excluded.driver,
                 native_session_id = excluded.native_session_id, tenure = excluded.tenure",
            params![seat, driver, native, tenure],
        )?;
        Ok(())
    }

    /// The agent's process (the leader of the group its driver made) and what identifies it beyond
    /// its pid, so `sculptured stop` can end it without the server, and never a process that reused the pid.
    pub fn record_process(
        &mut self,
        seat: &str,
        pid: u32,
        ident: Option<&str>,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO processes (seat, pid, ident) VALUES (?1, ?2, ?3) ON CONFLICT(seat) DO UPDATE SET pid = excluded.pid, ident = excluded.ident",
            params![seat, pid, ident],
        )?;
        Ok(())
    }

    pub fn clear_process(&mut self, seat: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM processes WHERE seat = ?1", params![seat])?;
        Ok(())
    }

    /// Every recorded agent process: (seat, pid, identity).
    pub fn processes(&self) -> rusqlite::Result<Vec<(String, u32, Option<String>)>> {
        let mut q = self
            .conn
            .prepare("SELECT seat, pid, ident FROM processes ORDER BY seat")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    }

    /// The seat's latest tenure number, or 0 if it has never launched on this host.
    /// A database error is returned, never read as "no tenure yet": that would restart
    /// tenure numbering at 1.
    pub fn seat_tenure(&self, seat: &str) -> rusqlite::Result<u32> {
        Ok(self
            .conn
            .query_row(
                "SELECT tenure FROM seats WHERE seat = ?1",
                params![seat],
                |r| r.get::<_, u32>(0),
            )
            .optional()?
            .unwrap_or(0))
    }
}
