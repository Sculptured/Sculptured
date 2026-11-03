//! The operator's pause and resume of one agent. A pause asks the agent to stop at its next safe point
//! (a checkpoint note, as a pausing budget asks), then stops it, and every agent it started, keeping
//! their sessions; Resume continues those same sessions, or starts fresh from the note without one.
use super::controls::{Control, note_path, record, send_wrap_up};
use super::resuming::resume_seat;
use crate::error::ApiError;
use crate::state::{AppState, BootGuard};
use crate::store::ControlMode;
use crate::views::SeatView;
use model::SeatLifecycle;
use serde::{Deserialize, Serialize};

/// What a pause or resume did: the agents it acted on, and every other agent with why not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TeamControlResult {
    pub affected: Vec<String>,
    pub skipped: Vec<SkippedSeat>,
    /// Something to know about an agent it did act on (plain words).
    pub notes: Vec<SeatNote>,
}

/// Something to know about an agent a pause or resume acted on, in plain words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct SeatNote {
    pub seat: String,
    pub reason: String,
}

/// An agent a pause or resume left alone: why, as a stable code and in plain words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct SkippedSeat {
    pub seat: String,
    pub code: SkipCode,
    pub reason: String,
}

/// Why a pause or resume left an agent alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SkipCode {
    NotRunning,
    AlreadyPaused,
    /// Stopping another way (a soft or hard stop, a budget's wrap-up).
    AlreadyStopping,
    NotPaused,
    /// Started by another agent: it pauses and resumes with that agent.
    StartedByAgent,
    Failed,
}

impl TeamControlResult {
    pub(super) fn skip(&mut self, seat: &str, code: SkipCode, reason: impl Into<String>) {
        self.skipped.push(SkippedSeat {
            seat: seat.to_string(),
            code,
            reason: reason.into(),
        });
    }
}

pub(super) const STOPS_WITH_PARENT: &str = "It stops with the agent that started it";

pub(super) const PAUSES_WITH_PARENT: &str =
    "It pauses with the agent that started it, and resumes with it";

pub(super) fn control_of(st: &AppState, seat: &str) -> Result<Option<ControlMode>, ApiError> {
    Ok(st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .control(seat)?)
}

/// True while `seat` is pausing or paused by the operator.
pub(super) fn is_paused(st: &AppState, seat: &str) -> Result<bool, ApiError> {
    Ok(matches!(
        control_of(st, seat)?,
        Some(ControlMode::Pausing | ControlMode::Paused)
    ))
}

/// A lifecycle in plain words.
pub(super) fn lifecycle_words(l: SeatLifecycle) -> &'static str {
    match l {
        SeatLifecycle::Planned => "not started yet",
        SeatLifecycle::Launching => "starting",
        SeatLifecycle::Running => "running",
        SeatLifecycle::Stopping => "stopping",
        SeatLifecycle::Stopped => "stopped",
        SeatLifecycle::Stale => "out of touch",
        SeatLifecycle::Attention => "needs attention",
        SeatLifecycle::Failed => "failed",
    }
}

/// Why `view` cannot be paused now (never over another stop), or None when it can.
pub(super) fn pause_refusal(
    st: &AppState,
    view: &SeatView,
) -> Result<Option<(SkipCode, String)>, ApiError> {
    Ok(match control_of(st, &view.id)? {
        Some(ControlMode::Pausing | ControlMode::Paused) => {
            Some((SkipCode::AlreadyPaused, "Already paused".into()))
        }
        _ if view.lifecycle != SeatLifecycle::Running => Some((
            SkipCode::NotRunning,
            format!("It is not running ({})", lifecycle_words(view.lifecycle)),
        )),
        Some(_) => Some((SkipCode::AlreadyStopping, "It is already stopping".into())),
        None => None,
    })
}

