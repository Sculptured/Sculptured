//! What the operator can ask of handoffs: "Hand over again", "Resend", and what a restarted
//! agent is owed.
use super::plan::Delivery;
use super::team::{Team, TeamNames};
use super::work::SeatWork;
use super::{InFlight, TEAM_START_ACTOR};
use crate::error::ApiError;
use crate::state::AppState;
use crate::store::{HandedOverRecord, ResendText, Store};
use crate::views::SeatView;
use model::{Actor, EventBody, MessageIntent, Provenance, SeatLifecycle, Subject};
use std::collections::HashSet;

fn seat_view(st: &AppState, seat: &str) -> Result<SeatView, ApiError> {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)?
        .ok_or_else(|| ApiError::not_found(format!("no agent {seat:?}")))
}

/// The current start of `rig`, refusing a team that is not running.
pub(super) fn running_start(store: &Store, rig: &str) -> Result<i64, ApiError> {
    match crate::handoffs::starts::starts(store, rig)?.0 {
        Some((seq, None)) => Ok(seq),
        _ => Err(ApiError::conflict(
            "team_not_running",
            "This team is not running. Start the team first.",
        )),
    }
}

/// "Hand over again": records that the operator wants `seat`'s latest finished turn
/// passed along. That starts a wave: every agent downstream runs once (for one still
/// waiting for its first handoff, it counts as this agent's result).
pub fn request_handover(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let rig = seat_view(st, seat)?.rig;
    let latest = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let start = running_start(&store, &rig)?;
        let snap = store
            .rig_snapshot(&rig)?
            .ok_or_else(|| ApiError::not_found(format!("no team {rig:?}")))?;
        let names = TeamNames::new(&snap.rig, store.draft(&rig)?.as_ref());
        let name = names.name(seat);
        if crate::handoffs::team::downstreams_of(&snap.rig, seat).is_empty() {
            return Err(ApiError::conflict(
                "no_downstream",
                format!("{name} does not hand work to anyone."),
            ));
        }
        let w = crate::handoffs::work::seat_work(
            &store,
            seat,
            start,
            &store.handed_over(seat, start)?,
            false,
        )?;
        w.turns.last().cloned().ok_or_else(|| {
            ApiError::conflict(
                "nothing_to_hand_over",
                format!("{name} has not finished a turn since the team started."),
            )
        })?
    };
    st.local_event(
        Actor::Human("local".into()),
        Subject::Seat(seat.to_string()),
        Provenance::Operator,
        EventBody::HandoverRequested {
            turn_id: latest.turn_id,
            turn_seq: latest.seq,
        },
    )?;
    Ok(())
}

/// Re-sends `seat`'s handoffs of the current start that never reached it (every copy
/// not sent, or queued and not being sent), and a first agent's Job that was not sent
/// although it runs. A handoff the host reports delivered or read is never sent again.
/// Returns how many were re-sent.
pub async fn resend(st: &AppState, seat: &str) -> Result<usize, ApiError> {
    let ResendPlan {
        start,
        originals,
        names,
        job,
    } = resend_plan(st, seat)?;
    let mut sent = 0;
    if let Some((text, _sending)) = job
        && send_job(st, seat, text, "job resend").await
    {
        sent += 1;
    }
    for original in originals {
        if resend_one(st, seat, start, &original, ResendText::Same, false, &names).await {
            sent += 1;
        }
    }
    if sent == 0 {
        return Err(ApiError::conflict(
            "nothing_to_resend",
            format!(
                "Nothing to resend: every handoff to {} reached it, and so did its Job.",
                names.name(seat)
            ),
        ));
    }
    Ok(sent)
}

/// What "Resend" sends: the handoffs (originals' ids) that never reached the agent, and its
/// Job when that was not sent (marked in flight while it is sent).
struct ResendPlan {
    start: i64,
    originals: Vec<String>,
    names: TeamNames,
    job: Option<(String, InFlight)>,
}

