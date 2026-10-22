//! Interpreted actions (spec §6.4): one row per tool call, its result, and what the interpreter
//! read into it. Feeds risk counts, the files a session changed and captured test runs.
//! A row is keyed by seat, session (tenure) and call id: call ids repeat across sessions
//! (the stub's `turn-1-call`, fake-claude's `toolu_1`). A result and its interpretation belong
//! to the call of the seat's current session.
use model::{ActionClass, EventBody, EventRecord, RiskFlag, Subject};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;

/// The seat's current session, as the seat projection last recorded it (0 before any).
pub(crate) fn tenure_of(tx: &Transaction, seat: &str) -> rusqlite::Result<i64> {
    Ok(tx
        .query_row("SELECT tenure FROM seats WHERE id = ?1", [seat], |r| {
            r.get(0)
        })
        .optional()?
        .unwrap_or(0))
}

pub(crate) fn project(tx: &Transaction, ev: &EventRecord) -> rusqlite::Result<()> {
    let Subject::Seat(seat) = &ev.subject else {
        return Ok(());
    };
    match &ev.body {
        EventBody::ToolCalled {
            turn_id,
            call_id,
            name,
            input,
            ..
        } => record_call(tx, seat, ev, turn_id.as_deref(), call_id, name, input),
        EventBody::ToolResulted {
            call_id, is_error, ..
        } => {
            // Only the first result of a call counts, so a repeated result is never interpreted twice.
            tx.execute(
                "UPDATE actions SET ok = ?4, result_seq = ?5
                 WHERE seat = ?1 AND tenure = ?2 AND call_id = ?3 AND result_seq IS NULL",
                params![seat, tenure_of(tx, seat)?, call_id, !*is_error, ev.seq],
            )?;
            Ok(())
        }
        EventBody::ActionInterpreted {
            call_id,
            class,
            paths,
            risks,
        } => record_reading(tx, seat, call_id, class, paths, risks),
        _ => Ok(()),
    }
}

/// A Practice agent numbers its calls per session, so an id may come again: the newest call wins.
fn record_call(
    tx: &Transaction,
    seat: &str,
    ev: &EventRecord,
    turn_id: Option<&str>,
    call_id: &str,
    name: &str,
    input: &Value,
) -> rusqlite::Result<()> {
    let tenure = tenure_of(tx, seat)?;
    tx.execute(
        "INSERT INTO actions (seat, call_id, rig, tenure, turn_id, called_seq, at_ms, tool, input_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (seat, tenure, call_id) DO UPDATE SET rig = excluded.rig,
             turn_id = excluded.turn_id, called_seq = excluded.called_seq, at_ms = excluded.at_ms,
             tool = excluded.tool, input_json = excluded.input_json, class_json = NULL,
             paths_json = '[]', risks_json = '[]', ok = NULL, result_seq = NULL",
        params![seat, call_id, crate::seat_ids::rig_or_empty(seat), tenure, turn_id, ev.seq, ev.at_ms, name, input.to_string()],
    )?;
    Ok(())
}

/// What the interpreter read into a call; its risks add to the seat's count.
fn record_reading(
    tx: &Transaction,
    seat: &str,
    call_id: &str,
    class: &ActionClass,
    paths: &[String],
    risks: &[RiskFlag],
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE actions SET class_json = ?4, paths_json = ?5, risks_json = ?6
         WHERE seat = ?1 AND tenure = ?2 AND call_id = ?3",
        params![
            seat,
            tenure_of(tx, seat)?,
            call_id,
            serde_json::to_string(class).expect("class serializes"),
            serde_json::to_string(paths).expect("paths serialize"),
            serde_json::to_string(risks).expect("risks serialize")
        ],
    )?;
    if !risks.is_empty() {
        tx.execute(
            "UPDATE seats SET risk_count = risk_count + ?2 WHERE id = ?1",
            params![seat, risks.len() as i64],
        )?;
    }
    Ok(())
}