/// A plain reason for a pause or resume that failed (never the server's own text).
pub(super) fn failure_reason(e: &ApiError) -> &'static str {
    match &*e.code {
        "budget_exhausted" => "It reached its output budget",
        "team_not_running" => "The team is stopped. Start it to run its agents",
        "agent_settling" => "It is still stopping or starting. Try again in a moment",
        _ => "It could not be done. Try again; its reason is shown on the agent",
    }
}

pub(super) fn claim(st: &AppState, rig: &str) -> Result<BootGuard, ApiError> {
    st.try_begin_boot(rig).ok_or_else(|| {
        ApiError::conflict(
            "team_starting",
            "Project is changing. Retry after that operation finishes.",
        )
    })
}

/// A pause, or the resume of a paused agent, under the team's claim; None for any other control. The
/// agent is read again once the claim is held.
pub(super) async fn pause_or_resume(
    st: &AppState,
    seen: &SeatView,
    control: Control,
) -> Result<Option<TeamControlResult>, ApiError> {
    let pause = control == Control::Pause;
    if !pause && !(control == Control::Resume && is_paused(st, &seen.id)?) {
        return Ok(None);
    }
    let _claim = claim(st, &seen.rig)?;
    let view = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(&seen.id)?
        .ok_or_else(|| ApiError::not_found("Unknown agent"))?;
    if pause {
        return pause_seat(st, &view).await.map(Some);
    }
    if !is_paused(st, &view.id)? {
        // Resumed by someone else meanwhile.
        let mut out = TeamControlResult::default();
        out.skip(&view.id, SkipCode::NotPaused, "It is not paused");
        return Ok(Some(out));
    }
    resume_seat(st, &view).await.map(Some)
}

/// Pauses one agent; the agents it started are listed with what its pause does to them.
async fn pause_seat(st: &AppState, view: &SeatView) -> Result<TeamControlResult, ApiError> {
    let mut out = TeamControlResult::default();
    match pause_refusal(st, view)? {
        Some((SkipCode::AlreadyPaused, why)) => out.skip(&view.id, SkipCode::AlreadyPaused, why),
        Some((SkipCode::NotRunning, _)) => {
            return Err(ApiError::conflict(
                "agent_not_running",
                "This agent is not running, so there is nothing to pause.",
            ));
        }
        Some(_) => {
            return Err(ApiError::conflict(
                "agent_stopping",
                "This agent is already stopping, so it cannot be paused.",
            ));
        }
        None => {
            begin_pause(st, view).await?;
            out.affected.push(view.id.clone());
            for d in descendants_of_pause(st, &view.id)? {
                if d.live {
                    let (code, why) = d.skip();
                    out.skip(&d.id, code, why);
                }
            }
        }
    }
    Ok(out)
}

/// Asks a running agent with no other stop in progress to wrap up for a pause. Nothing is recorded
/// when the request cannot be sent.
pub(super) async fn begin_pause(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    record(st, &view.id, ControlMode::Pausing)?;
    let sent = send_wrap_up(st, view, pause_text(&view.id)).await;
    if sent.is_err() {
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .clear_control(&view.id)?;
    }
    sent
}

/// An agent started (recursively) by an agent that pauses.
struct Descendant {
    id: String,
    /// It may still have a process.
    live: bool,
    /// Its own control row, if any.
    own: Option<ControlMode>,
    /// It pauses with the agent: neither it nor an agent between them has another stop under way.
    pauses: bool,
}

impl Descendant {
    /// What the pause does to it, as a team pause would say.
    fn skip(&self) -> (SkipCode, &'static str) {
        match self.own {
            Some(ControlMode::Pausing | ControlMode::Paused) => {
                (SkipCode::AlreadyPaused, "Already paused")
            }
            Some(_) => (SkipCode::AlreadyStopping, "It is already stopping"),
            None if self.pauses => (SkipCode::StartedByAgent, PAUSES_WITH_PARENT),
            None => (SkipCode::AlreadyStopping, STOPS_WITH_PARENT),
        }
    }
}

