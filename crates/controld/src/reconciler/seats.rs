//! One agent at a time: restart, stop, interrupt, and messages the operator sends.
use super::STOP_GRACE_MS;
use crate::error::ApiError;
use crate::hosts::CommandFailure;
use crate::state::AppState;
use crate::views::StopFailure;
use host_proto::HostCommand;
use model::{Actor, Desired, EventBody, MessageIntent, Provenance, SeatLifecycle, Subject};

/// "Restart this agent": relaunches one agent that needs attention, failed or stopped,
/// inside a team that is still set to run, without touching the others. Once it runs it
/// gets what it is owed (`handoffs::deliver_owed`).
pub async fn restart_seat(st: &AppState, seat_id: &str) -> Result<(), ApiError> {
    let view = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat_id)?
        .ok_or_else(|| ApiError::not_found(format!("no agent {seat_id:?}")))?;
    let snap = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(&view.rig)?;
    let snap = snap.ok_or_else(|| ApiError::not_found(format!("no team {:?}", view.rig)))?;
    let who = view.label.clone().unwrap_or_else(|| view.member.clone());
    if snap.desired != Desired::Running {
        return Err(ApiError::conflict(
            "team_not_running",
            "This team is stopped. Start the team to run its agents.",
        ));
    }
    if !matches!(
        view.lifecycle,
        SeatLifecycle::Attention | SeatLifecycle::Failed | SeatLifecycle::Stopped
    ) {
        return Err(ApiError::conflict(
            "agent_not_restartable",
            format!(
                "Only an agent that needs attention, failed or stopped can be restarted; {who} is {}.",
                format!("{:?}", view.lifecycle).to_lowercase()
            ),
        ));
    }
    let seat = snap
        .rig
        .seats
        .iter()
        .find(|s| s.id == seat_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("no agent {seat_id:?}")))?;
    let Some(_claim) = st.try_begin_boot(&view.rig) else {
        return Err(ApiError::conflict(
            "team_starting",
            "This team is still starting. Restart the agent once it has finished starting.",
        ));
    };
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .clear_control(seat_id)?;
    let spec_dir = crate::launch::spec_dir(st, &view.rig);
    // Held from before the relaunch until what it is owed has been sent.
    let restarting = crate::handoffs::Restarting::new(st, seat_id);
    if !crate::reconciler::launch_one(st, &seat, &spec_dir).await {
        let reason = st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .seat(seat_id)?
            .and_then(|v| v.lifecycle_reason)
            .unwrap_or_else(|| "unknown".into());
        return Err(ApiError::bad_gateway(
            "restart_failed",
            format!("{who} could not be restarted: {reason}"),
        ));
    }
    tokio::spawn(crate::handoffs::deliver_owed(
        st.clone(),
        seat_id.to_string(),
        restarting,
        crate::handoffs::OwedMode::Fresh,
    ));
    Ok(())
}

/// Stops a seat's process. A host that says the seat is not running has nothing to
/// stop; reconcile any live projection from that explicit response. A computer that is offline
/// cannot stop it now, and may never come back: the seat is stopped here at once, with a reason
/// that says it may still run there, and the computer ends it when it reconnects
/// (`restore::on_hello`), or `sculptured stop` ends it on that computer.
/// Why an agent whose computer is offline shows as stopped.
pub(crate) const OFFLINE_STOP: &str = "Stopped here; its computer is offline. If the agent is still running there, it is ended when that computer reconnects, or run `sculptured stop` on it";

pub(crate) async fn stop_on_host(st: &AppState, host: &str, seat: &str) -> Result<(), String> {
    stop_on_host_as(st, host, seat, false).await
}

/// `stop_on_host`, or with `hard` a stop like Ctrl+C: interrupt now, force after 2 s.
pub(crate) async fn stop_on_host_as(
    st: &AppState,
    host: &str,
    seat: &str,
    hard: bool,
) -> Result<(), String> {
    match st
        .hosts
        .command_now(
            host,
            HostCommand::Stop {
                seat: seat.to_string(),
                grace_ms: STOP_GRACE_MS,
                hard,
            },
        )
        .await
    {
        Err(CommandFailure::NotConnected(_)) => {
            let reason = OFFLINE_STOP.to_string();
            crate::hosts::record_lifecycle(
                st,
                seat.to_string(),
                Provenance::Operator,
                SeatLifecycle::Stopped,
                reason,
            );
            Ok(())
        }
        Err(CommandFailure::Refused(e)) if e == format!("seat {seat} is not running") => {
            tracing::info!(%seat, error = %e, "nothing to stop on host");
            if crate::reconciler::lifecycle(st, seat).is_some_and(stoppable) {
                let recorded = st.local_event(
                    crate::hosts::controld(),
                    Subject::Seat(seat.to_string()),
                    Provenance::ProtocolObserved,
                    EventBody::SeatLifecycleChanged {
                        lifecycle: SeatLifecycle::Stopped,
                        reason: Some("stopped by operator; no process was running".into()),
                    },
                );
                recorded.map_err(|e| format!("cannot record stopped: {e}"))?;
            }
            Ok(())
        }
        other => other.map_err(CommandFailure::into_message),
    }
}

