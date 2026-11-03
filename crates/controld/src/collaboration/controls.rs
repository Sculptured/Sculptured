use super::TeamControlResult;
use crate::error::ApiError;
use crate::reconciler;
use crate::state::{AppState, BootGuard};
use crate::store::ControlMode;
use crate::views::SeatView;
use model::{EventBody, MessageIntent, SeatLifecycle};

#[cfg(test)]
#[path = "controls_tests.rs"]
mod tests;

/// What the operator (or a limit) asks of a running agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Wrap up the current work, then stop.
    Soft,
    /// Wrap up at a checkpoint the agent can continue from: the run pauses until its plan window resets.
    Checkpoint,
    /// The operator's pause: wrap up at a checkpoint, then stop and keep the session for Resume.
    Pause,
    /// Stop now, with every agent it spawned.
    Hard,
    /// Undo a stop.
    Resume,
}

impl Control {
    pub fn parse(s: &str) -> Result<Self, ApiError> {
        match s {
            "soft" => Ok(Self::Soft),
            "hard" => Ok(Self::Hard),
            "resume" => Ok(Self::Resume),
            "pause" => Ok(Self::Pause),
            _ => Err(ApiError::bad_request(
                "invalid_control",
                "Choose soft, hard, pause or resume",
            )),
        }
    }
}

pub async fn control_agent(st: &AppState, seat: &str, control: Control) -> Result<(), ApiError> {
    control_seat(st, seat, control).await.map(|_| ())
}

/// `control_agent`, with what a pause or the resume of a paused agent did (None for the others).
pub async fn control_seat(
    st: &AppState,
    seat: &str,
    control: Control,
) -> Result<Option<TeamControlResult>, ApiError> {
    let view = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)?
        .ok_or_else(|| ApiError::not_found("Unknown agent"))?;
    if let Some(done) = super::pausing::pause_or_resume(st, &view, control).await? {
        return Ok(Some(done));
    }
    if control == Control::Resume {
        return undo_stop(st, &view).await;
    }
    let _claim = claim_for_stop(st, &view.rig)?;
    if control == Control::Hard {
        return hard_stop(st, seat).await.map(|()| None);
    }
    record(st, seat, ControlMode::Soft)?;
    if view.lifecycle != SeatLifecycle::Running {
        return Ok(None);
    }
    let text = if control == Control::Checkpoint {
        checkpoint_text(seat)
    } else {
        SOFT_TEXT.to_string()
    };
    send_wrap_up(st, &view, text).await.map(|()| None)
}

/// Undoes a stop under the team's claim. A pause that arrived since the caller's read resumes its
/// session instead; only other stopped agents start fresh.
async fn undo_stop(st: &AppState, seen: &SeatView) -> Result<Option<TeamControlResult>, ApiError> {
    let claim = super::pausing::claim(st, &seen.rig)?;
    let view = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(&seen.id)?
        .ok_or_else(|| ApiError::not_found("Unknown agent"))?;
    if super::pausing::is_paused(st, &view.id)? {
        return super::resuming::resume_seat(st, &view).await.map(Some);
    }
    refuse_if_budget_exhausted(st, &view.id)?;
    if view.lifecycle != SeatLifecycle::Running {
        return reconciler::restart_seat_claimed(st, &view.id, claim)
            .await
            .map(|()| None);
    }
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .clear_control(&view.id)?;
    Ok(None)
}

fn claim_for_stop(st: &AppState, rig: &str) -> Result<BootGuard, ApiError> {
    st.try_begin_boot(rig).ok_or_else(|| {
        ApiError::conflict(
            "team_starting",
            "Project is changing. Retry the stop after that operation finishes.",
        )
    })
}

/// Refuses to start an agent again once its output budget ran out.
pub(super) fn refuse_if_budget_exhausted(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let exhausted = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .budget_exhausted(seat)?;
    if exhausted {
        return Err(ApiError::conflict(
            "budget_exhausted",
            "This agent reached its output budget. Start a new project run to reset it.",
        ));
    }
    Ok(())
}

/// Stops `seat` now with every agent it started (descendants must not continue after a hard stop of
/// their parent). The caller holds the team's claim.
async fn hard_stop(st: &AppState, seat: &str) -> Result<(), ApiError> {
    // The agents it started first (the latest first), then the agent itself.
    let mut ids = super::token_limits::with_descendants(
        &st.store.lock().expect("store mutex must not be poisoned"),
        seat,
    )?;
    ids.reverse();
    stop_all_now(st, &ids).await
}

/// Records a hard stop of each of `ids`, then stops them all at once: each stop interrupts at once and
/// forces after 2 s, whatever the others do.
async fn stop_all_now(st: &AppState, ids: &[String]) -> Result<(), ApiError> {
    for id in ids {
        record(st, id, ControlMode::Hard)?;
    }
    let stops = ids.iter().map(|id| reconciler::stop_seat_now(st, id));
    let failures: Vec<String> = ids
        .iter()
        .zip(futures::future::join_all(stops).await)
        .filter_map(|(id, r)| r.err().map(|e| format!("{id}: {}", e.message)))
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ApiError::conflict("stop_failed", failures.join("; ")))
    }
}

