//! Checks: an agent's design can name a shell command (its "check", such as `npm test`). Sculptured runs
//! it itself, in the agent's folder on this computer, after each turn the agent finishes, and records the
//! command's own exit status (`CheckStarted`, `CheckRan`). That is the evidence for "verified"; an agent
//! saying it is done is not. One check runs per agent at a time; a turn that ends meanwhile runs it once more.
use crate::error::ApiError;
use crate::extract::Path;
use crate::hosts::LOCAL_HOST;
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use model::{Actor, EventBody, Provenance, Subject, TurnStatus};
use std::time::Instant;
use tokio::sync::broadcast::error::RecvError;

pub use localops::checks::TIMEOUT;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/seats/{id}/check", axum::routing::post(run_now))
}

/// Whether checks run in this process. A hosted Sculptured is on the server: its checks run on the
/// agent's computer (`HostCommand::RunCheck`), never here.
#[must_use]
pub fn allowed(st: &AppState) -> bool {
    !crate::ops::hosted(st)
}

/// Runs the agent's check now (the "Run check" button).
async fn run_now(
    State(st): State<AppState>,
    Path(seat): Path<String>,
) -> Result<StatusCode, ApiError> {
    target(&st, &seat).map_err(|why| ApiError::conflict("no_check", why))?;
    start(&st, &seat);
    Ok(StatusCode::ACCEPTED)
}

pub async fn run(st: AppState) {
    let mut rx = st.hub.subscribe();
    loop {
        match rx.recv().await {
            Ok(item) => {
                if let (
                    Subject::Seat(seat),
                    EventBody::TurnEnded {
                        status: TurnStatus::Completed,
                        ..
                    },
                ) = (&item.event.subject, &item.event.body)
                    && target(&st, seat).is_ok()
                {
                    start(&st, seat);
                }
            }
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => return,
        }
    }
}

/// The agent's check command and folder, or why it has none to run here.
fn target(st: &AppState, seat: &str) -> Result<(String, String), String> {
    let rig = crate::seat_ids::rig_of(seat).ok_or("Unknown agent")?;
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let draft = store
        .draft(rig)
        .ok()
        .flatten()
        .ok_or("This agent's team has no design")?;
    let cmd = draft
        .agent_for_seat(seat)
        .and_then(|a| {
            a.advanced
                .check
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(str::to_string)
        })
        .ok_or("This agent has no check command")?;
    let view = store
        .seat(seat)
        .ok()
        .flatten()
        .ok_or("This agent has not started yet")?;
    if allowed(st) && view.host.as_deref().is_some_and(|h| h != LOCAL_HOST) {
        return Err("Checks run only for agents on this computer".into());
    }
    let cwd = view
        .cwd
        .ok_or("This agent has not started yet, so it has no folder")?;
    Ok((cmd, cwd))
}

/// Seats with a check running here.
static RUNNING: std::sync::LazyLock<localops::checks::Rerun> =
    std::sync::LazyLock::new(Default::default);

fn start(st: &AppState, seat: &str) {
    if !allowed(st) {
        let (st, seat) = (st.clone(), seat.to_string());
        tokio::spawn(async move { on_computer(&st, &seat).await });
        return;
    }
    if !RUNNING.start(seat) {
        return;
    }
    let (st, seat) = (st.clone(), seat.to_string());
    tokio::spawn(async move {
        loop {
            // Read again each time: the design or the folder may have changed.
            if let Ok((cmd, cwd)) = target(&st, &seat) {
                check_once(&st, &seat, &cmd, &cwd).await;
            }
            if !RUNNING.again(&seat) {
                return;
            }
        }
    });
}

async fn check_once(st: &AppState, seat: &str, cmd: &str, cwd: &str) {
    let record = |body: EventBody| {
        if let Err(e) = st.local_event(
            Actor::System("check".into()),
            Subject::Seat(seat.to_string()),
            Provenance::Verified,
            body,
        ) {
            tracing::error!(error = %e, seat, "cannot record a check");
        }
    };
    record(EventBody::CheckStarted {
        command: cmd.to_string(),
    });
    let began = Instant::now();
    let localops::checks::CheckOutcome {
        passed,
        exit_code,
        timed_out,
        output,
    } = localops::checks::execute(cmd, cwd).await;
    record(EventBody::CheckRan {
        command: cmd.to_string(),
        passed,
        exit_code,
        timed_out,
        output,
        duration_ms: began.elapsed().as_millis() as u64,
    });
}

/// Hosted: asks the agent's computer to run the check (it queues and reports the result as events).
/// A refusal, such as checks turned off there, is recorded as a failed check saying why.
async fn on_computer(st: &AppState, seat: &str) {
    let Ok((command, cwd)) = target(st, seat) else {
        return;
    };
    let host = match crate::launch::seat_host(st, seat) {
        Ok(h) => h,
        Err(_) => return,
    };
    // Hosted launches make the agent's own copy on the computer, so the check names it the same way.
    let own_copy = crate::seat_ids::rig_of(seat).and_then(|rig| {
        let (_, s) = crate::launch::compiled_seat(st, seat).ok()?;
        crate::launch::own_copy_for(st, rig, seat, s.runtime)
    });
    let sent = match st
        .hosts
        .check_features(&host, &[host_proto::features::LOCAL_OPS])
    {
        Err(e) => Err(e),
        Ok(()) => {
            st.hosts
                .command(
                    &host,
                    host_proto::HostCommand::RunCheck {
                        seat: seat.to_string(),
                        command: command.clone(),
                        cwd,
                        own_copy,
                    },
                )
                .await
        }
    };
    if let Err(why) = sent {
        let body = EventBody::CheckRan {
            command,
            passed: false,
            exit_code: None,
            timed_out: false,
            output: why,
            duration_ms: 0,
        };
        if let Err(e) = st.local_event(
            Actor::System("check".into()),
            Subject::Seat(seat.to_string()),
            Provenance::Verified,
            body,
        ) {
            tracing::error!(error = %e, seat, "cannot record a refused check");
        }
    }
}
