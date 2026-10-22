//! The action interpreter's place in the event path (spec §6.4). When a tool call's result is
//! stored, the call is read (`interpreter` crate) and an `ActionInterpreted` event (heuristic,
//! `system:interpreter`, shown as `≈ heuristic`) is appended in the same transaction.
//! Observe-only: it never blocks anything, and it never claims a verified judgement. The call's
//! own `ToolCalled` and `ToolResulted` events stay the verbatim evidence.
use model::{EventBody, EventRecord, RiskFlag, Subject};
use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;
use std::path::Path;

/// The event the interpreter derives from `ev`: an `ActionInterpreted` for the first result of a
/// known call of the seat's current session. A result for a call never seen derives nothing.
pub(crate) fn derive(tx: &Transaction, ev: &EventRecord) -> rusqlite::Result<Option<EventBody>> {
    let (
        Subject::Seat(seat),
        EventBody::ToolResulted {
            call_id,
            output,
            is_error,
            ..
        },
    ) = (&ev.subject, &ev.body)
    else {
        return Ok(None);
    };
    // The projection has just recorded this result on the call's row; a repeated result left it alone.
    let call: Option<(String, String)> = tx
        .query_row(
            "SELECT tool, input_json FROM actions WHERE seat = ?1 AND tenure = ?2 AND call_id = ?3 AND result_seq = ?4",
            params![seat, crate::projections::actions::tenure_of(tx, seat)?, call_id, ev.seq],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((tool, input)) = call else {
        return Ok(None);
    };
    let input: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let cwd: Option<String> = tx
        .query_row("SELECT cwd FROM seats WHERE id = ?1", [seat], |r| r.get(0))
        .optional()?
        .flatten();
    let (class, paths) = interpreter::classify(&tool, &input);
    let risks = interpreter::risks(
        &tool,
        &input,
        output,
        !is_error,
        cwd.as_deref().map(Path::new),
    );
    Ok(Some(EventBody::ActionInterpreted {
        call_id: call_id.clone(),
        class,
        paths,
        risks,
    }))
}

/// What a pending tool call would do, for a permission prompt (`≈ heuristic`; empty when nothing
/// stands out, which is not a judgement that the call is safe).
#[must_use]
pub fn consequence(tool: &str, input: &Value, cwd: Option<&Path>) -> Vec<RiskFlag> {
    interpreter::consequence(tool, input, cwd)
}