/// The end of a soft stop's wrap-up: stopped now, unless it was undone (Resume) meanwhile. A team
/// that is changing is left for the next tick.
async fn finish_soft_stop(st: &AppState, view: &SeatView) -> Result<(), ApiError> {
    let Some(_claim) = st.try_begin_boot(&view.rig) else {
        return Ok(());
    };
    let store_mode = || {
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .control(&view.id)
    };
    if store_mode()? != Some(ControlMode::Soft) {
        return Ok(());
    }
    hard_stop(st, &view.id).await?;
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .set_control_mode(&view.id, ControlMode::Finished)?;
    Ok(())
}

/// Sends a running agent its wrap-up instruction, remembered so the stop follows its wrap-up turn.
pub(super) async fn send_wrap_up(
    st: &AppState,
    view: &SeatView,
    text: String,
) -> Result<(), ApiError> {
    // Interrupting first makes the wrap-up instruction available even to CLIs that queue input.
    let id = reconciler::send_message(
        st,
        &view.id,
        text,
        MessageIntent::Instruct,
        view.current_turn.is_some(),
    )
    .await?;
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .set_control_message(&view.id, &id)?;
    Ok(())
}

/// Where an agent keeps its checkpoint note, relative to its folder.
#[must_use]
pub fn note_path(seat: &str) -> String {
    format!(".sculptured/notes/{}.md", crate::seat_ids::member_of(seat))
}

fn checkpoint_text(seat: &str) -> String {
    format!(
        "The run is pausing: it reached its plan budget and continues by itself after the plan window resets. Stop at a checkpoint within approximately 5,000 additional output tokens. Do not begin new research, broad refactors, new tasks or child agents. Save your work, then write {} in your folder: what is done (exact artifact paths and revisions), checks actually run, what is next, and anything you would need to know to continue with nothing else in context. Keep it short. End your turn.",
        note_path(seat)
    )
}

const SOFT_TEXT: &str = "Soft stop requested by You. Wrap up the CURRENT assignment quickly within approximately 5,000 additional output tokens. Do not begin new research, broad refactors, new tasks or child agents. Save the current work, summarize exact artifact paths/revision, checks actually run, remaining problems and what the next agent needs. If appropriate, call complete_work with the honest outcome or send_agent_message to the next agent/your parent. Never claim success to finish faster. End your turn. The runner will stop this session after the wrap-up turn, the observed token budget, or 5 minutes. Usage reports may arrive late; the 5K threshold is approximate.";
/// Stops `seats` (all of team `rig`) and every agent they started, now and all at once, like Ctrl+C,
/// under one claim on the team (separate `control_agent` calls would refuse each other).
pub async fn stop_now(st: &AppState, rig: &str, seats: &[String]) -> Result<(), ApiError> {
    let _claim = claim_for_stop(st, rig)?;
    let mut all: Vec<String> = Vec::new();
    {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        for s in seats {
            all.extend(
                super::token_limits::with_descendants(&store, s)?
                    .into_iter()
                    .filter(|d| !all.contains(d))
                    .collect::<Vec<_>>(),
            );
        }
    }
    stop_all_now(st, &all).await
}

pub(super) fn record(st: &AppState, seat: &str, mode: ControlMode) -> Result<(), ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let baseline = store.seat(seat)?.and_then(|s| s.tokens_out).unwrap_or(0);
    store.record_control(seat, mode, baseline)?;
    Ok(())
}
/// Durable soft-stop budgets. Reports can arrive at turn end, so disclose possible overshoot.
pub async fn run(st: AppState) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tick.tick().await;
        if let Err(e) = super::budgets::check(&st).await {
            tracing::warn!(error=?e,"output-budget check failed");
        }
        if let Err(e) = check(&st).await {
            tracing::warn!(error=?e,"soft-stop check failed");
        }
    }
}
async fn check(st: &AppState) -> Result<(), ApiError> {
    let controls = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .soft_stops()?;
    for stop in controls {
        let view = {
            let store = st.store.lock().expect("store mutex must not be poisoned");
            match store.seat(&stop.seat)? {
                Some(view) if wrap_up_over(&store, &view, &stop)? => view,
                _ => continue,
            }
        };
        // A pause keeps the session for Resume; a soft stop is over.
        if stop.pausing {
            super::pausing::finish_pause(st, &view).await?;
        } else {
            finish_soft_stop(st, &view).await?;
        }
    }
    Ok(())
}

/// A wrap-up is over after its turn, about 5,000 more output tokens, or 5 minutes.
fn wrap_up_over(
    store: &crate::store::Store,
    view: &SeatView,
    stop: &crate::store::SoftStop,
) -> Result<bool, ApiError> {
    let events = store.latest_events_for_seat(&stop.seat, 2000)?;
    let turn = stop.message_id.as_ref().and_then(|id| {
        events.iter().find_map(|e| match &e.body {
            EventBody::MessageConsumed {
                message_id,
                turn_id,
            } if message_id == id => Some(turn_id),
            _ => None,
        })
    });
    let ended = turn.is_some_and(|t| {
        events
            .iter()
            .any(|e| matches!(&e.body,EventBody::TurnEnded{turn_id,..}if turn_id==t))
    });
    Ok(ended
        || view.tokens_out.unwrap_or(0).saturating_sub(stop.baseline) >= 5000
        || model::now_ms() - stop.requested_ms >= 300000)
}
