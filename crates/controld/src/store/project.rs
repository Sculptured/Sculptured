//! Writing events: each event and its projection onto the seat and message tables commit together.
use super::LOCAL_EVENT_HOST;
use crate::views::StreamItem;
use host_proto::HostEvent;
use model::{Actor, EventBody, EventRecord, Provenance, SeatLifecycle, Subject};
use rusqlite::{Transaction, params};

/// Why a message still queued or delivered when its seat's process ended is failed.
pub const STOPPED_BEFORE_READING: &str = "the agent stopped before reading this message";

pub(super) fn insert_event(
    tx: &Transaction,
    host: &str,
    ev: HostEvent,
) -> rusqlite::Result<EventRecord> {
    let mut rec = EventRecord {
        seq: 0,
        host: host.to_string(),
        host_seq: ev.host_seq,
        at_ms: ev.at_ms,
        actor: ev.actor,
        subject: ev.subject,
        provenance: ev.provenance,
        body: ev.body,
    };
    tx.execute(
        "INSERT INTO events (host, host_seq, at_ms, subject, json) VALUES (?1, ?2, ?3, ?4, '{}')",
        params![host, rec.host_seq as i64, rec.at_ms, rec.subject.key()],
    )?;
    rec.seq = tx.last_insert_rowid();
    tx.execute(
        "UPDATE events SET json = ?2 WHERE seq = ?1",
        params![
            rec.seq,
            serde_json::to_string(&rec).expect("event serializes")
        ],
    )?;
    tx.execute(
        "INSERT INTO host_cursor (host, last_seq) VALUES (?1, ?2)
         ON CONFLICT (host) DO UPDATE SET last_seq = excluded.last_seq",
        params![host, rec.host_seq as i64],
    )?;
    project(tx, &rec)?;
    Ok(rec)
}

fn project(tx: &Transaction, ev: &EventRecord) -> rusqlite::Result<()> {
    if let Subject::Rig(rig) = &ev.subject
        && matches!(
            ev.body,
            EventBody::RigBooted { .. } | EventBody::TeamTopologyChanged { .. }
        )
    {
        if matches!(ev.body, EventBody::RigBooted { .. }) {
            super::runs::snapshot_team(tx, rig, ev.seq)?;
        }
        tx.execute("INSERT INTO deployment_specs SELECT name,(SELECT MAX(seq) FROM events WHERE subject=?2 AND json_extract(json,'$.body.type')='rig-booted'),compiled_json FROM rigs WHERE name=?1 ON CONFLICT(rig,start_seq) DO UPDATE SET json=excluded.json",params![rig,ev.subject.key()])?;
    }
    if let EventBody::PlanUsageReported { usage } = &ev.body {
        let seat = match &ev.subject {
            Subject::Seat(s) => Some(s.as_str()),
            _ => None,
        };
        crate::plan_usage::project(tx, usage, ev.at_ms, seat)?;
    }
    project_seat(tx, ev)?;
    crate::projections::project(tx, ev)
}

fn project_seat(tx: &Transaction, ev: &EventRecord) -> rusqlite::Result<()> {
    let Subject::Seat(seat) = &ev.subject else {
        return Ok(());
    };
    tx.execute(
        "UPDATE seats SET last_event_ms = ?2 WHERE id = ?1",
        params![seat, ev.at_ms],
    )?;
    if matches!(
        ev.body,
        EventBody::AgentText { .. }
            | EventBody::ToolCalled { .. }
            | EventBody::ToolResulted { .. }
            | EventBody::ThinkingRecorded { .. }
            | EventBody::UsageReported { .. }
            | EventBody::ContextReported { .. }
            | EventBody::TurnEnded { .. }
    ) {
        super::stall::on_output(tx, seat)?;
    }
    match &ev.body {
        EventBody::StallSuspected { reason } => {
            super::stall::on_suspected(tx, seat, reason, ev.at_ms)
        }
        EventBody::SeatLifecycleChanged { lifecycle, reason } => {
            project_lifecycle(tx, seat, ev, *lifecycle, reason.as_deref())
        }
        EventBody::MessageQueued { .. }
        | EventBody::MessageDelivered { .. }
        | EventBody::MessageConsumed { .. }
        | EventBody::MessageFailed { .. } => project_message(tx, seat, ev),
        EventBody::TenureEnded { .. }
        | EventBody::TenureStarted { .. }
        | EventBody::ModelObserved { .. }
        | EventBody::RestoreNeeded { .. }
        | EventBody::HandoffWaiting { .. } => project_session(tx, seat, &ev.body),
        EventBody::CheckStarted { command } => {
            let c = crate::views::SeatCheck {
                command: command.clone(),
                passed: None,
                exit_code: None,
                timed_out: false,
                output: String::new(),
                at_ms: ev.at_ms,
                duration_ms: None,
            };
            tx.execute(
                "UPDATE seats SET last_check = ?2 WHERE id = ?1",
                params![seat, serde_json::to_string(&c).expect("check serializes")],
            )
            .map(|_| ())
        }
        EventBody::CheckRan {
            command,
            passed,
            exit_code,
            timed_out,
            output,
            duration_ms,
        } => {
            let c = crate::views::SeatCheck {
                command: command.clone(),
                passed: Some(*passed),
                exit_code: *exit_code,
                timed_out: *timed_out,
                output: output.clone(),
                at_ms: ev.at_ms,
                duration_ms: Some(*duration_ms),
            };
            tx.execute(
                "UPDATE seats SET last_check = ?2 WHERE id = ?1",
                params![seat, serde_json::to_string(&c).expect("check serializes")],
            )
            .map(|_| ())
        }
        _ => project_turn(tx, seat, ev),
    }
}