pub(crate) fn stoppable(l: SeatLifecycle) -> bool {
    crate::store::is_live(l) || l == SeatLifecycle::Attention
}

/// Records the stop intent, then stops live seats in reverse launch order. A seat that
/// could not be stopped does not stop the others; it is returned with the reason.
pub async fn stop_rig(st: AppState, name: String) -> Result<Vec<StopFailure>, ApiError> {
    let snap = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(&name)?
        .ok_or_else(|| ApiError::not_found(format!("no rig {name:?}")))?;
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .set_desired(&name, Desired::Stopped)?;
    st.local_event(
        Actor::Human("local".into()),
        Subject::Rig(name.clone()),
        Provenance::Operator,
        EventBody::RigDesiredChanged {
            desired: Desired::Stopped,
        },
    )?;
    let mut failed = Vec::new();
    let reversed_launch_layers = snap.rig.launch_layers.iter().rev();
    for layer in reversed_launch_layers {
        for seat_id in layer {
            if !crate::reconciler::lifecycle(&st, seat_id).is_some_and(stoppable) {
                continue;
            }
            let host = crate::launch::seat_host(&st, seat_id)?;
            if let Err(e) = stop_on_host(&st, &host, seat_id).await {
                tracing::warn!(seat = %seat_id, error = %e, "stop failed");
                failed.push(StopFailure {
                    seat: seat_id.clone(),
                    reason: e,
                });
            }
        }
    }
    Ok(failed)
}

/// Records the operator message, then delivers it. A delivery failure is recorded
/// as MessageFailed and reported as 502 with the host's reason.
pub async fn send_message(
    st: &AppState,
    seat: &str,
    text: String,
    intent: MessageIntent,
    interrupt: bool,
) -> Result<String, ApiError> {
    send_message_as(
        st,
        seat,
        text,
        intent,
        interrupt,
        Actor::Human("local".into()),
    )
    .await
}

/// `send_message` with an explicit actor (the team's Job uses `system:team-start`).
pub async fn send_message_as(
    st: &AppState,
    seat: &str,
    text: String,
    intent: MessageIntent,
    interrupt: bool,
    actor: Actor,
) -> Result<String, ApiError> {
    let host = crate::launch::seat_host(st, seat)?;
    let text = crate::team_context::with_context(st, seat, text);
    let message_id = uuid::Uuid::new_v4().to_string();
    let provenance = if matches!(&actor, Actor::Seat(_)) {
        Provenance::AgentReported
    } else {
        Provenance::Operator
    };
    st.local_event(
        actor,
        Subject::Seat(seat.to_string()),
        provenance,
        EventBody::MessageQueued {
            message_id: message_id.clone(),
            intent,
            text: text.clone(),
        },
    )?;
    let delivered: Result<(), String> = async {
        if interrupt && intent == MessageIntent::Instruct {
            st.hosts
                .command(
                    &host,
                    HostCommand::Interrupt {
                        seat: seat.to_string(),
                    },
                )
                .await?;
        }
        st.hosts
            .command(
                &host,
                HostCommand::Send {
                    seat: seat.to_string(),
                    message_id: message_id.clone(),
                    intent,
                    text,
                },
            )
            .await
    }
    .await;
    if let Err(reason) = delivered {
        st.local_event(
            crate::hosts::controld(),
            Subject::Seat(seat.to_string()),
            Provenance::ProtocolObserved,
            EventBody::MessageFailed {
                message_id: message_id.clone(),
                reason: reason.clone(),
            },
        )?;
        return Err(ApiError::bad_gateway("host_unreachable", reason));
    }
    Ok(message_id)
}

pub async fn interrupt_seat(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let host = crate::launch::seat_host(st, seat)?;
    st.hosts
        .command(
            &host,
            HostCommand::Interrupt {
                seat: seat.to_string(),
            },
        )
        .await
        .map_err(|e| ApiError::bad_gateway("host_unreachable", e))
}

pub async fn stop_seat(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let host = crate::launch::seat_host(st, seat)?;
    stop_on_host(st, &host, seat)
        .await
        .map_err(|e| ApiError::bad_gateway("host_unreachable", e))
}

/// Stops a seat now, like Ctrl+C: its program and everything it started are interrupted, and
/// forced after 2 seconds.
pub async fn stop_seat_now(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let host = crate::launch::seat_host(st, seat)?;
    stop_on_host_as(st, &host, seat, true)
        .await
        .map_err(|e| ApiError::bad_gateway("host_unreachable", e))
}
