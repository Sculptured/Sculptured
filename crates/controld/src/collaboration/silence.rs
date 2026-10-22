//! Silent agents (heuristic, an idea from Gas Town's patrol): a running agent in a turn with no output
//! for `QUIET_MS` is flagged as possibly stalled, once, with a `stall-suspected` event. Nothing acts on
//! it; the operator decides, and the agent's next output clears it. One waiting for a person is not
//! silent.
use crate::state::AppState;
use model::{EventBody, Provenance, Subject};
use std::sync::atomic::{AtomicI64, Ordering};

/// Long enough for a slow build or test run to finish quietly.
pub const QUIET_MS: i64 = 30 * 60_000;
const EVERY_MS: i64 = 30_000;

static LAST_MS: AtomicI64 = AtomicI64::new(0);

/// Called every second by the budget loop; looks every `EVERY_MS`.
pub fn check(st: &AppState) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if now - LAST_MS.load(Ordering::Relaxed) < EVERY_MS {
        return;
    }
    LAST_MS.store(now, Ordering::Relaxed);
    let found = match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .silent_seats(now, QUIET_MS)
    {
        Ok(f) => f,
        Err(e) => return tracing::warn!(error = %e, "cannot look for silent agents"),
    };
    for s in found {
        let body = EventBody::StallSuspected { reason: s.reason() };
        if let Err(e) = st.local_event(
            crate::hosts::controld(),
            Subject::Seat(s.seat.clone()),
            Provenance::Heuristic,
            body,
        ) {
            tracing::warn!(seat = %s.seat, error = %e, "cannot flag a silent agent");
        }
    }
}