/// The process is gone: a message it had not read yet will never be read by it.
fn fail_unread(tx: &Transaction, seat: &str) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE messages SET state = 'failed', reason = ?2 WHERE seat = ?1 AND state IN ('queued', 'delivered')",
        params![seat, STOPPED_BEFORE_READING],
    )?;
    Ok(())
}

/// Sets a seat's JSON column (`restore_offer`, `limit_hit`).
fn set_json(
    tx: &Transaction,
    seat: &str,
    column: &str,
    value: &impl serde::Serialize,
) -> rusqlite::Result<()> {
    let json = serde_json::to_string(value).expect("a view serializes");
    tx.execute(
        &format!("UPDATE seats SET {column} = ?2 WHERE id = ?1"),
        params![seat, json],
    )?;
    Ok(())
}

fn project_lifecycle(
    tx: &Transaction,
    seat: &str,
    ev: &EventRecord,
    lifecycle: SeatLifecycle,
    reason: Option<&str>,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE seats SET lifecycle = ?2, lifecycle_reason = ?3, lifecycle_provenance = ?4 WHERE id = ?1",
        params![seat, crate::store::rows::enum_text(&lifecycle), reason, crate::store::rows::enum_text(&ev.provenance)],
    )?;
    if !keeps_turn_state(lifecycle) {
        // No process is running for this seat: its turn, tool and activity are
        // no longer current, so they must not be shown as if they were.
        tx.execute("UPDATE seats SET current_turn = NULL, current_tool = NULL, activity = 'unknown' WHERE id = ?1", [seat])?;
    }
    if matches!(lifecycle, SeatLifecycle::Stopped | SeatLifecycle::Failed) {
        // The process is gone even when no TenureEnded arrives (an attention seat the
        // host reports as not running is recorded as Stopped), so unread messages fail.
        fail_unread(tx, seat)?;
        tx.execute(
            "UPDATE seats SET restore_offer = NULL WHERE id = ?1",
            [seat],
        )?;
    }
    if ev.host != LOCAL_EVENT_HOST {
        tx.execute(
            "UPDATE seats SET host = ?2 WHERE id = ?1",
            params![seat, ev.host],
        )?;
    }
    Ok(())
}

fn project_message(tx: &Transaction, seat: &str, ev: &EventRecord) -> rusqlite::Result<()> {
    match &ev.body {
        EventBody::MessageQueued { message_id, intent, text } => tx.execute(
            "INSERT OR IGNORE INTO messages (id, seat, intent, text, state, at_ms)
             VALUES (?1, ?2, ?3, ?4, 'queued', ?5)",
            params![message_id, seat, crate::store::rows::enum_text(intent), text, ev.at_ms],
        )?,
        EventBody::MessageDelivered { message_id } => {
            tx.execute("UPDATE messages SET state = 'delivered' WHERE id = ?1 AND state = 'queued'", [message_id])?
        }
        EventBody::MessageConsumed { message_id, turn_id } => tx.execute(
            "UPDATE messages SET state = 'consumed', turn_id = ?2
             WHERE id = ?1 AND state IN ('queued', 'delivered')",
            params![message_id, turn_id],
        )?,
        EventBody::MessageFailed { message_id, reason } => tx.execute(
            "UPDATE messages SET state = 'failed', reason = ?2 WHERE id = ?1 AND state IN ('queued', 'delivered')",
            params![message_id, reason],
        )?,
        _ => 0,
    };
    Ok(())
}