/// The agents `seat` started (recursively), parents first, with what its pause does to each.
fn descendants_of_pause(st: &AppState, seat: &str) -> Result<Vec<Descendant>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let mut out: Vec<Descendant> = Vec::new();
    let mut parents = vec![(seat.to_string(), true)];
    let mut i = 0;
    while i < parents.len() {
        let (parent, parent_pauses) = parents[i].clone();
        for id in store.children(&parent)? {
            let live = store
                .seat(&id)?
                .is_some_and(|v| crate::reconciler::stoppable(v.lifecycle));
            let own = store.control(&id)?;
            let pauses = parent_pauses && own.is_none();
            parents.push((id.clone(), pauses));
            out.push(Descendant {
                id,
                live,
                own,
                pauses,
            });
        }
        i += 1;
    }
    Ok(out)
}

/// The end of a pause's wrap-up (the watcher's turn): the agent stops with every agent it started.
/// Each one with no other stop under way is paused with its session kept, or, below an agent stopping
/// another way, stopped as that stop would. Called off meanwhile, nothing happens; a team that is
/// changing, or a stop that failed, is retried next tick.
pub(super) async fn finish_pause(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    let Some(_claim) = st.try_begin_boot(&view.rig) else {
        return Ok(());
    };
    if control_of(st, &view.id)? != Some(ControlMode::Pausing) {
        return Ok(());
    }
    let live: Vec<Descendant> = descendants_of_pause(st, &view.id)?
        .into_iter()
        .filter(|d| d.live)
        .collect();
    let recorded = record_descendants(st, &live)?;
    // The agents it started first (the latest first), then the agent itself.
    let ids: Vec<&str> = live
        .iter()
        .rev()
        .map(|d| d.id.as_str())
        .chain(std::iter::once(view.id.as_str()))
        .collect();
    if stop_for_pause(st, &ids, &recorded).await? {
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .set_control_mode(&view.id, ControlMode::Paused)?;
    }
    Ok(())
}

/// Records what a pause does to each live descendant without a stop of its own: paused with it, or
/// (below an agent stopping another way) stopped as that stop would. Returns the ones recorded.
fn record_descendants<'a>(st: &AppState, live: &'a [Descendant]) -> Result<Vec<&'a str>, ApiError> {
    let mut recorded = Vec::new();
    for d in live.iter().filter(|d| d.own.is_none()) {
        let mode = if d.pauses {
            ControlMode::Paused
        } else {
            ControlMode::Hard
        };
        record(st, &d.id, mode)?;
        recorded.push(d.id.as_str());
    }
    Ok(recorded)
}

/// Stops `ids` at once; true when none is left running. One whose row this pause `recorded` and that
/// could not be stopped has that row removed (the next tick tries again).
async fn stop_for_pause(st: &AppState, ids: &[&str], recorded: &[&str]) -> Result<bool, ApiError> {
    let stops = ids
        .iter()
        .map(|id| crate::reconciler::stop_seat_now(st, id));
    let mut stopped_all = true;
    for (id, r) in ids.iter().zip(futures::future::join_all(stops).await) {
        let Err(e) = r else { continue };
        if !still_live(st, id)? {
            continue;
        }
        tracing::warn!(seat = %id, error = %e.message, "a pause could not stop an agent yet");
        stopped_all = false;
        if recorded.contains(id) {
            st.store
                .lock()
                .expect("store mutex must not be poisoned")
                .clear_control(id)?;
        }
    }
    Ok(stopped_all)
}

fn still_live(st: &AppState, seat: &str) -> Result<bool, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    Ok(store
        .seat(seat)?
        .is_some_and(|v| crate::reconciler::stoppable(v.lifecycle)))
}

fn pause_text(seat: &str) -> String {
    format!(
        "Paused by You. Stop at a safe checkpoint within approximately 5,000 additional output tokens. Do not begin new research, broad refactors, new tasks or child agents. Save your work, then write {} in your folder: what is done (exact artifact paths and revisions), checks actually run, what is next, and anything you would need to know to continue with nothing else in context. Keep it short. End your turn. The runner then stops this session and keeps it; when You resume, you continue in this same conversation.",
        note_path(seat)
    )
}
