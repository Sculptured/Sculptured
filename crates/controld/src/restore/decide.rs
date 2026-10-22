//! The operator's answer to a restore offer: resume the agent's own session, start it fresh, or leave
//! it stopped. A team answer applies to every agent of the team still waiting, spawned children
//! included, in launch order. A failed resume never falls back to a fresh start: the agent keeps a
//! new offer that says why and cannot be resumed.
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::handoffs::{OwedMode, Restarting};
use crate::state::AppState;
use crate::views::SeatView;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use host_proto::LaunchMode;
use model::{Actor, EventBody, Provenance, RestoreDecision, SeatLifecycle, Subject, TenureCause};
use rigspec::RestorePolicy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/seats/{id}/restore", axum::routing::post(seat_route))
        .route("/api/teams/{id}/restore", axum::routing::post(team_route))
}

#[derive(Serialize, Deserialize)]
struct RestoreReq {
    request_id: String,
    decision: RestoreDecision,
}

async fn seat_route(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RestoreReq>,
) -> Response {
    let fp = crate::api::fingerprint(&format!("restore-seat:{id}"), &req);
    let (st2, decision) = (st.clone(), req.decision);
    crate::api::idempotent(&st, &req.request_id, fp, move || async move {
        let rig = crate::seat_ids::rig_of(&id)
            .ok_or_else(|| ApiError::not_found(format!("no agent {id:?}")))?
            .to_string();
        let _claim = st2.try_begin_boot(&rig).ok_or_else(starting)?;
        restore_one(&st2, &id, decision).await?;
        Ok(json!({}))
    })
    .await
}

async fn team_route(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RestoreReq>,
) -> Response {
    let fp = crate::api::fingerprint(&format!("restore-team:{id}"), &req);
    let (st2, decision) = (st.clone(), req.decision);
    crate::api::idempotent(&st, &req.request_id, fp, move || async move {
        let _claim = st2.try_begin_boot(&id).ok_or_else(starting)?;
        restore_team(&st2, &id, decision).await
    })
    .await
}

fn starting() -> ApiError {
    ApiError::conflict(
        "team_starting",
        "This team is still starting. Answer once it has finished starting.",
    )
}

/// Every waiting agent of the team, in launch order. With Resume, agents that cannot be resumed keep
/// waiting (they need Start fresh or Leave); each failure is reported, the rest go on.
async fn restore_team(
    st: &AppState,
    rig: &str,
    decision: RestoreDecision,
) -> Result<Value, ApiError> {
    let (mut done, mut waiting, mut failed) = (vec![], vec![], vec![]);
    for (seat, policy) in waiting_seats(st, rig)? {
        let resumable = seat.restore_offer.as_ref().is_some_and(|o| o.can_resume);
        // The team's Resume follows each agent's "When it restarts" setting. Sculptured keeps no
        // checkpoints, so a checkpoint-only agent waits for its own decision.
        let decision = match (decision, policy) {
            (RestoreDecision::Resume, RestorePolicy::RelaunchFresh) => RestoreDecision::Fresh,
            (RestoreDecision::Resume, RestorePolicy::CheckpointOnly) => {
                waiting.push(seat.id);
                continue;
            }
            (d, _) => d,
        };
        if decision == RestoreDecision::Resume && !resumable {
            waiting.push(seat.id);
            continue;
        }
        match restore_one(st, &seat.id, decision).await {
            Ok(()) => done.push(seat.id),
            Err(e) => failed.push(json!({ "seat": seat.id, "reason": e.message })),
        }
    }
    Ok(json!({ "restored": done, "still_waiting": waiting, "failed": failed }))
}

/// The team's agents with a restore offer, in launch order (parents before their children), with
/// each one's restart setting.
fn waiting_seats(st: &AppState, rig: &str) -> Result<Vec<(SeatView, RestorePolicy)>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let snap = store
        .rig_snapshot(rig)?
        .ok_or_else(|| ApiError::not_found(format!("no team {rig:?}")))?;
    let policy = |id: &str| {
        snap.rig
            .seats
            .iter()
            .find(|s| s.id == id)
            .map_or(RestorePolicy::ResumeIfPossible, |s| s.restore_policy)
    };
    let mut out = vec![];
    let flattened_launch_layers = snap.rig.launch_layers.iter().flatten();
    for id in flattened_launch_layers {
        if let Some(v) = store.seat(id)?.filter(|v| v.restore_offer.is_some()) {
            out.push((v, policy(id)));
        }
    }
    Ok(out)
}

