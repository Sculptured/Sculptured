//! The operator's pause and resume, for one agent or every agent of a team. A pause asks the agent to
//! stop at its next safe point (a checkpoint note, as a pausing budget asks), then stops it and keeps
//! its session; Resume continues that same session, or starts fresh from the note when there is none.
use super::controls::{note_path, record, send_wrap_up};
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::state::{AppState, BootGuard};
use crate::store::ControlMode;
use crate::views::SeatView;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use model::{Actor, ContinueMode, Desired, MessageIntent, SeatLifecycle};
use serde::{Deserialize, Serialize};

/// What a team-wide pause or resume did: the agents it acted on, and every other agent with why not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TeamControlResult {
    pub affected: Vec<String>,
    pub skipped: Vec<SkippedSeat>,
}

/// An agent a team-wide pause or resume left alone, and why (plain words).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct SkippedSeat {
    pub seat: String,
    pub reason: String,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/pause", super::post(pause_team))
        .route("/api/teams/{id}/resume", super::post(resume_team))
}

/// True while `seat` is pausing or paused by the operator.
pub(super) fn is_paused(st: &AppState, seat: &str) -> Result<bool, ApiError> {
    let mode = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .control(seat)?;
    Ok(matches!(
        mode,
        Some(ControlMode::Pausing | ControlMode::Paused)
    ))
}

/// Pauses one agent; the caller holds the team's claim. Pausing an agent already paused does nothing.
pub(super) async fn pause_one(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    if is_paused(st, &view.id)? {
        return Ok(());
    }
    if view.lifecycle != SeatLifecycle::Running {
        return Err(ApiError::conflict(
            "agent_not_running",
            "This agent is not running, so there is nothing to pause.",
        ));
    }
    begin_pause(st, view).await
}

/// Resumes one paused agent under the team's claim.
pub(super) async fn resume_one(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    let _claim = claim(st, &view.rig)?;
    continue_seat(st, view).await
}

async fn begin_pause(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    record(st, &view.id, ControlMode::Pausing)?;
    send_wrap_up(st, view, pause_text(&view.id)).await
}

/// Continues a paused agent: a pause still wrapping up is called off, and a stopped agent starts again
/// in its own session.
async fn continue_seat(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    let exhausted = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .budget_exhausted(&view.id)?;
    if exhausted {
        return Err(ApiError::conflict(
            "budget_exhausted",
            "This agent reached its output budget. Start a new project run to reset it.",
        ));
    }
    match view.lifecycle {
        SeatLifecycle::Running => call_off_pause(st, &view.id).await,
        SeatLifecycle::Stopped | SeatLifecycle::Attention | SeatLifecycle::Failed => {
            restart_paused(st, view).await
        }
        _ => Err(ApiError::conflict(
            "agent_settling",
            "This agent is still stopping or starting. Resume it once it has settled.",
        )),
    }
}

/// A pause still wrapping up: the agent is not stopped, and goes on with its job.
async fn call_off_pause(st: &AppState, seat: &str) -> Result<(), ApiError> {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .clear_control(seat)?;
    let text = format!("Pause called off by You. {CONTINUE}");
    crate::reconciler::send_message_as(st, seat, text, MessageIntent::Instruct, false, operator())
        .await
        .map(|_| ())
}

/// A paused agent that stopped starts again, resuming its session (fresh from its note without one).
async fn restart_paused(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    let desired = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(&view.rig)?
        .map(|s| s.desired);
    if desired != Some(Desired::Running) {
        return Err(ApiError::conflict(
            "team_not_running",
            "This team is stopped. Start the team to run its agents.",
        ));
    }
    let text = format!(
        "Resumed by You. {CONTINUE} If you no longer have that context, read your note {} in your folder first.",
        note_path(&view.id)
    );
    crate::budget::pause::restart(st, &view.id, ContinueMode::Resume, text, operator()).await
}

const CONTINUE: &str = "Continue your job where you stopped.";

fn pause_text(seat: &str) -> String {
    format!(
        "Paused by You. Stop at a safe checkpoint within approximately 5,000 additional output tokens. Do not begin new research, broad refactors, new tasks or child agents. Save your work, then write {} in your folder: what is done (exact artifact paths and revisions), checks actually run, what is next, and anything you would need to know to continue with nothing else in context. Keep it short. End your turn. The runner then stops this session and keeps it; when You resume, you continue in this same conversation.",
        note_path(seat)
    )
}

fn operator() -> Actor {
    Actor::Human("local".into())
}

