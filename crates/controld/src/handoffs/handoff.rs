use super::{
    Actor, AppState, Duration, EventBody, EventRecord, HandoffSource, HandoffStuck, HandoffWrite,
    HostCommand, MessageIntent, Ordering, Provenance, RecvError, Store, Subject, TeamNames, Wave,
    plan, starts,
};

/// How many times one piece of work may go round a loop of "hands work to" arrows before the
/// loop stops and says so (Hand over again goes round again). Each round costs model turns.
pub const LOOP_ROUNDS: usize = 3;

/// Actor id for handoff messages. Must match the web's `HANDOFF_ACTOR`.
pub const HANDOFF_ACTOR: &str = "handoff";

/// Actor id for a team's Job messages (see `reconciler::TEAM_START_ACTOR`).
pub(crate) const TEAM_START_ACTOR: &str = crate::reconciler::TEAM_START_ACTOR;

/// Longest result passed along, in bytes; a longer one is cut and marked.
pub const RESULT_CAP: usize = 8 * 1024;

/// Marker appended to a result cut at `RESULT_CAP`.
pub const TRUNCATED: &str = "…(truncated)";

/// Put before unfinished work that "Restart this agent" sends again.
pub const CONTINUE_PREFIX: &str = "Your previous attempt did not finish. Continue from where you left off; check what is already written before redoing work.";

/// How long a restarted agent may take to run before its owed messages are given up.
pub(in super::super) const OWED_WAIT: Duration = Duration::from_secs(60);

/// Starts the handoff dispatcher for `st` once. Without a tokio runtime (a plain unit
/// test building the router) it does nothing.
pub fn ensure_running(st: &AppState) {
    let Ok(rt) = tokio::runtime::Handle::try_current() else {
        return;
    };
    if st.handoffs_started.swap(true, Ordering::SeqCst) {
        return;
    }
    rt.spawn(run(st.clone()));
}

pub(super) async fn run(st: AppState) {
    // Subscribe before the catch-up so nothing stored in between is missed.
    let mut rx = st.hub.subscribe();
    catch_up(&st).await;
    loop {
        match rx.recv().await {
            Ok(item) => {
                if let Some((rig, boot)) = relevant(&item.event) {
                    process_rig(&st, &rig, boot).await;
                }
            }
            Err(RecvError::Lagged(_)) => catch_up(&st).await,
            Err(RecvError::Closed) => return,
        }
    }
}

/// Every started team, as after a controld restart.
pub(super) async fn catch_up(st: &AppState) {
    let rigs = match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .list_rigs()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "handoffs: cannot list teams");
            return;
        }
    };
    for r in rigs {
        process_rig(st, &r.name, true).await;
    }
}

/// The team an event may change handoffs for, and whether it began or ended a start.
pub(super) fn relevant(e: &EventRecord) -> Option<(String, bool)> {
    match (&e.subject, &e.body) {
        (Subject::Rig(r), EventBody::RigBooted { .. } | EventBody::RigDesiredChanged { .. }) => {
            Some((r.clone(), true))
        }
        (
            Subject::Seat(s),
            EventBody::TurnEnded { .. }
            | EventBody::TenureEnded { .. }
            | EventBody::SeatLifecycleChanged { .. }
            | EventBody::HandoverRequested { .. }
            | EventBody::MessageDelivered { .. }
            | EventBody::MessageConsumed { .. }
            | EventBody::MessageFailed { .. },
        ) => crate::handoffs::rig_of(s).map(|r| (r.to_string(), false)),
        _ => None,
    }
}

pub(in super::super) enum Action {
    Status {
        seat: String,
        waiting_for: Vec<String>,
        stuck: Option<HandoffStuck>,
    },
    Hand {
        write: HandoffWrite,
        host: String,
        name: String,
        team: TeamNames,
    },
    /// A result checked against the agent's result checks: recorded, and when it failed and
    /// rounds remain, the failures sent back to the agent.
    Checked {
        seat: String,
        turn_id: String,
        passed: bool,
        results: Vec<model::RuleResult>,
        bounce: bool,
    },
    /// A message the upstream agent sent itself stands in for the handoff: recorded, not sent.
    Adopt {
        seat: String,
        start_seq: i64,
        first: bool,
        message_id: String,
        from: Vec<HandoffSource>,
        wave: Option<Wave>,
    },
}

/// Plans under the store lock, then records and sends each action. A direct message recorded as
/// a handoff makes its receiver's turn on it a work turn, so the team is planned again.
pub async fn process_rig(st: &AppState, rig: &str, with_previous: bool) {
    while plan_once(st, rig, with_previous).await {}
}

