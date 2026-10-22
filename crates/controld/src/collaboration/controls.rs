use crate::error::ApiError;
use crate::reconciler;
use crate::state::AppState;
use crate::store::ControlMode;
use model::{EventBody, MessageIntent, SeatLifecycle};

/// What the operator (or a limit) asks of a running agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Wrap up the current work, then stop.
    Soft,
    /// Wrap up at a checkpoint the agent can continue from: the run pauses until its plan window resets.
    Checkpoint,
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
            _ => Err(ApiError::bad_request(
                "invalid_control",
                "Choose soft, hard or resume",
            )),
        }
    }
}

pub async fn control_agent(st: &AppState, seat: &str, control: Control) -> Result<(), ApiError> {
    let view = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)?
        .ok_or_else(|| ApiError::not_found("Unknown agent"))?;
    if control == Control::Resume {
        if st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .budget_exhausted(seat)?
        {
            return Err(ApiError::conflict(
                "budget_exhausted",
                "This agent reached its output budget. Start a new project run to reset it.",
            ));
        }
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .clear_control(seat)?;
        if view.lifecycle != SeatLifecycle::Running {
            reconciler::restart_seat(st, seat).await?;
        }
        return Ok(());
    }
    let _claim = st.try_begin_boot(&view.rig).ok_or_else(|| {
        ApiError::conflict(
            "team_starting",
            "Project is changing. Retry the stop after that operation finishes.",
        )
    })?;
    let children = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let mut all = store.children(seat)?;
        let mut i = 0;
        while i < all.len() {
            let next = store.children(&all[i])?;
            for child in next {
                if !all.contains(&child) {
                    all.push(child)
                }
            }
            i += 1;
        }
        all
    };
    // Descendants must not continue after a hard stop of their parent.
    if control == Control::Hard {
        let mut failures = Vec::new();
        let matching_children = children
            .iter()
            .rev()
            .map(String::as_str)
            .chain(std::iter::once(seat));
        for id in matching_children {
            record(st, id, ControlMode::Hard)?;
        }
        // All at once: each stop interrupts at once and forces after 2 s, whatever the others do.
        let ids: Vec<&str> = children
            .iter()
            .rev()
            .map(String::as_str)
            .chain(std::iter::once(seat))
            .collect();
        let stops = ids.iter().map(|id| reconciler::stop_seat_now(st, id));
        let paired_ids = ids.iter().zip(futures::future::join_all(stops).await);
        for (id, r) in paired_ids {
            if let Err(e) = r {
                failures.push(format!("{id}: {}", e.message));
            }
        }
        if !failures.is_empty() {
            return Err(ApiError::conflict("stop_failed", failures.join("; ")));
        }
        return Ok(());
    }
    record(st, seat, ControlMode::Soft)?;
    if view.lifecycle != SeatLifecycle::Running {
        return Ok(());
    }
    let text = if control == Control::Checkpoint {
        checkpoint_text(seat)
    } else {
        SOFT_TEXT.to_string()
    };
    // Interrupting first makes the wrap-up instruction available even to CLIs that queue input.
    let id = reconciler::send_message(
        st,
        seat,
        text,
        MessageIntent::Instruct,
        view.current_turn.is_some(),
    )
    .await?;
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .set_control_message(seat, &id)?;
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
    let _claim = st.try_begin_boot(rig).ok_or_else(|| {
        ApiError::conflict(
            "team_starting",
            "Project is changing. Retry the stop after that operation finishes.",
        )
    })?;
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
    for id in &all {
        record(st, id, ControlMode::Hard)?;
    }
    let results =
        futures::future::join_all(all.iter().map(|id| reconciler::stop_seat_now(st, id))).await;
    let failures: Vec<String> = all
        .iter()
        .zip(results)
        .filter_map(|(id, r)| r.err().map(|e| format!("{id}: {}", e.message)))
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ApiError::conflict("stop_failed", failures.join("; ")))
    }
}

fn record(st: &AppState, seat: &str, mode: ControlMode) -> Result<(), ApiError> {
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
    for crate::store::SoftStop {
        seat,
        baseline: base,
        requested_ms: at,
        message_id: msg,
    } in controls
    {
        let stop = {
            let store = st.store.lock().expect("store mutex must not be poisoned");
            let Some(view) = store.seat(&seat)? else {
                continue;
            };
            let events = store.latest_events_for_seat(&seat, 2000)?;
            let turn = msg.as_ref().and_then(|id| {
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
            ended
                || view.tokens_out.unwrap_or(0).saturating_sub(base) >= 5000
                || model::now_ms() - at >= 300000
        };
        if stop {
            control_agent(st, &seat, Control::Hard).await?;
            st.store
                .lock()
                .expect("store mutex must not be poisoned")
                .set_control_mode(&seat, ControlMode::Finished)?;
        }
    }
    Ok(())
}
