//! Handoff records: what was handed to a seat in a start, recorded with its message in one transaction.
use super::project::append_local_in;
use super::rows::{events_of_types_in, json_parse, message_column};
use super::Store;
use crate::views::StreamItem;
use model::{Actor, EventBody, EventRecord, Provenance, Subject};
use rusqlite::{Connection, OptionalExtension, Transaction};
use std::collections::HashSet;

/// A `HandedOver` record with its event seq.
#[derive(Clone, Debug)]
pub struct HandedOverRecord {
    pub seq: i64,
    pub message_id: String,
    pub from: Vec<model::HandoffSource>,
    pub resend_of: Option<String>,
    pub wave: Option<model::Wave>,
}

/// Reads the seat's events after `after_seq` only (the start the messages belong to), so
/// the cost does not grow with earlier starts.
pub(super) fn reached_in(conn: &Connection, seat: &str, after_seq: i64, ids: &[String]) -> rusqlite::Result<bool> {
    let by_events = events_of_types_in(conn, &Subject::Seat(seat.to_string()), after_seq, &["message-delivered", "message-consumed"])?
        .iter()
        .any(|e| match &e.body {
            EventBody::MessageDelivered { message_id } | EventBody::MessageConsumed { message_id, .. } => ids.contains(message_id),
            _ => false,
        });
    if by_events {
        return Ok(true);
    }
    for id in ids {
        let state: Option<String> = message_column(conn, "state", id)?;
        if matches!(state.as_deref(), Some("delivered" | "consumed")) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `HandedOver` records on `seat` that belong to the start at `start_seq`. Records are
/// matched by their own `start_seq`, never by position: a previous start's records can
/// be written after a newer start began (its "not sent" finalisation). Every record of a
/// start is written after its `RigBooted`, so only events after `start_seq` are read.
pub(super) fn handed_over_in(conn: &Connection, seat: &str, start_seq: i64) -> rusqlite::Result<Vec<HandedOverRecord>> {
    Ok(events_of_types_in(conn, &Subject::Seat(seat.to_string()), start_seq, &["handed-over"])?
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::HandedOver { start_seq: s, message_id, from, resend_of, wave } if s == start_seq => {
                Some(HandedOverRecord { seq: e.seq, message_id, from, resend_of, wave })
            }
            _ => None,
        })
        .collect())
}

/// One handoff message for `Store::record_handoff`.
#[derive(Clone, Debug)]
pub struct HandoffWrite {
    pub seat: String,
    /// Seq of the `RigBooted` that began this start.
    pub start_seq: i64,
    /// True for the seat's first handoff in this start.
    pub first: bool,
    pub actor: Actor,
    pub message_id: String,
    pub intent: model::MessageIntent,
    pub text: String,
    pub from: Vec<model::HandoffSource>,
    /// The "More work" wave the message belongs to, if any.
    pub wave: Option<model::Wave>,
    /// Some(reason) records the message as failed (not sent) straight away.
    pub failed: Option<String>,
}