fn resend_plan(st: &AppState, seat: &str) -> Result<ResendPlan, ApiError> {
    let view = seat_view(st, seat)?;
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let start = running_start(&store, &view.rig)?;
    let snap = store
        .rig_snapshot(&view.rig)?
        .ok_or_else(|| ApiError::not_found(format!("no team {:?}", view.rig)))?;
    let draft = store.draft(&view.rig)?;
    let names = TeamNames::new(&snap.rig, draft.as_ref());
    if view.lifecycle != SeatLifecycle::Running {
        return Err(ApiError::conflict(
            "agent_not_running",
            format!(
                "{} is not running. Restart it first; what it is owed is then sent.",
                names.name(seat)
            ),
        ));
    }
    let in_flight = st
        .handoffs_in_flight
        .lock()
        .expect("handoffs_in_flight mutex must not be poisoned")
        .clone();
    let records = store.handed_over(seat, start)?;
    let w = crate::handoffs::work::seat_work(&store, seat, start, &records, false)?;
    let originals = unreached(&store, &w, &records, &in_flight)?;
    let team = Team::load(&store, &snap.rig, &snap.seats, draft)?;
    let job = job_to_resend(st, &store, &team, seat, start, &w)?;
    Ok(ResendPlan {
        start,
        originals,
        names,
        job,
    })
}

/// The handoffs (originals' ids) whose every copy was not sent, or is queued and not being sent.
fn unreached(
    store: &Store,
    w: &SeatWork,
    records: &[HandedOverRecord],
    in_flight: &HashSet<String>,
) -> rusqlite::Result<Vec<String>> {
    let mut originals = Vec::new();
    for (r, copies) in crate::handoffs::plan::groups(records) {
        if matches!(
            crate::handoffs::plan::delivery(store, w, &copies, in_flight)?,
            Delivery::NotSent | Delivery::NotDelivered
        ) {
            originals.push(r.message_id.clone());
        }
    }
    Ok(originals)
}

/// A first agent's Job that was not sent although it runs is sent again, once: a second press
/// while it is being sent finds it in flight.
fn job_to_resend(
    st: &AppState,
    store: &Store,
    team: &Team,
    seat: &str,
    start: i64,
    w: &SeatWork,
) -> rusqlite::Result<Option<(String, InFlight)>> {
    let head = !crate::handoffs::team::upstreams_of(team.rig)
        .iter()
        .any(|(b, _)| b == seat);
    let in_flight = st
        .handoffs_in_flight
        .lock()
        .expect("handoffs_in_flight mutex must not be poisoned")
        .clone();
    if !head || !crate::handoffs::plan::job_unsent(store, team, seat, start, w, &in_flight)? {
        return Ok(None);
    }
    let key = format!("job:{seat}");
    if !st
        .handoffs_in_flight
        .lock()
        .expect("handoffs_in_flight mutex must not be poisoned")
        .insert(key.clone())
    {
        return Ok(None);
    }
    Ok(team.job(seat).map(|text| {
        (
            text,
            InFlight {
                st: st.clone(),
                id: key,
            },
        )
    }))
}

/// Sends `text` as the agent's Job (actor team-start). A failure is recorded as a failed message.
pub(super) async fn send_job(st: &AppState, seat: &str, text: String, what: &str) -> bool {
    let actor = Actor::System(TEAM_START_ACTOR.into());
    match crate::reconciler::send_message_as(st, seat, text, MessageIntent::Inform, false, actor)
        .await
    {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(%seat, error = %e.message, "{what} failed; recorded as a failed message");
            false
        }
    }
}

pub(super) async fn resend_one(
    st: &AppState,
    seat: &str,
    start: i64,
    original: &str,
    text: ResendText<'_>,
    continuing: bool,
    names: &TeamNames,
) -> bool {
    let new_id = uuid::Uuid::new_v4().to_string();
    let _sending = InFlight::new(st, &new_id);
    let (text, host) = {
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        let in_flight = st
            .handoffs_in_flight
            .lock()
            .expect("handoffs_in_flight mutex must not be poisoned")
            .iter()
            .filter(|id| **id != new_id)
            .cloned()
            .collect();
        let host = store.seat(seat).ok().flatten().and_then(|v| v.host);
        match store.record_resend(seat, start, original, &new_id, text, continuing, &in_flight) {
            Ok(Some((items, text))) => {
                st.publish(items);
                (text, host)
            }
            Ok(None) => return false,
            Err(e) => {
                tracing::error!(%seat, error = %e, "handoffs: cannot record a resend");
                return false;
            }
        }
    };
    let host = host.unwrap_or_else(|| crate::launch::agent_host(st, seat));
    crate::handoffs::send(st, seat, &new_id, text, &host, &names.name(seat), names).await;
    true
}
