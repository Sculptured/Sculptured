//! Safety stops preempt team actions. An emergency stop, an agent's hard stop or a limit's stop never
//! waits for a pause, a resume or a budget continuation: it marks the team (or the agents) stopped
//! (`AppState::mark_stopped`), records each agent's hard stop and stops them, with or without the
//! team's claim. The action holding the claim checks before each agent it would start, continue or
//! pause, and once an agent it started is up: an agent a stop covered since the action was asked for
//! is left alone (or stopped again), and the action goes on with the rest.
use crate::error::ApiError;
use crate::state::AppState;
use crate::store::ControlMode;

/// Why a team action left an agent alone: a safety stop came while it ran.
pub(crate) fn stopped_meanwhile() -> ApiError {
    ApiError::conflict(
        "stopped_meanwhile",
        "This agent was stopped while that was under way, so it stays stopped.",
    )
}

/// Why the team's claim is refused, in plain words: another team action, or a start (or another
/// change of the team).
pub(crate) fn busy(st: &AppState, rig: &str) -> ApiError {
    if st.team_action_running(rig) {
        return ApiError::conflict(
            "team_busy",
            "Another team action is running (pause, resume, or continuing after a budget). Try again in a moment.",
        );
    }
    ApiError::conflict(
        "team_starting",
        "This team is still starting or changing. Try again once it has finished.",
    )
}

/// The team action holding `seat`'s team claim must leave it alone: a safety stop covered it.
pub(crate) fn preempted(st: &AppState, seat: &str) -> bool {
    st.stopped_during_action(crate::seat_ids::rig_or_empty(seat), seat)
}

/// Clears `seat`'s control row, unless a safety stop covered it since the team action holding its
/// claim was asked for (then the stop's row stays and this refuses). One step under the store lock,
/// so a stop recorded right after is never cleared.
pub(crate) fn clear_unless_stopped(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    if preempted(st, seat) {
        return Err(stopped_meanwhile());
    }
    store.clear_control(seat)?;
    Ok(())
}

/// Records `mode` for `seat` (as `controls::record`), unless a safety stop covered it meanwhile.
pub(crate) fn record_unless_stopped(
    st: &AppState,
    seat: &str,
    mode: ControlMode,
) -> Result<(), ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    if preempted(st, seat) {
        return Err(stopped_meanwhile());
    }
    let baseline = store.seat(seat)?.and_then(|s| s.tokens_out).unwrap_or(0);
    store.record_control(seat, mode, baseline)?;
    Ok(())
}

/// After a team action started `seat` again: if a safety stop covered it meanwhile (the stop may have
/// reached it before it was up), it is stopped now and this refuses.
pub(crate) async fn stop_if_stopped(st: &AppState, seat: &str) -> Result<(), ApiError> {
    if !preempted(st, seat) {
        return Ok(());
    }
    super::controls::record(st, seat, ControlMode::Hard)?;
    if let Err(e) = crate::reconciler::stop_seat_now(st, seat).await {
        tracing::warn!(%seat, error = %e.message, "an agent started during a stop could not be stopped yet");
    }
    Err(stopped_meanwhile())
}
