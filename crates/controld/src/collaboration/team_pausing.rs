//! Pause all and Resume all: the operator's pause and resume for every agent of a team at once, under
//! one claim on the team (modelled on stop-now).
use super::pausing::{
    PAUSES_WITH_PARENT, SkipCode, TeamControlResult, begin_pause, claim, control_of,
    failure_reason, is_paused, pause_refusal, paused_descendants, resume_seat,
};
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::state::AppState;
use crate::store::ControlMode;
use crate::views::SeatView;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use serde::{Deserialize, Serialize};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/pause", super::post(pause_team))
        .route("/api/teams/{id}/resume", super::post(resume_team))
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

type Skip = (SkipCode, String);

/// Decides, for each agent of a team, whether Pause all asks it to pause (None) or why not.
struct PausePlan<'a> {
    seats: &'a [TeamSeat],
    /// Its own reason not to pause, ignoring the agent that started it.
    own: Vec<Option<Skip>>,
    /// Its own pause is under way.
    pausing: Vec<bool>,
}

impl<'a> PausePlan<'a> {
    fn new(st: &AppState, seats: &'a [TeamSeat]) -> Result<Self, ApiError> {
        let mut own = Vec::with_capacity(seats.len());
        let mut pausing = Vec::with_capacity(seats.len());
        for s in seats {
            own.push(pause_refusal(st, &s.view)?);
            pausing.push(control_of(st, &s.view.id)? == Some(ControlMode::Pausing));
        }
        Ok(Self {
            seats,
            own,
            pausing,
        })
    }

    /// An agent started by an agent that pauses pauses with it (that agent's pause stops it); one
    /// started by an agent stopping another way stops with it.
    fn skip(&self, i: usize) -> Option<Skip> {
        if let Some(own) = &self.own[i] {
            return Some(own.clone());
        }
        let parent = self.seats[i].parent.as_deref()?;
        let p = self.seats.iter().position(|s| s.view.id == parent)?;
        let with_parent = (SkipCode::StartedByAgent, PAUSES_WITH_PARENT.to_string());
        if self.pausing[p] {
            return Some(with_parent);
        }
        match self.skip(p) {
            None | Some((SkipCode::StartedByAgent, _)) => Some(with_parent),
            Some((SkipCode::AlreadyStopping, _)) => Some((
                SkipCode::AlreadyStopping,
                "It stops with the agent that started it".into(),
            )),
            Some(_) => None,
        }
    }
}

/// Pauses every running agent of team `rig` that no other stop or pause covers.
async fn pause_all(st: &AppState, rig: &str) -> Result<TeamControlResult, ApiError> {
    let mut out = TeamControlResult::default();
    let Some(seats) = team_seats(st, rig)? else {
        return Ok(out);
    };
    let _claim = claim(st, rig)?;
    let plan = PausePlan::new(st, &seats)?;
    for (i, seat) in seats.iter().enumerate() {
        let id = &seat.view.id;
        if let Some((code, why)) = plan.skip(i) {
            out.skip(id, code, why);
            continue;
        }
        match begin_pause(st, &seat.view).await {
            Ok(()) => out.affected.push(id.clone()),
            Err(e) => out.skip(id, SkipCode::Failed, failure_reason(&e)),
        }
    }
    Ok(out)
}

/// The paused agents of a team that Resume all asks to resume, decided before any resumes; the others
/// are noted in `out`. One that paused with the agent that started it resumes with that agent.
fn resume_roots<'a>(
    st: &AppState,
    seats: &'a [TeamSeat],
    out: &mut TeamControlResult,
) -> Result<Vec<&'a SeatView>, ApiError> {
    let mut roots = Vec::new();
    for TeamSeat { view, parent } in seats {
        if !is_paused(st, &view.id)? {
            out.skip(&view.id, SkipCode::NotPaused, "It is not paused");
            continue;
        }
        let with_parent = control_of(st, &view.id)? == Some(ControlMode::Paused)
            && parent.as_deref().map_or(Ok(false), |p| is_paused(st, p))?;
        if !with_parent {
            roots.push(view);
        }
    }
    Ok(roots)
}

/// Resumes every paused agent of team `rig`; one that paused with the agent that started it resumes
/// with that agent.
async fn resume_all(st: &AppState, rig: &str) -> Result<TeamControlResult, ApiError> {
    let mut out = TeamControlResult::default();
    let Some(seats) = team_seats(st, rig)? else {
        return Ok(out);
    };
    let _claim = claim(st, rig)?;
    for view in resume_roots(st, &seats, &mut out)? {
        match resume_seat(st, view).await {
            Ok(done) => {
                out.affected.extend(done.affected);
                out.skipped.extend(done.skipped);
            }
            Err(e) => {
                out.skip(&view.id, SkipCode::Failed, failure_reason(&e));
                for child in paused_descendants(st, &view.id)? {
                    let why = "It resumes with the agent that started it";
                    out.skip(&child.id, SkipCode::StartedByAgent, why);
                }
            }
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
