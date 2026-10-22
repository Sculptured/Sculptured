//! A team's Jobs: each agent's first message, sent once it runs after a start.
use super::READY_TIMEOUT;
use crate::state::AppState;
use model::{Actor, Desired, EventBody, MessageIntent, Provenance, SeatLifecycle, Subject};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// Actor id for a team's Job messages; the conversation labels them "sent at start".
pub const TEAM_START_ACTOR: &str = "team-start";

/// Actor id for handoff messages (see `handoffs`).
pub use crate::handoffs::HANDOFF_ACTOR;

/// Delivers each `(seat, job)` as that agent's first message once it is running after
/// `after_seq`. An agent that settles without running gets its Job recorded as a
/// failed message saying why, and so does every agent still waiting when the team is
/// stopped or started again. `layers` bounds how long a later layer may take to start.
pub async fn deliver_jobs(
    st: AppState,
    jobs: Vec<(String, String)>,
    after_seq: i64,
    layers: usize,
) {
    let wait = READY_TIMEOUT * (layers.max(1) as u32) + Duration::from_secs(5);
    let tasks: Vec<_> = jobs
        .into_iter()
        .map(|(seat, text)| tokio::spawn(deliver_job(st.clone(), seat, text, after_seq, wait)))
        .collect();
    for t in tasks {
        if let Err(e) = t.await {
            tracing::error!(error = %e, "a job delivery task did not finish");
        }
    }
}

/// Why a Job of an older start was not sent: a newer start began first.
pub const STARTED_AGAIN: &str = "the team was started again before this agent started";

/// Some(Ok) once running; Some(Err(why)) when it settled without running.
fn start_outcome(body: &EventBody) -> Option<Result<(), String>> {
    let EventBody::SeatLifecycleChanged { lifecycle, reason } = body else {
        return None;
    };
    match lifecycle {
        SeatLifecycle::Running => Some(Ok(())),
        SeatLifecycle::Attention | SeatLifecycle::Failed | SeatLifecycle::Stopped => {
            Some(Err(reason
                .clone()
                .unwrap_or_else(|| format!("{lifecycle:?}").to_lowercase())))
        }
        _ => None,
    }
}

/// The first event after `after_seq` that decides a Job: Ok when the agent is running;
/// Err(why it was not sent) when the agent settled without running, or when its team
/// was stopped or started again before that. The first RigBooted after `after_seq` is
/// the start that queued this Job; a second one is a newer start.
fn recorded_outcome(st: &AppState, seat: &str, after_seq: i64) -> Option<Result<(), String>> {
    let mut events = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let mut events = store.events_for_seat(seat, after_seq, 2000).ok()?;
        if let Some(rig) = crate::seat_ids::rig_of(seat) {
            events.extend(
                store
                    .events_for_subject(&Subject::Rig(rig.to_string()), after_seq, 2000)
                    .ok()?,
            );
        }
        events
    };
    events.sort_by_key(|e| e.seq);
    let mut boots = 0;
    events.iter().find_map(|e| match &e.body {
        EventBody::RigDesiredChanged {
            desired: Desired::Stopped,
        } => Some(Err(
            "the team was stopped before this agent started".to_string()
        )),
        EventBody::RigBooted { .. } => {
            boots += 1;
            (boots > 1).then(|| Err(STARTED_AGAIN.to_string()))
        }
        body => {
            start_outcome(body).map(|r| r.map_err(|why| format!("the agent did not start ({why})")))
        }
    })
}

async fn deliver_job(st: AppState, seat: String, text: String, after_seq: i64, wait: Duration) {
    // Subscribe before reading the store so no deciding event falls in between.
    let mut rx = st.hub.subscribe();
    let deadline = tokio::time::Instant::now() + wait;
    let rig = crate::seat_ids::rig_of(&seat).map(|r| Subject::Rig(r.to_string()));
    let mut outcome = recorded_outcome(&st, &seat, after_seq);
    while outcome.is_none() {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Err(_) => {
                outcome = Some(Err(format!(
                    "the agent did not start (it was not running within {}s)",
                    wait.as_secs()
                )))
            }
            Ok(Ok(item)) => {
                let subject = &item.event.subject;
                if item.event.seq > after_seq
                    && (*subject == Subject::Seat(seat.clone()) || rig.as_ref() == Some(subject))
                {
                    outcome = recorded_outcome(&st, &seat, after_seq);
                }
            }
            Ok(Err(RecvError::Lagged(_))) => outcome = recorded_outcome(&st, &seat, after_seq),
            Ok(Err(RecvError::Closed)) => return,
        }
    }
    match outcome {
        Some(Ok(())) => {
            let actor = Actor::System(TEAM_START_ACTOR.into());
            if let Err(e) = crate::reconciler::seats::send_message_as(
                &st,
                &seat,
                text,
                MessageIntent::Inform,
                false,
                actor,
            )
            .await
            {
                tracing::warn!(%seat, error = %e.message, "job delivery failed; recorded as a failed message");
            }
        }
        Some(Err(why)) => record_unsent_job(&st, &seat, text, why),
        None => {}
    }
}

fn record_unsent_job(st: &AppState, seat: &str, text: String, why: String) {
    let message_id = uuid::Uuid::new_v4().to_string();
    let queued = st.local_event(
        Actor::System(TEAM_START_ACTOR.into()),
        Subject::Seat(seat.to_string()),
        Provenance::Operator,
        EventBody::MessageQueued {
            message_id: message_id.clone(),
            intent: MessageIntent::Inform,
            text,
        },
    );
    // "Not sent" is controld's inference from the lifecycle and the team's stop or
    // restart, so it is Heuristic, like the fix wave's other controld inferences.
    let failed = st.local_event(
        crate::hosts::controld(),
        Subject::Seat(seat.to_string()),
        Provenance::Heuristic,
        EventBody::MessageFailed {
            message_id,
            reason: format!("not sent: {why}"),
        },
    );
    if let Err(e) = queued.and(failed) {
        tracing::error!(%seat, error = %e, "cannot record the unsent job");
    }
}