/// What a re-sent handoff says: the original text, the original after a prefix, or only the prefix
/// (a resumed session already holds the original in its history).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResendText<'a> {
    Same,
    Prefixed(&'a str),
    Only(&'a str),
}

/// The message's `MessageQueued` and its `HandedOver` record, as the operator's records.
fn append_handoff(tx: &Transaction, actor: Actor, seat: &str, queued: EventBody, handed: EventBody) -> rusqlite::Result<Vec<StreamItem>> {
    let subject = Subject::Seat(seat.to_string());
    Ok(vec![
        append_local_in(tx, actor.clone(), subject.clone(), Provenance::Operator, queued)?,
        append_local_in(tx, actor, subject, Provenance::Operator, handed)?,
    ])
}

/// The ids of handoff `original` and its resends.
fn copies_of(records: &[HandedOverRecord], original: &str) -> Vec<String> {
    records
        .iter()
        .filter(|r| r.message_id == original || r.resend_of.as_deref() == Some(original))
        .map(|r| r.message_id.clone())
        .collect()
}

fn resend_text(conn: &Connection, original: &str, text: ResendText<'_>) -> rusqlite::Result<String> {
    let original_text: String = conn.query_row("SELECT text FROM messages WHERE id = ?1", [original], |r| r.get(0))?;
    Ok(match text {
        ResendText::Same => original_text,
        ResendText::Prefixed(p) => format!("{p}\n\n{original_text}"),
        ResendText::Only(p) => p.to_string(),
    })
}

/// Who recorded the event at `seq`.
fn actor_of(conn: &Connection, seq: i64) -> rusqlite::Result<Actor> {
    let json: String = conn.query_row("SELECT json FROM events WHERE seq = ?1", [seq], |r| r.get(0))?;
    Ok(json_parse::<EventRecord>(&json)?.actor)
}

impl Store {
    /// Records one handoff message atomically: the message, the `handed-over` record of
    /// the upstream turns it carries and, for a message that cannot be sent, its failure.
    /// Nothing is recorded (None) when any of those turns was already handed to this seat
    /// in the start that began at `h.start_seq` (records of other starts never count), or
    /// when `h.first` disagrees with whether a first handoff was already recorded:
    /// replays, resyncs and retries never hand twice.
    pub fn record_handoff(&mut self, h: &HandoffWrite) -> rusqlite::Result<Option<Vec<StreamItem>>> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let earlier = handed_over_in(&tx, &h.seat, h.start_seq)?;
        let first_already = earlier.iter().any(|r| r.resend_of.is_none());
        let covered: Vec<i64> = earlier.iter().flat_map(|r| r.from.iter().map(|f| f.key())).collect();
        if h.first == first_already || h.from.iter().any(|f| covered.contains(&f.key())) {
            return Ok(None);
        }
        let queued = EventBody::MessageQueued { message_id: h.message_id.clone(), intent: h.intent, text: h.text.clone() };
        let handed = EventBody::HandedOver {
            start_seq: h.start_seq,
            message_id: h.message_id.clone(),
            from: h.from.clone(),
            resend_of: None,
            wave: h.wave.clone(),
        };
        let mut items = append_handoff(&tx, h.actor.clone(), &h.seat, queued, handed)?;
        if let Some(reason) = &h.failed {
            // "Not sent" is controld's inference from the seat's lifecycle or the team's
            // stop, so it is Heuristic, like the Job's "not sent".
            let failed = EventBody::MessageFailed { message_id: h.message_id.clone(), reason: reason.clone() };
            items.push(append_local_in(&tx, Actor::System("controld".into()), Subject::Seat(h.seat.clone()), Provenance::Heuristic, failed)?);
        }
        tx.commit()?;
        Ok(Some(items))
    }

    /// Records `message_id`, a message another agent sent `seat` itself, as the handoff of `from`
    /// (nothing is sent). Refused (None) like `record_handoff` when that work was handed already.
    pub fn record_adopted(
        &mut self,
        seat: &str,
        start_seq: i64,
        first: bool,
        message_id: &str,
        from: Vec<model::HandoffSource>,
        wave: Option<model::Wave>,
    ) -> rusqlite::Result<Option<Vec<StreamItem>>> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let earlier = handed_over_in(&tx, seat, start_seq)?;
        let first_already = earlier.iter().any(|r| r.resend_of.is_none());
        let covered: Vec<i64> = earlier.iter().flat_map(|r| r.from.iter().map(|f| f.key())).collect();
        if first == first_already || from.iter().any(|f| covered.contains(&f.key())) || earlier.iter().any(|r| r.message_id == message_id) {
            return Ok(None);
        }
        let handed = EventBody::HandedOver { start_seq, message_id: message_id.to_string(), from, resend_of: None, wave };
        let actor = Actor::System(crate::handoffs::HANDOFF_ACTOR.into());
        let item = append_local_in(&tx, actor, Subject::Seat(seat.to_string()), Provenance::Heuristic, handed)?;
        tx.commit()?;
        Ok(Some(vec![item]))
    }

    /// The handoff records of `seat` in the start that began at `start_seq`, oldest first.
    pub fn handed_over(&self, seat: &str, start_seq: i64) -> rusqlite::Result<Vec<HandedOverRecord>> {
        handed_over_in(&self.conn, seat, start_seq)
    }

    /// Re-sends handoff `original` (a message recorded for `seat`) as a new message with
    /// the same sources and wave, and the text `text` chooses. Unless `continuing` (the agent
    /// read it but its work did not finish), it is refused (None) when the agent already
    /// has any copy (delivered or read, by the host's own events, whatever the message's
    /// state says), and always when a copy in `in_flight` is still being sent.
    #[allow(clippy::too_many_arguments)]
    pub fn record_resend(
        &mut self,
        seat: &str,
        start_seq: i64,
        original: &str,
        new_id: &str,
        text: ResendText<'_>,
        continuing: bool,
        in_flight: &HashSet<String>,
    ) -> rusqlite::Result<Option<(Vec<StreamItem>, String)>> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let records = handed_over_in(&tx, seat, start_seq)?;
        let Some(orig) = records.iter().find(|r| r.message_id == original && r.resend_of.is_none()) else { return Ok(None) };
        let copies = copies_of(&records, original);
        if copies.iter().any(|c| in_flight.contains(c)) || (!continuing && reached_in(&tx, seat, start_seq, &copies)?) {
            return Ok(None);
        }
        let text = resend_text(&tx, original, text)?;
        let queued = EventBody::MessageQueued { message_id: new_id.to_string(), intent: model::MessageIntent::Inform, text: text.clone() };
        let handed = EventBody::HandedOver {
            start_seq,
            message_id: new_id.to_string(),
            from: orig.from.clone(),
            resend_of: Some(original.to_string()),
            wave: orig.wave.clone(),
        };
        let items = append_handoff(&tx, actor_of(&tx, orig.seq)?, seat, queued, handed)?;
        tx.commit()?;
        Ok(Some((items, text)))
    }

    /// True when the host reported any of `ids` delivered to or read by `seat` after
    /// `after_seq` (the start they belong to), or the message state says so. The host's
    /// events decide even when the message was recorded as failed (a send that timed out
    /// here but reached the agent).
    pub fn reached(&self, seat: &str, after_seq: i64, ids: &[String]) -> rusqlite::Result<bool> {
        reached_in(&self.conn, seat, after_seq, ids)
    }

    /// True when the host reported any of `ids` read by `seat` after `after_seq`.
    pub fn read_by(&self, seat: &str, after_seq: i64, ids: &[String]) -> rusqlite::Result<bool> {
        Ok(events_of_types_in(&self.conn, &Subject::Seat(seat.to_string()), after_seq, &["message-consumed"])?
            .iter()
            .any(|e| matches!(&e.body, EventBody::MessageConsumed { message_id, .. } if ids.contains(message_id))))
    }

    /// Events at or below this seq belong to starts booted before handoffs existed.
    pub fn handoff_cutoff(&self) -> rusqlite::Result<i64> {
        Ok(self.conn.query_row("SELECT seq FROM handoff_cutoff", [], |r| r.get(0)).optional()?.unwrap_or(0))
    }
}
