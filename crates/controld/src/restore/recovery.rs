use super::{
    AppState, Desired, EventBody, LOCAL_HOST, Provenance, RigSource, Router, SeatLifecycle,
    SeatView, Subject, decide,
};

/// `POST /api/seats/{id}/restore` and `POST /api/teams/{id}/restore`.
pub fn routes() -> Router<AppState> {
    decide::routes()
}

/// Why a seat that should have a process needs attention after its host came back
/// without it. In plain words: what happened and what the operator can do. A team is
/// started again from the team; a rig booted from a file, from that file; when the
/// source can't be read, neither is guessed. Nothing is relaunched without that
/// decision (restore-on-boot is M2).
pub(super) fn lost_process_reason(host: &str, source: Option<&RigSource>) -> String {
    let when = if host == LOCAL_HOST {
        "Sculptured".to_string()
    } else {
        format!("host {host}")
    };
    let again = match source {
        Some(RigSource::Team { .. }) => "Start the team again",
        Some(RigSource::File { .. }) => "Start it again from its file",
        None => "Start it again",
    };
    format!("This agent's process ended when {when} restarted. {again} to relaunch it.")
}

/// On every Hello: seats that went stale while the host was away but are still alive
/// on it go back to running, or to launching when the host says they are still starting
/// (`launching_seats`, a subset of `live_seats`). Every other seat on that host that should have a process
/// (stale, running or launching) has lost it, for example because hostd restarted,
/// so it needs attention.
pub fn on_hello(st: &AppState, host: &str, live_seats: &[String], launching_seats: &[String]) {
    let seats = match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seats_on_host(host)
    {
        Ok(seats) => seats,
        Err(e) => {
            tracing::error!(%host, error = %e, "cannot read seats on reconnect");
            return;
        }
    };
    for s in seats {
        let launching = launching_seats.contains(&s.id);
        let live = live_seats.contains(&s.id) || launching;
        if stop_owed(st, &s, live) {
            continue;
        }
        if let Some((provenance, lifecycle, reason)) = after_hello(st, host, &s, live, launching) {
            let lost = lifecycle == SeatLifecycle::Attention && !live;
            crate::restore::record_lifecycle(
                st,
                s.id.clone(),
                provenance,
                lifecycle,
                reason.clone(),
            );
            if lost {
                offer(st, &s, reason);
            }
        }
    }
}

/// An agent stopped while its computer was offline: stopped now that it is back. The computer obeys
/// the server: an agent it still runs that the server has stopped is ended. True when that settles
/// the seat (nothing else to reattach).
pub(super) fn stop_owed(st: &AppState, s: &SeatView, live: bool) -> bool {
    let team_stopped = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(&s.rig)
        .ok()
        .flatten()
        .is_some_and(|r| r.desired == Desired::Stopped);
    let stopped_here = s.lifecycle == SeatLifecycle::Stopped && live;
    let owed = s.lifecycle == SeatLifecycle::Stopping
        || stopped_here
        || (team_stopped && s.lifecycle == SeatLifecycle::Stale);
    if !owed {
        return false;
    }
    if !live {
        let reason = "Its computer reconnected; the agent had already ended".to_string();
        crate::restore::record_lifecycle(
            st,
            s.id.clone(),
            Provenance::ProtocolObserved,
            SeatLifecycle::Stopped,
            reason,
        );
        return true;
    }
    if !stopped_here {
        crate::restore::record_lifecycle(
            st,
            s.id.clone(),
            Provenance::Operator,
            SeatLifecycle::Stopping,
            "Its computer reconnected; stopping the agent".into(),
        );
    }
    let (st, host, seat) = (st.clone(), s.host.clone().unwrap_or_default(), s.id.clone());
    tokio::spawn(async move {
        if let Err(e) = crate::reconciler::stop_on_host(&st, &host, &seat).await {
            tracing::warn!(%seat, error = %e, "the stop owed on reconnect failed");
        }
    });
    true
}

/// A lost agent in a team still set to run waits for the team's restore answer. It can be resumed
/// when its session id is known and its driver resumes sessions.
pub(super) fn offer(st: &AppState, s: &SeatView, reason: String) {
    let snap = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(&s.rig)
        .ok()
        .flatten();
    if !snap.is_some_and(|r| r.desired == Desired::Running) {
        return;
    }
    let can_resume = s.native_session_id.is_some() && s.caps.as_ref().is_some_and(|c| c.resume);
    let body = EventBody::RestoreNeeded { reason, can_resume };
    if let Err(e) = st.local_event(
        crate::restore::controld(),
        Subject::Seat(s.id.clone()),
        Provenance::Heuristic,
        body,
    ) {
        tracing::error!(seat = %s.id, error = %e, "cannot record restore offer");
    }
}

/// What a seat becomes after its host's Hello, if anything changes: back to launching or running
/// when the host still has its process, or attention when the process is gone.
pub(super) fn after_hello(
    st: &AppState,
    host: &str,
    s: &SeatView,
    live: bool,
    launching: bool,
) -> Option<(Provenance, SeatLifecycle, String)> {
    let observed =
        |lifecycle, why: &str| Some((Provenance::ProtocolObserved, lifecycle, why.to_string()));
    match s.lifecycle {
        SeatLifecycle::Stale | SeatLifecycle::Attention if launching => observed(
            SeatLifecycle::Launching,
            "Reconnected; the agent is still starting",
        ),
        SeatLifecycle::Stale if live => observed(
            SeatLifecycle::Running,
            "Reconnected; the agent kept running",
        ),
        SeatLifecycle::Attention if live => observed(
            SeatLifecycle::Running,
            "Reconnected; the agent's process is still running",
        ),
        SeatLifecycle::Stale | SeatLifecycle::Running | SeatLifecycle::Launching if !live => {
            let source = st
                .store
                .lock()
                .expect("store mutex must not be poisoned")
                .rig_source(&s.rig)
                .ok()
                .flatten();
            Some((
                Provenance::Heuristic,
                SeatLifecycle::Attention,
                lost_process_reason(host, source.as_ref()),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_reasons_follow_the_rig_source_and_never_guess() {
        let team = RigSource::Team {
            id: "t".into(),
            revision: 0,
        };
        let file = RigSource::File {
            path: "/x.yaml".into(),
        };
        let start = "This agent's process ended when Sculptured restarted.";
        assert_eq!(
            lost_process_reason(LOCAL_HOST, Some(&team)),
            format!("{start} Start the team again to relaunch it.")
        );
        assert_eq!(
            lost_process_reason(LOCAL_HOST, Some(&file)),
            format!("{start} Start it again from its file to relaunch it.")
        );
        assert_eq!(
            lost_process_reason(LOCAL_HOST, None),
            format!("{start} Start it again to relaunch it.")
        );
        assert_eq!(
            lost_process_reason("box", None),
            "This agent's process ended when host box restarted. Start it again to relaunch it."
        );
    }
}
