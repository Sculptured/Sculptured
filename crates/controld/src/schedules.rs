//! Schedules (`TeamDraft.schedules`): while a team runs, each sends its agent a message every N minutes.
//! The first comes one interval after the team starts. An agent that is busy (in a turn, with a message
//! waiting, or being stopped) is skipped, never queued for, and the skip is recorded.
use crate::error::ApiError;
use crate::extract::Path;
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use model::{Desired, MessageIntent, SeatLifecycle, Subject};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/schedules", axum::routing::get(runs))
}

async fn runs(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::error::respond(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .schedule_runs(&id, 50)
            .map(|r| json!(r))
            .map_err(ApiError::from),
    )
}

/// What a due schedule should do now: send, or skip and say why.
enum Due {
    Send { seat: String, message: String },
    Skip(&'static str),
}

/// Sends every due schedule of a running team (called each scheduler tick).
pub async fn tick(st: &AppState, rig: &str) {
    let due = match due(st, rig) {
        Ok(d) => d,
        Err(e) => return tracing::warn!(%rig, error = %e, "cannot read schedules"),
    };
    for (schedule, what) in due {
        let record = |outcome: &str| {
            st.store
                .lock()
                .expect("store mutex must not be poisoned")
                .record_schedule_run(rig, &schedule, outcome)
        };
        match what {
            Due::Skip(why) => {
                if let Err(e) = record(why) {
                    tracing::warn!(%rig, %schedule, error = %e, "cannot record a schedule run");
                }
            }
            Due::Send { seat, message } => {
                // Recorded before sending: if the record can't be written, nothing is sent (no repeat every second).
                let run = match record("sending") {
                    Ok(run) => run,
                    Err(e) => {
                        tracing::warn!(%rig, %schedule, error = %e, "cannot record a schedule run; not sending");
                        continue;
                    }
                };
                let actor = model::Actor::System(format!("schedule:{schedule}"));
                let outcome = match crate::reconciler::send_message_as(
                    st,
                    &seat,
                    message,
                    MessageIntent::Inform,
                    false,
                    actor,
                )
                .await
                {
                    Ok(_) => "sent".to_string(),
                    Err(e) => format!("not delivered: {}", e.message),
                };
                if let Err(e) = st
                    .store
                    .lock()
                    .expect("store mutex must not be poisoned")
                    .set_schedule_outcome(run, &outcome)
                {
                    tracing::warn!(%rig, %schedule, error = %e, "cannot record a schedule outcome");
                }
            }
        }
    }
}

fn due(st: &AppState, rig: &str) -> rusqlite::Result<Vec<(String, Due)>> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let Some(d) = store.draft(rig)? else {
        return Ok(vec![]);
    };
    if d.schedules.is_empty()
        || store
            .rig_snapshot(rig)?
            .is_none_or(|s| s.desired != Desired::Running)
    {
        return Ok(vec![]);
    }
    let Some(booted) = store
        .latest_of_types(&Subject::Rig(rig.into()), &["rig-booted"], 1)?
        .first()
        .map(|e| e.at_ms)
    else {
        return Ok(vec![]);
    };
    let now = model::now_ms();
    let mut out = Vec::new();
    // One message per agent per tick, so two schedules for the same agent never queue up.
    let mut sending = std::collections::HashSet::new();
    for s in &d.schedules {
        let last = store
            .last_schedule_run(rig, &s.id)?
            .unwrap_or(booted)
            .max(booted);
        if now - last < i64::from(s.every_minutes) * 60_000 {
            continue;
        }
        let Some(seat) = d.seat_id(&s.agent) else {
            continue;
        };
        let view = store.seat(&seat)?;
        let what = if !view
            .as_ref()
            .is_some_and(|v| v.lifecycle == SeatLifecycle::Running)
        {
            Due::Skip("skipped: agent not running")
        } else if store.control(&seat)?.is_some() {
            Due::Skip("skipped: agent is being stopped")
        } else if view.is_some_and(|v| v.current_turn.is_some())
            || store.has_pending_messages(&seat)?
            || !sending.insert(seat.clone())
        {
            Due::Skip("skipped: busy")
        } else {
            Due::Send {
                seat,
                message: s.message.clone(),
            }
        };
        out.push((s.id.clone(), what));
    }
    Ok(out)
}