/// One plan and its actions; true when a direct message was recorded as a handoff.
pub(super) async fn plan_once(st: &AppState, rig: &str, with_previous: bool) -> bool {
    let mut adopted_any = false;
    let actions = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let in_flight = st
            .handoffs_in_flight
            .lock()
            .expect("handoffs_in_flight mutex must not be poisoned")
            .clone();
        match plan::plan(&store, rig, with_previous, &in_flight) {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(%rig, error = %e, "handoffs: cannot read the team");
                return false;
            }
        }
    };
    for action in actions {
        match action {
            Action::Status {
                seat,
                waiting_for,
                stuck,
            } => set_status(st, &seat, waiting_for, stuck),
            Action::Hand {
                write,
                host,
                name,
                team,
            } => hand(st, write, host, &name, &team).await,
            Action::Adopt {
                seat,
                start_seq,
                first,
                message_id,
                from,
                wave,
            } => adopted_any |= adopt(st, &seat, start_seq, first, &message_id, from, wave),
            Action::Checked {
                seat,
                turn_id,
                passed,
                results,
                bounce,
            } => super::result_checks::record(st, &seat, &turn_id, passed, results, bounce).await,
        }
    }
    adopted_any
}

/// Records `message_id` as the handoff of `from`; true when it was recorded now.
pub(super) fn adopt(
    st: &AppState,
    seat: &str,
    start_seq: i64,
    first: bool,
    message_id: &str,
    from: Vec<HandoffSource>,
    wave: Option<Wave>,
) -> bool {
    let adopted = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .record_adopted(seat, start_seq, first, message_id, from, wave);
    match adopted {
        Ok(Some(items)) => {
            st.publish(items);
            true
        }
        Ok(None) => false,
        Err(e) => {
            tracing::error!(%seat, error = %e, "handoffs: cannot record a direct message as the handoff");
            false
        }
    }
}

pub(super) fn set_status(
    st: &AppState,
    seat: &str,
    waiting_for: Vec<String>,
    stuck: Option<HandoffStuck>,
) {
    let mut store = st.store.lock().expect("store mutex must not be poisoned");
    let view = store.seat(seat).ok().flatten();
    let (now_waiting, now_stuck) = view
        .map(|v| (v.waiting_for.unwrap_or_default(), v.handoff_stuck))
        .unwrap_or_default();
    if now_waiting == waiting_for && now_stuck == stuck {
        return;
    }
    let actor = Actor::System(HANDOFF_ACTOR.into());
    let body = EventBody::HandoffWaiting { waiting_for, stuck };
    match store.append_local(
        actor,
        Subject::Seat(seat.to_string()),
        Provenance::Heuristic,
        body,
    ) {
        Ok(item) => st.publish(vec![item]),
        Err(e) => tracing::error!(%seat, error = %e, "handoffs: cannot record the waiting state"),
    }
}

/// Marks `message_id` as being sent until the guard drops.
pub(in super::super) struct InFlight {
    pub(super) st: AppState,
    pub(super) id: String,
}

impl InFlight {
    pub(super) fn new(st: &AppState, id: &str) -> Self {
        st.handoffs_in_flight
            .lock()
            .expect("handoffs_in_flight mutex must not be poisoned")
            .insert(id.to_string());
        InFlight {
            st: st.clone(),
            id: id.to_string(),
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.st
            .handoffs_in_flight
            .lock()
            .expect("handoffs_in_flight mutex must not be poisoned")
            .remove(&self.id);
    }
}

pub(super) async fn hand(
    st: &AppState,
    mut write: HandoffWrite,
    host: String,
    name: &str,
    team: &TeamNames,
) {
    if st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .control(&write.seat)
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    let _sending = InFlight::new(st, &write.message_id);
    write.text = crate::team_context::with_context(st, &write.seat, write.text);
    let recorded = {
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        match store.record_handoff(&write) {
            Ok(Some(items)) => {
                st.publish(items);
                true
            }
            Ok(None) => false,
            Err(e) => {
                tracing::error!(seat = %write.seat, error = %e, "handoffs: cannot record a handoff");
                false
            }
        }
    };
    if !recorded || write.failed.is_some() {
        return;
    }
    send(
        st,
        &write.seat,
        &write.message_id,
        write.text.clone(),
        &host,
        name,
        team,
    )
    .await;
}

/// Sends a recorded handoff message; a failure is recorded as "not sent" with the reason.
pub(in super::super) async fn send(
    st: &AppState,
    seat: &str,
    message_id: &str,
    text: String,
    host: &str,
    name: &str,
    team: &TeamNames,
) {
    let cmd = HostCommand::Send {
        seat: seat.to_string(),
        message_id: message_id.to_string(),
        intent: MessageIntent::Inform,
        text,
    };
    if let Err(reason) = st.hosts.command(host, cmd).await {
        tracing::warn!(%seat, %reason, "handoff delivery failed; recorded as a failed message");
        let reason = format!(
            "not sent: {name} could not be reached ({})",
            team.plain(&reason)
        );
        let failed = st.local_event(
            crate::handoffs::controld(),
            Subject::Seat(seat.to_string()),
            Provenance::ProtocolObserved,
            EventBody::MessageFailed {
                message_id: message_id.to_string(),
                reason,
            },
        );
        if let Err(e) = failed {
            tracing::error!(%seat, error = %e, "handoffs: cannot record a failed delivery");
        }
    }
}

/// The team's current start: its `RigBooted` seq and, when the team was stopped since, the
/// seq of that stop. None before the first start after the handoff cutoff.
pub fn current_start(store: &Store, rig: &str) -> rusqlite::Result<Option<(i64, Option<i64>)>> {
    Ok(starts::starts(store, rig)?
        .0
        .map(|(seq, end)| (seq, end.map(|(_, at)| at))))
}