fn claim(st: &AppState, rig: &str) -> Result<BootGuard, ApiError> {
    st.try_begin_boot(rig).ok_or_else(|| {
        ApiError::conflict(
            "team_starting",
            "Project is changing. Retry after that operation finishes.",
        )
    })
}

/// An agent of a team, and the agent that started it, if any.
struct TeamSeat {
    view: SeatView,
    parent: Option<String>,
}

/// The team's agents (spawned ones included), or None when it never started. An unknown team is not
/// found.
fn team_seats(st: &AppState, rig: &str) -> Result<Option<Vec<TeamSeat>>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let Some(snap) = store.rig_snapshot(rig)? else {
        return match store.draft(rig)? {
            Some(_) => Ok(None),
            None => Err(ApiError::not_found(format!("no team {rig:?}"))),
        };
    };
    let mut out = Vec::with_capacity(snap.seats.len());
    for view in snap.seats {
        let parent = store.parent(&view.id)?;
        out.push(TeamSeat { view, parent });
    }
    Ok(Some(out))
}

fn skip(out: &mut TeamControlResult, seat: &str, reason: impl Into<String>) {
    out.skipped.push(SkippedSeat {
        seat: seat.to_string(),
        reason: reason.into(),
    });
}

fn lifecycle_word(l: SeatLifecycle) -> String {
    format!("{l:?}").to_lowercase()
}

/// Why a team-wide pause leaves `seat` alone, or None to pause it. An agent started by a running agent
/// is stopped with it (with every agent it started), so it is not asked separately.
fn pause_skip(
    st: &AppState,
    seat: &TeamSeat,
    running: &[&str],
) -> Result<Option<String>, ApiError> {
    let view = &seat.view;
    Ok(if is_paused(st, &view.id)? {
        Some("Already paused".into())
    } else if view.lifecycle != SeatLifecycle::Running {
        Some(format!(
            "It is not running ({})",
            lifecycle_word(view.lifecycle)
        ))
    } else if seat.parent.as_deref().is_some_and(|p| running.contains(&p)) {
        Some("It pauses with the agent that started it".into())
    } else {
        None
    })
}

/// Pauses every running agent of team `rig` under one claim.
async fn pause_all(st: &AppState, rig: &str) -> Result<TeamControlResult, ApiError> {
    let mut out = TeamControlResult::default();
    let Some(seats) = team_seats(st, rig)? else {
        return Ok(out);
    };
    let _claim = claim(st, rig)?;
    let running: Vec<&str> = seats
        .iter()
        .filter(|s| s.view.lifecycle == SeatLifecycle::Running)
        .map(|s| s.view.id.as_str())
        .collect();
    for seat in &seats {
        let id = &seat.view.id;
        if let Some(why) = pause_skip(st, seat, &running)? {
            skip(&mut out, id, why);
            continue;
        }
        match begin_pause(st, &seat.view).await {
            Ok(()) => out.affected.push(id.clone()),
            Err(e) => skip(&mut out, id, e.message),
        }
    }
    Ok(out)
}

/// Resumes every paused agent of team `rig` under one claim.
async fn resume_all(st: &AppState, rig: &str) -> Result<TeamControlResult, ApiError> {
    let mut out = TeamControlResult::default();
    let Some(seats) = team_seats(st, rig)? else {
        return Ok(out);
    };
    let _claim = claim(st, rig)?;
    for TeamSeat { view, .. } in &seats {
        if !is_paused(st, &view.id)? {
            skip(&mut out, &view.id, "It is not paused");
            continue;
        }
        match continue_seat(st, view).await {
            Ok(()) => out.affected.push(view.id.clone()),
            Err(e) => skip(&mut out, &view.id, e.message),
        }
    }
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct TeamControlRequest {
    request_id: String,
}

/// Pause all: every running agent of the team stops at its next safe point and keeps its session.
async fn pause_team(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<TeamControlRequest>,
) -> Response {
    let fp = super::fingerprint(&format!("pause:{id}"), &req);
    let st2 = st.clone();
    super::idempotent(&st, &req.request_id, fp, move || async move {
        let out = pause_all(&st2, &id).await?;
        Ok(serde_json::to_value(out).expect("a team control result serializes"))
    })
    .await
}

/// Resume all: every paused agent of the team continues its session.
async fn resume_team(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<TeamControlRequest>,
) -> Response {
    let fp = super::fingerprint(&format!("resume:{id}"), &req);
    let st2 = st.clone();
    super::idempotent(&st, &req.request_id, fp, move || async move {
        let out = resume_all(&st2, &id).await?;
        Ok(serde_json::to_value(out).expect("a team control result serializes"))
    })
    .await
}
