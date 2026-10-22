//! Row and JSON conversions shared by the store and the projections.
use crate::views::SeatView;
use model::EventRecord;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub(super) const SEAT_COLS: &str = "id, rig, pod, member, runtime, label, host, lifecycle, lifecycle_reason, activity, \
    tenure, native_session_id, caps_json, current_turn, current_tool, last_event_ms, tokens_in, tokens_out, \
    lifecycle_provenance, waiting_for, handoff_stuck, open_questions, risk_count, restore_offer, cwd, limit_hit, \
    model_requested, model_observed, stalled, last_check, \
    EXISTS (SELECT 1 FROM budget_queue q WHERE q.seat = seats.id)";

pub(crate) fn enum_text<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .expect("enum serializes")
        .as_str()
        .expect("unit variant")
        .to_string()
}

pub(crate) fn conv_err(e: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
}

pub(crate) fn enum_parse<T: DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(conv_err)
}

pub(crate) fn json_parse<T: DeserializeOwned>(s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(conv_err)
}

pub(super) fn seat_from_row(r: &Row) -> rusqlite::Result<SeatView> {
    Ok(SeatView {
        id: r.get(0)?,
        rig: r.get(1)?,
        pod: r.get(2)?,
        member: r.get(3)?,
        runtime: enum_parse(&r.get::<_, String>(4)?)?,
        label: r.get(5)?,
        host: r.get(6)?,
        lifecycle: enum_parse(&r.get::<_, String>(7)?)?,
        lifecycle_reason: r.get(8)?,
        activity: enum_parse(&r.get::<_, String>(9)?)?,
        tenure: r.get::<_, i64>(10)? as u32,
        native_session_id: r.get(11)?,
        caps: r
            .get::<_, Option<String>>(12)?
            .map(|s| json_parse(&s))
            .transpose()?,
        current_turn: r.get(13)?,
        current_tool: r.get(14)?,
        last_event_ms: r.get(15)?,
        tokens_in: r.get::<_, Option<i64>>(16)?.map(|n| n as u64),
        tokens_out: r.get::<_, Option<i64>>(17)?.map(|n| n as u64),
        lifecycle_provenance: r
            .get::<_, Option<String>>(18)?
            .map(|s| enum_parse(&s))
            .transpose()?,
        waiting_for: r
            .get::<_, Option<String>>(19)?
            .map(|s| json_parse(&s))
            .transpose()?,
        handoff_stuck: r
            .get::<_, Option<String>>(20)?
            .map(|s| json_parse(&s))
            .transpose()?,
        open_questions: r.get::<_, i64>(21)? as u32,
        risk_count: r.get::<_, i64>(22)? as u32,
        restore_offer: r
            .get::<_, Option<String>>(23)?
            .map(|s| json_parse(&s))
            .transpose()?,
        cwd: r.get(24)?,
        limit_hit: r
            .get::<_, Option<String>>(25)?
            .map(|s| json_parse(&s))
            .transpose()?,
        model_requested: r.get(26)?,
        model_observed: r.get(27)?,
        stalled: r
            .get::<_, Option<String>>(28)?
            .map(|s| json_parse(&s))
            .transpose()?,
        check: r
            .get::<_, Option<String>>(29)?
            .map(|s| json_parse(&s))
            .transpose()?,
        slot_wait: r.get(30)?,
    })
}

pub(super) fn seat_in(conn: &Connection, id: &str) -> rusqlite::Result<Option<SeatView>> {
    conn.query_row(
        &format!("SELECT {SEAT_COLS} FROM seats WHERE id = ?1"),
        [id],
        seat_from_row,
    )
    .optional()
}

pub(super) fn cursor_in(conn: &Connection, host: &str) -> rusqlite::Result<u64> {
    Ok(conn
        .query_row(
            "SELECT last_seq FROM host_cursor WHERE host = ?1",
            [host],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0) as u64)
}

/// `'a', 'b'` for an SQL `IN (…)` list. The types are this crate's own constants; quotes are stripped all the same.
pub(super) fn type_list(types: &[&str]) -> String {
    types
        .iter()
        .map(|t| format!("'{}'", t.replace('\'', "")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The events an SQL query selects (its first column is the event's JSON), in its order.
pub(super) fn query_events(
    conn: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> rusqlite::Result<Vec<EventRecord>> {
    let mut st = conn.prepare(sql)?;
    st.query_map(args, |r| r.get::<_, String>(0))?
        .map(|j| j.and_then(|j| json_parse::<EventRecord>(&j)))
        .collect()
}

/// Events about `subject` after `after_seq` whose body type is one of `types`, oldest first.
pub(super) fn events_of_types_in(
    conn: &Connection,
    subject: &model::Subject,
    after_seq: i64,
    types: &[&str],
) -> rusqlite::Result<Vec<EventRecord>> {
    let sql = format!(
        "SELECT json FROM events INDEXED BY events_by_subject_type WHERE subject = ?1 AND seq > ?2 AND json_extract(json, '$.body.type') IN ({}) ORDER BY seq",
        type_list(types)
    );
    query_events(conn, &sql, params![subject.key(), after_seq])
}

/// One text column of the message `id`; None for an unknown message.
pub(super) fn message_column<T: rusqlite::types::FromSql>(
    conn: &Connection,
    column: &str,
    id: &str,
) -> rusqlite::Result<Option<T>> {
    conn.query_row(
        &format!("SELECT {column} FROM messages WHERE id = ?1"),
        [id],
        |r| r.get(0),
    )
    .optional()
}
