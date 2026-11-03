//! The operator's resume of a paused agent: a pause still wrapping up is called off, and a stopped
//! agent starts again in its own session (fresh from its note without one), with the agents it
//! started that paused with it.
use super::controls::{note_path, record, refuse_if_budget_exhausted};
use super::pausing::{SeatNote, SkipCode, TeamControlResult, failure_reason};
use crate::error::ApiError;
use crate::state::AppState;
use crate::store::ControlMode;
use crate::views::SeatView;
use model::{Actor, ContinueMode, Desired, MessageIntent, SeatLifecycle};

/// It runs again, but was not told to continue.
const NOT_TOLD: &str = "It runs again, but the message asking it to continue could not be sent. Send it a message to continue.";

/// Resumes one paused agent, then the agents it started that paused with it (parents first).
pub(super) async fn resume_seat(
    st: &AppState,
    view: &SeatView,
) -> Result<TeamControlResult, ApiError> {
    let mut out = TeamControlResult::default();
    let told = continue_seat(st, view).await?;
    resumed(&mut out, &view.id, told);
    for child in paused_descendants(st, &view.id)? {
        match continue_seat(st, &child).await {
            Ok(told) => resumed(&mut out, &child.id, told),
            Err(e) => out.skip(&child.id, SkipCode::Failed, failure_reason(&e)),
        }
    }
    Ok(out)
}

fn resumed(out: &mut TeamControlResult, seat: &str, told: bool) {
    out.affected.push(seat.to_string());
    if !told {
        out.notes.push(SeatNote {
            seat: seat.to_string(),
            reason: NOT_TOLD.into(),
        });
    }
}

/// The agents `seat` started (recursively) that paused with it, parents first. The walk stops at an
/// agent that is not paused: what that one started did not pause with `seat`.
pub(super) fn paused_descendants(st: &AppState, seat: &str) -> Result<Vec<SeatView>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let mut out: Vec<SeatView> = Vec::new();
    let mut parents = vec![seat.to_string()];
    let mut i = 0;
    while i < parents.len() {
        for id in store.children(&parents[i])? {
            if store.control(&id)? == Some(ControlMode::Paused)
                && let Some(v) = store.seat(&id)?
            {
                parents.push(id);
                out.push(v);
            }
        }
        i += 1;
    }
    Ok(out)
}

/// Continues a paused agent; Ok(false) when it continues but could not be told to.
async fn continue_seat(st: &AppState, view: &SeatView) -> Result<bool, ApiError> {
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
async fn call_off_pause(st: &AppState, seat: &str) -> Result<bool, ApiError> {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .clear_control(seat)?;
    let text = format!("Pause called off by You. {CONTINUE}");
    Ok(tell(st, seat, text).await)
}

/// A paused agent that stopped starts again, resuming its session (fresh from its note without one).
///
/// The relaunch clears the pause first (what the agent is owed is held back while a stop is
/// recorded). If it does not come up, the agent is left paused, whatever its state now: the next
/// Resume retries, and if it comes up after all, that Resume calls the pause off.
async fn restart_paused(st: &AppState, view: &SeatView) -> Result<bool, ApiError> {
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
    if let Err(e) = crate::budget::pause::relaunch(st, &view.id, ContinueMode::Resume).await {
        record(st, &view.id, ControlMode::Paused)?;
        return Err(e);
    }
    let text = format!(
        "Resumed by You. {CONTINUE} If you no longer have that context, read your note {} in your folder first.",
        note_path(&view.id)
    );
    Ok(tell(st, &view.id, text).await)
}

/// Sends the operator's instruction; false when it could not be sent.
async fn tell(st: &AppState, seat: &str, text: String) -> bool {
    let operator = Actor::Human("local".into());
    let sent = crate::reconciler::send_message_as(
        st,
        seat,
        text,
        MessageIntent::Instruct,
        false,
        operator,
    )
    .await;
    if let Err(e) = &sent {
        tracing::warn!(%seat, error = %e.message, "a resumed agent could not be told to continue");
    }
    sent.is_ok()
}

const CONTINUE: &str = "Continue your job where you stopped.";