async fn restore_one(st: &AppState, seat: &str, decision: RestoreDecision) -> Result<(), ApiError> {
    let view = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)?
        .ok_or_else(|| ApiError::not_found(format!("no agent {seat:?}")))?;
    let offer = view.restore_offer.clone().ok_or_else(|| {
        ApiError::conflict(
            "nothing_to_restore",
            "This agent is not waiting to be restored.",
        )
    })?;
    let operator = Actor::Human("local".into());
    st.local_event(
        operator,
        Subject::Seat(seat.to_string()),
        Provenance::Operator,
        EventBody::RestoreDecided { decision },
    )?;
    match decision {
        RestoreDecision::Leave => {
            crate::hosts::record_lifecycle(
                st,
                seat.to_string(),
                Provenance::Operator,
                SeatLifecycle::Stopped,
                "Left stopped after the restart".into(),
            );
            Ok(())
        }
        RestoreDecision::Fresh => {
            relaunch_and_send(st, seat, LaunchMode::Fresh, TenureCause::Fresh, None).await
        }
        RestoreDecision::Resume => {
            let id = view.native_session_id.filter(|_| offer.can_resume);
            let id = id.ok_or_else(|| ApiError::conflict("cannot_resume", offer.reason))?;
            let baseline = Some(usage_baseline(st, seat)?);
            relaunch_and_send(
                st,
                seat,
                LaunchMode::Resume {
                    native_session_id: id,
                },
                TenureCause::Resume,
                baseline,
            )
            .await
        }
    }
}

/// Relaunches, then sends what the agent is still owed. A resume that does not come up leaves a new,
/// non-resumable offer with the reason, so the operator decides again.
pub(crate) async fn relaunch_and_send(
    st: &AppState,
    seat: &str,
    mode: LaunchMode,
    cause: TenureCause,
    baseline: Option<[u64; 4]>,
) -> Result<(), ApiError> {
    let owed = if cause == TenureCause::Resume {
        OwedMode::Resumed
    } else {
        OwedMode::Fresh
    };
    let restarting = Restarting::new(st, seat);
    let settled = crate::launch::relaunch(st, seat, mode, cause, baseline).await;
    if matches!(settled, Ok(SeatLifecycle::Running)) {
        tokio::spawn(crate::handoffs::deliver_owed(
            st.clone(),
            seat.to_string(),
            restarting,
            owed,
        ));
        return Ok(());
    }
    let reason = match settled {
        Ok(_) => st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .seat(seat)?
            .and_then(|v| v.lifecycle_reason)
            .unwrap_or_else(|| "unknown".into()),
        Err(e) => e.message,
    };
    let body = EventBody::RestoreNeeded {
        reason: format!("Restoring did not work: {reason}"),
        can_resume: false,
    };
    st.local_event(
        crate::hosts::controld(),
        Subject::Seat(seat.to_string()),
        Provenance::ProtocolObserved,
        body,
    )?;
    Err(ApiError::bad_gateway("restore_failed", reason))
}

/// The session's usage so far: every report since the tenure that opened it (resumes continue it), so
/// the first turn after the resume does not count the whole session again.
pub(crate) fn usage_baseline(st: &AppState, seat: &str) -> Result<[u64; 4], ApiError> {
    let subject = Subject::Seat(seat.to_string());
    let events = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .events_of_types(&subject, 0, &["tenure-started", "usage-reported"])?;
    let mut b = [0u64; 4];
    for e in events {
        match e.body {
            EventBody::TenureStarted { cause, .. } if cause != TenureCause::Resume => b = [0; 4],
            EventBody::UsageReported {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                ..
            } => {
                b = [
                    b[0] + input_tokens,
                    b[1] + output_tokens,
                    b[2] + cache_read_tokens,
                    b[3] + cache_write_tokens,
                ];
            }
            _ => {}
        }
    }
    Ok(b)
}