/// The session and what controld records about it (restore offer, handoff waiting).
fn project_session(tx: &Transaction, seat: &str, body: &EventBody) -> rusqlite::Result<()> {
    match body {
        EventBody::TenureEnded { .. } => {
            // The process is gone: its turn and tool are no longer current.
            tx.execute(
                "UPDATE seats SET current_turn = NULL, current_tool = NULL WHERE id = ?1",
                [seat],
            )?;
            fail_unread(tx, seat)?;
        }
        EventBody::TenureStarted {
            tenure,
            native_session_id,
            caps,
            ..
        } => {
            tx.execute(
                "UPDATE seats SET tenure = ?2, native_session_id = ?3, caps_json = ?4,
                     current_turn = NULL, current_tool = NULL, restore_offer = NULL, limit_hit = NULL, stalled = NULL,
                     model_requested = NULL, model_observed = NULL WHERE id = ?1",
                params![seat, *tenure as i64, native_session_id, serde_json::to_string(caps).expect("caps")],
            )?;
        }
        // A new session asks again (TenureStarted cleared both); a record without the CLI's word keeps
        // the observed model it already reported.
        EventBody::ModelObserved {
            requested,
            observed,
            ..
        } => {
            tx.execute(
                "UPDATE seats SET model_requested = ?2, model_observed = COALESCE(?3, model_observed) WHERE id = ?1",
                params![seat, requested, observed],
            )?;
        }
        EventBody::RestoreNeeded { reason, can_resume } => {
            set_json(
                tx,
                seat,
                "restore_offer",
                &crate::views::RestoreOffer {
                    reason: reason.clone(),
                    can_resume: *can_resume,
                },
            )?;
        }
        EventBody::HandoffWaiting { waiting_for, stuck } => {
            let value = (!waiting_for.is_empty())
                .then(|| serde_json::to_string(waiting_for).expect("ids serialize"));
            let stuck = stuck
                .as_ref()
                .map(|s| serde_json::to_string(s).expect("stuck serializes"));
            tx.execute(
                "UPDATE seats SET waiting_for = ?2, handoff_stuck = ?3 WHERE id = ?1",
                params![seat, value, stuck],
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn project_turn(tx: &Transaction, seat: &str, ev: &EventRecord) -> rusqlite::Result<()> {
    match &ev.body {
        EventBody::SeatActivityChanged { activity } => {
            tx.execute(
                "UPDATE seats SET activity = ?2 WHERE id = ?1",
                params![seat, crate::store::rows::enum_text(activity)],
            )?;
        }
        EventBody::TurnStarted { turn_id } => {
            tx.execute(
                "UPDATE seats SET current_turn = ?2, current_tool = NULL WHERE id = ?1",
                params![seat, turn_id],
            )?;
            tx.execute(
                "UPDATE seats SET limit_hit = NULL, stalled = NULL WHERE id = ?1",
                [seat],
            )?;
        }
        EventBody::ToolResulted { is_error, .. } => {
            super::stall::on_tool_result(tx, seat, *is_error, ev.at_ms)?
        }
        EventBody::LimitReached {
            kind, resets, text, ..
        } => {
            set_json(
                tx,
                seat,
                "limit_hit",
                &crate::views::LimitHit {
                    kind: *kind,
                    resets: resets.clone(),
                    text: text.clone(),
                    at_ms: ev.at_ms,
                },
            )?;
        }
        EventBody::TurnEnded { .. } => {
            tx.execute(
                "UPDATE seats SET current_turn = NULL, current_tool = NULL WHERE id = ?1",
                [seat],
            )?;
        }
        EventBody::ToolCalled {
            name,
            parent_call_id: None,
            ..
        } => {
            tx.execute(
                "UPDATE seats SET current_tool = ?2 WHERE id = ?1",
                params![seat, name],
            )?;
        }
        EventBody::UsageReported {
            input_tokens,
            output_tokens,
            ..
        } => {
            tx.execute(
                "UPDATE seats SET tokens_in = COALESCE(tokens_in, 0) + ?2,
                     tokens_out = COALESCE(tokens_out, 0) + ?3 WHERE id = ?1",
                params![seat, *input_tokens as i64, *output_tokens as i64],
            )?;
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn item_for(tx: &Transaction, event: EventRecord) -> rusqlite::Result<StreamItem> {
    let seat = match &event.subject {
        Subject::Seat(id) => crate::store::rows::seat_in(tx, id)?,
        _ => None,
    };
    Ok(StreamItem { event, seat })
}

/// controld's own next event (host `LOCAL_EVENT_HOST`), numbered after its last one.
pub(super) fn local_event(
    tx: &Transaction,
    at_ms: i64,
    actor: Actor,
    subject: Subject,
    provenance: Provenance,
    body: EventBody,
) -> rusqlite::Result<HostEvent> {
    Ok(HostEvent {
        host_seq: crate::store::rows::cursor_in(tx, LOCAL_EVENT_HOST)? + 1,
        at_ms,
        actor,
        subject,
        provenance,
        body,
    })
}

pub(super) fn append_local_in(
    tx: &Transaction,
    actor: Actor,
    subject: Subject,
    provenance: Provenance,
    body: EventBody,
) -> rusqlite::Result<StreamItem> {
    let ev = local_event(tx, model::now_ms(), actor, subject, provenance, body)?;
    let rec = insert_event(tx, LOCAL_EVENT_HOST, ev)?;
    item_for(tx, rec)
}

/// Lifecycles after which a seat's turn, tool and activity may still be current: the
/// process is running or starting, or may still be alive (stale: host unreachable;
/// stopping: not yet exited). Every other lifecycle clears them.
fn keeps_turn_state(l: SeatLifecycle) -> bool {
    matches!(
        l,
        SeatLifecycle::Running
            | SeatLifecycle::Launching
            | SeatLifecycle::Stale
            | SeatLifecycle::Stopping
    )
}
