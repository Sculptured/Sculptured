//! Needs you: the questions and agents waiting for the operator, oldest first.
use crate::error::ApiError;
use crate::projections::questions::QuestionRow;
use crate::state::AppState;
use crate::store::Store;
use crate::views::{InboxItem, InboxKind, SeatView};
use model::{EventBody, QuestionSource, QuestionState, SeatLifecycle, Subject};

/// Some(true): the event puts the agent in Needs you for the kind; Some(false): it ends that; None: neither.
type Classify = fn(&EventBody) -> Option<bool>;

/// An agent's reason to be in Needs you.
fn seat_kind(s: &SeatView) -> InboxKind {
    if s.restore_offer.is_some() {
        InboxKind::Restore
    } else if s.lifecycle == SeatLifecycle::Running && s.stalled.is_some() {
        InboxKind::Stalled
    } else if s.lifecycle == SeatLifecycle::Running && s.limit_hit.is_some() {
        // Its last turn ended on a usage or rate limit: say so, not "stuck". Nothing retries it.
        InboxKind::Limit
    } else if s.lifecycle == SeatLifecycle::Running {
        InboxKind::Stuck
    } else {
        InboxKind::Attention
    }
}

/// The events that put an agent in Needs you for `kind`, or take it out; None for a limit (its own time).
fn needs_rule(kind: InboxKind) -> Option<(&'static [&'static str], Classify)> {
    Some(match kind {
        InboxKind::Limit | InboxKind::Stalled => return None,
        InboxKind::Restore => (
            &["restore-needed", "tenure-started", "seat-lifecycle-changed"],
            |b| match b {
                EventBody::RestoreNeeded { .. } => Some(true),
                EventBody::TenureStarted { .. } => Some(false),
                EventBody::SeatLifecycleChanged {
                    lifecycle: SeatLifecycle::Stopped | SeatLifecycle::Failed,
                    ..
                } => Some(false),
                _ => None,
            },
        ),
        InboxKind::Stuck => (&["handoff-waiting"], |b| match b {
            EventBody::HandoffWaiting { stuck, .. } => Some(stuck.is_some()),
            _ => None,
        }),
        _ => (&["seat-lifecycle-changed"], |b| match b {
            EventBody::SeatLifecycleChanged { lifecycle, .. } => Some(matches!(
                lifecycle,
                SeatLifecycle::Attention | SeatLifecycle::Failed | SeatLifecycle::Stale
            )),
            _ => None,
        }),
    })
}

/// When the agent started needing the operator for `kind`: the first event of the unbroken run of events
/// that keep it there. Later events (even repeats of the same reason) never move it, so the list order is
/// stable. Falls back to the agent's latest event when the history does not say.
fn needs_since(store: &Store, s: &SeatView, kind: InboxKind) -> i64 {
    let fallback = s.last_event_ms.unwrap_or(0);
    let Some((types, classify)) = needs_rule(kind) else {
        return s
            .stalled
            .as_ref()
            .map(|x| x.at_ms)
            .or(s.limit_hit.as_ref().map(|h| h.at_ms))
            .unwrap_or(fallback);
    };
    let recent = match store.latest_of_types(&Subject::Seat(s.id.clone()), types, 500) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(seat = %s.id, error = %e, "cannot read when the agent started needing attention");
            return fallback;
        }
    };
    let mut since = None;
    for ev in &recent {
        match classify(&ev.body) {
            Some(true) => since = Some(ev.at_ms),
            Some(false) => break,
            None => {}
        }
    }
    since.unwrap_or(fallback)
}

const WAITING_QUESTIONS: &str = "state = 'open' OR (state = 'expired' AND acknowledged = 0)
    OR (state = 'answered' AND unconfirmed_reason IS NOT NULL AND acknowledged = 0 AND EXISTS (
        SELECT 1 FROM seats s WHERE s.id = questions.seat AND s.lifecycle IN ('launching', 'running', 'stopping', 'stale')
            AND (questions.turn_id IS NULL OR questions.turn_id = s.current_turn)))";

/// The agents in Needs you, with why and since when.
fn waiting_seats(store: &Store) -> rusqlite::Result<Vec<(SeatView, InboxKind, i64)>> {
    let ids: Vec<String> = {
        let mut q = store.connection().prepare(
            "SELECT id FROM seats WHERE lifecycle IN ('attention', 'failed', 'stale')
                OR (lifecycle = 'running' AND (handoff_stuck IS NOT NULL OR limit_hit IS NOT NULL OR stalled IS NOT NULL)) ORDER BY rig, ord",
        )?;
        q.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    Ok(ids
        .iter()
        .filter_map(|id| store.seat(id).ok().flatten())
        .map(|s| {
            let kind = seat_kind(&s);
            let since = needs_since(store, &s, kind);
            (s, kind, since)
        })
        .collect())
}

fn question_item(st: &AppState, q: QuestionRow) -> InboxItem {
    let kind = match (q.state, q.source) {
        (QuestionState::Answered, _) => InboxKind::Unconfirmed,
        (QuestionState::Expired, _) => InboxKind::Expired,
        (_, QuestionSource::NativePermission) => InboxKind::Permission,
        _ => InboxKind::Question,
    };
    InboxItem {
        id: format!("q:{}", q.id),
        kind,
        rig: q.rig.clone(),
        seat: Some(q.seat.clone()),
        opened_at_ms: q.opened_at_ms,
        question: Some(crate::questions::view(st, q)),
        seat_view: None,
        decision: None,
    }
}

fn seat_item((s, kind, since): (SeatView, InboxKind, i64)) -> InboxItem {
    InboxItem {
        id: format!("seat:{}", s.id),
        kind,
        rig: s.rig.clone(),
        seat: Some(s.id.clone()),
        opened_at_ms: since,
        question: None,
        seat_view: Some(s),
        decision: None,
    }
}

/// Needs you, oldest first: open questions, expired ones not yet seen, agents in attention, failed or
/// stale (a restore waiting for a decision is its own kind), running agents whose last turn hit a usage or
/// rate limit, and running agents whose handoffs are stuck. An answered question whose answer may not have
/// reached the agent stays too, while the agent may still be waiting for it (its process runs and the asking
/// turn is current) and until the operator acknowledges it: a lost answer never leaves an agent stalled with
/// nothing in Needs you.
pub fn inbox(st: &AppState) -> Result<Vec<InboxItem>, ApiError> {
    let (rows, seats, decisions) = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        (
            crate::projections::questions::questions_where(
                store.connection(),
                WAITING_QUESTIONS,
                [],
            )?,
            waiting_seats(&store)?,
            crate::workflow::decisions(&store),
        )
    };
    let mut items: Vec<InboxItem> = rows.into_iter().map(|q| question_item(st, q)).collect();
    items.extend(seats.into_iter().map(seat_item));
    items.extend(decisions);
    // Urgent questions first, low ones last (as their agents say); otherwise oldest first.
    let rank = |i: &InboxItem| match i.question.as_ref().and_then(|q| q.prompt.urgency) {
        Some(model::Urgency::High) => 0,
        Some(model::Urgency::Low) => 2,
        _ => 1,
    };
    items.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then(a.opened_at_ms.cmp(&b.opened_at_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(items)
}
