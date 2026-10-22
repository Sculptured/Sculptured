use super::{
    Actor, Connection, EventBody, EventRecord, HostEvent, MessageState, OptionalExtension, Path,
    Provenance, Receipt, SeatLifecycle, SeatView, StreamItem, Subject, params, schema,
};

/// Host name under which controld records its own events (operator actions, gaps).
pub const LOCAL_EVENT_HOST: &str = "controld";

pub struct Store {
    pub(super) conn: Connection,
}

impl Store {
    /// The store's connection, for tests and the M2 modules that read their own tables.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The folder a seat was launched in (absolute), for the interpreter and the agent pane.
    pub fn set_seat_cwd(&mut self, seat: &str, cwd: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE seats SET cwd = ?2 WHERE id = ?1",
            params![seat, cwd],
        )?;
        Ok(())
    }

    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    pub(super) fn init(mut conn: Connection) -> rusqlite::Result<Self> {
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn seat(&self, id: &str) -> rusqlite::Result<Option<SeatView>> {
        crate::store::seat_in(&self.conn, id)
    }

    pub fn seats_on_host(&self, host: &str) -> rusqlite::Result<Vec<SeatView>> {
        self.seats_where("host = ?1 ORDER BY rig, ord", host)
    }

    pub fn host_cursor(&self, host: &str) -> rusqlite::Result<u64> {
        crate::store::cursor_in(&self.conn, host)
    }

    /// Idempotent per (host, host_seq). A gap after the host's cursor is recorded
    /// first as `EventsLost`, so lost history is never silent.
    pub fn ingest(&mut self, host: &str, ev: HostEvent) -> rusqlite::Result<Vec<StreamItem>> {
        if host == LOCAL_EVENT_HOST {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "host id `{LOCAL_EVENT_HOST}` is reserved"
            )));
        }
        let tx = self.conn.transaction()?;
        let cursor = crate::store::cursor_in(&tx, host)?;
        if ev.host_seq <= cursor {
            return Ok(Vec::new());
        }
        let mut recs = Vec::new();
        if ev.host_seq > cursor + 1 {
            let body = EventBody::EventsLost {
                count: ev.host_seq - cursor - 1,
            };
            let actor = Actor::System("controld".into());
            let lost = crate::store::local_event(
                &tx,
                ev.at_ms,
                actor,
                Subject::Host(host.to_string()),
                Provenance::ProtocolObserved,
                body,
            )?;
            recs.push(crate::store::insert_event(&tx, LOCAL_EVENT_HOST, lost)?);
        }
        recs.push(crate::store::insert_event(&tx, host, ev)?);
        let last = recs.last().expect("just pushed").clone();
        if let Some(body) = crate::interpret::derive(&tx, &last)? {
            let actor = Actor::System("interpreter".into());
            let derived = crate::store::local_event(
                &tx,
                last.at_ms,
                actor,
                last.subject.clone(),
                Provenance::Heuristic,
                body,
            )?;
            recs.push(crate::store::insert_event(&tx, LOCAL_EVENT_HOST, derived)?);
        }
        let items = recs
            .into_iter()
            .map(|e| crate::store::item_for(&tx, e))
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(items)
    }

    pub fn append_local(
        &mut self,
        actor: Actor,
        subject: Subject,
        provenance: Provenance,
        body: EventBody,
    ) -> rusqlite::Result<StreamItem> {
        let tx = self.conn.transaction()?;
        let item = crate::store::append_local_in(&tx, actor, subject, provenance, body)?;
        tx.commit()?;
        Ok(item)
    }

    /// Events about `subject` after `after_seq` whose body type is one of `types`
    /// (kebab-case, e.g. `turn-ended`), oldest first, without a limit.
    pub fn events_of_types(
        &self,
        subject: &Subject,
        after_seq: i64,
        types: &[&str],
    ) -> rusqlite::Result<Vec<EventRecord>> {
        crate::store::events_of_types_in(&self.conn, subject, after_seq, types)
    }

    /// The newest `limit` events of `types` about `subject`, newest first.
    pub fn latest_of_types(
        &self,
        subject: &Subject,
        types: &[&str],
        limit: usize,
    ) -> rusqlite::Result<Vec<EventRecord>> {
        let sql = format!(
            "SELECT json FROM events WHERE subject = ?1 AND json_extract(json, '$.body.type') IN ({}) ORDER BY seq DESC LIMIT ?2",
            crate::store::type_list(types)
        );
        crate::store::query_events(&self.conn, &sql, params![subject.key(), limit as i64])
    }

    pub fn events_for_subject(
        &self,
        subject: &Subject,
        after_seq: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<EventRecord>> {
        let sql = "SELECT json FROM events WHERE subject = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3";
        crate::store::query_events(
            &self.conn,
            sql,
            params![subject.key(), after_seq, limit as i64],
        )
    }

    pub fn events_for_seat(
        &self,
        seat: &str,
        after_seq: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<EventRecord>> {
        self.events_for_subject(&Subject::Seat(seat.to_string()), after_seq, limit)
    }

    /// The seat's newest `limit` events, returned oldest first.
    pub fn latest_events_for_seat(
        &self,
        seat: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<EventRecord>> {
        let sql = "SELECT json FROM events WHERE subject = ?1 ORDER BY seq DESC LIMIT ?2";
        let mut v = crate::store::query_events(
            &self.conn,
            sql,
            params![Subject::Seat(seat.to_string()).key(), limit as i64],
        )?;
        v.reverse();
        Ok(v)
    }

    /// The highest event seq stored so far (0 when empty).
    pub fn last_seq(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))
    }

    pub fn message_state(&self, id: &str) -> rusqlite::Result<Option<MessageState>> {
        crate::store::message_column::<String>(&self.conn, "state", id)?
            .map(|s| crate::store::enum_parse(&s))
            .transpose()
    }

    /// A message's text; None for an unknown message.
    pub fn message_text(&self, id: &str) -> rusqlite::Result<Option<String>> {
        crate::store::message_column(&self.conn, "text", id)
    }

    /// Why a failed message failed; None while it has not failed.
    pub fn message_reason(&self, id: &str) -> rusqlite::Result<Option<String>> {
        Ok(crate::store::message_column::<Option<String>>(&self.conn, "reason", id)?.flatten())
    }

    pub fn receipt(&self, request_id: &str) -> rusqlite::Result<Option<Receipt>> {
        self.conn
            .query_row(
                "SELECT fingerprint, status, body FROM receipts WHERE request_id = ?1",
                [request_id],
                |r| {
                    Ok(Receipt {
                        fingerprint: r.get(0)?,
                        status: r.get::<_, i64>(1)? as u16,
                        body: r.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn save_receipt(&mut self, request_id: &str, r: &Receipt) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO receipts (request_id, fingerprint, status, body) VALUES (?1, ?2, ?3, ?4)",
            params![request_id, r.fingerprint, r.status as i64, r.body],
        )?;
        Ok(())
    }
}

/// Lifecycles that mean "a process may be alive for this seat".
#[must_use]
pub fn is_live(l: SeatLifecycle) -> bool {
    matches!(
        l,
        SeatLifecycle::Running
            | SeatLifecycle::Launching
            | SeatLifecycle::Stopping
            | SeatLifecycle::Stale
    )
}
