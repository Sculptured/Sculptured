//! The operator's pause and resume of one agent. A pause asks the agent to stop at its next safe point
//! (a checkpoint note, as a pausing budget asks), then stops it, and every agent it started, keeping
//! their sessions; Resume continues those same sessions, or starts fresh from the note without one.
use super::controls::{Control, note_path, record, refuse_if_budget_exhausted, send_wrap_up};
use crate::error::ApiError;
use crate::state::{AppState, BootGuard};
use crate::store::ControlMode;
use crate::views::SeatView;
use model::{Actor, ContinueMode, Desired, MessageIntent, SeatLifecycle};
use serde::{Deserialize, Serialize};

/// What a pause or resume did: the agents it acted on, and every other agent with why not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TeamControlResult {
    pub affected: Vec<String>,
    pub skipped: Vec<SkippedSeat>,
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

/// A pause, or the resume of a paused agent, under the team's claim; None for any other control.
pub(super) async fn pause_or_resume(
    st: &AppState,
    view: &SeatView,
    control: Control,
) -> Result<Option<TeamControlResult>, ApiError> {
    let pause = control == Control::Pause;
    if !pause && !(control == Control::Resume && is_paused(st, &view.id)?) {
        return Ok(None);
    }
    let _claim = claim(st, &view.rig)?;
    if pause {
        pause_seat(st, view).await.map(Some)
    } else {
        resume_seat(st, view).await.map(Some)
    }
}

/// Pauses one agent; the agents it started are listed as pausing with it.
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
            for child in live_descendants(st, &view.id)? {
                out.skip(&child, SkipCode::StartedByAgent, PAUSES_WITH_PARENT);
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

/// The agents `seat` started (recursively) that may still have a process, parents first.
fn live_descendants(st: &AppState, seat: &str) -> Result<Vec<String>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let mut out = Vec::new();
    for id in super::token_limits::with_descendants(&store, seat)?
        .into_iter()
        .skip(1)
    {
        let live = store
            .seat(&id)?
            .is_some_and(|v| crate::reconciler::stoppable(v.lifecycle));
        if live {
            out.push(id);
        }
    }
    Ok(out)
}

/// The end of a pause's wrap-up (the watcher's turn): the agent stops with every agent it started,
/// and each one that had no other stop under way is paused with its session kept. Called off
/// meanwhile, nothing happens; a team that is changing, or a stop that failed, is retried next tick.
pub(super) async fn finish_pause(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    let Some(_claim) = st.try_begin_boot(&view.rig) else {
        return Ok(());
    };
    if control_of(st, &view.id)? != Some(ControlMode::Pausing) {
        return Ok(());
    }
    let children = live_descendants(st, &view.id)?;
    let mut pausing = Vec::new();
    for child in &children {
        if control_of(st, child)?.is_none() {
            record(st, child, ControlMode::Paused)?;
            pausing.push(child.as_str());
        }
    }
    // The agents it started first (the latest first), then the agent itself.
    let ids: Vec<&str> = children
        .iter()
        .rev()
        .map(String::as_str)
        .chain(std::iter::once(view.id.as_str()))
        .collect();
    if stop_for_pause(st, &ids, &pausing).await? {
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .set_control_mode(&view.id, ControlMode::Paused)?;
    }
    Ok(())
}

/// Stops `ids` at once; true when none is left running. A child (in `pausing`) that could not be
/// stopped is not recorded as paused.
async fn stop_for_pause(st: &AppState, ids: &[&str], pausing: &[&str]) -> Result<bool, ApiError> {
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
        if pausing.contains(id) {
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

/// Resumes one paused agent, then the agents it started that paused with it (parents first).
pub(super) async fn resume_seat(
    st: &AppState,
    view: &SeatView,
) -> Result<TeamControlResult, ApiError> {
    continue_seat(st, view).await?;
    let mut out = TeamControlResult {
        affected: vec![view.id.clone()],
        ..Default::default()
    };
    for child in paused_descendants(st, &view.id)? {
        match continue_seat(st, &child).await {
            Ok(()) => out.affected.push(child.id),
            Err(e) => out.skip(&child.id, SkipCode::Failed, failure_reason(&e)),
        }
    }
    Ok(out)
}

/// The agents `seat` started (recursively) that are paused, parents first.
pub(super) fn paused_descendants(st: &AppState, seat: &str) -> Result<Vec<SeatView>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let mut out = Vec::new();
    for id in super::token_limits::with_descendants(&store, seat)?
        .into_iter()
        .skip(1)
    {
        if store.control(&id)? == Some(ControlMode::Paused)
            && let Some(v) = store.seat(&id)?
        {
            out.push(v);
        }
    }
    Ok(out)
}

/// Continues a paused agent: a pause still wrapping up is called off, and a stopped agent starts again
/// in its own session.
async fn continue_seat(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    refuse_if_budget_exhausted(st, &view.id)?;
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
/// If it cannot start, it stays paused.
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
    // The restart clears the pause first: what the agent is owed is held back while a stop is recorded.
    let restarted =
        crate::budget::pause::restart(st, &view.id, ContinueMode::Resume, text, operator()).await;
    let running = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(&view.id)?
        .is_some_and(|v| crate::store::is_live(v.lifecycle));
    if restarted.is_err() && !running {
        record(st, &view.id, ControlMode::Paused)?;
    }
    restarted
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
