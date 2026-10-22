use super::{
    Actor, ApiError, AppState, BootGuard, CompiledRig, Desired, Duration, EventBody, HostCommand,
    Path, PathBuf, Provenance, RigSource, Runtime, SeatLifecycle, Subject, Value, json,
};

pub(super) const READY_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const STOP_GRACE_MS: u64 = 5000;

/// Plain words for an agent type, for reasons shown in the UI.
#[must_use]
pub fn runtime_words(rt: Runtime) -> &'static str {
    match rt {
        Runtime::ClaudeCode => "Claude",
        Runtime::Codex => "Codex",
        Runtime::Pi => "Pi",
        Runtime::Omp => "OMP",
        Runtime::Terminal => "Terminal",
        Runtime::Stub => "Practice",
    }
}

/// Starts a compiled rig. Refuses a running rig and a name another source owns, claims
/// the boot synchronously (a concurrent start gets 409), records the source, desired
/// running and RigBooted, then boots layer by layer in the background.
pub fn start_rig(
    st: &AppState,
    rig: CompiledRig,
    source: RigSource,
    spec_dir: PathBuf,
) -> Result<Value, ApiError> {
    check_startable(st, &rig.name, &source)?;
    // Claim before touching the stored spec: a boot already in flight must not be revived with a new spec.
    let Some(claim) = st.try_begin_boot(&rig.name) else {
        return Err(running_conflict(&rig.name));
    };
    launch_claimed(st, rig, &source, spec_dir, claim)
}

/// `start_rig` for a caller that claimed the boot itself before reading what it starts
/// (a team claims before reading its draft, so a save cannot slip in between).
pub fn start_rig_claimed(
    st: &AppState,
    rig: CompiledRig,
    source: RigSource,
    spec_dir: PathBuf,
    claim: BootGuard,
) -> Result<Value, ApiError> {
    check_startable(st, &rig.name, &source)?;
    launch_claimed(st, rig, &source, spec_dir, claim)
}

pub(super) fn running_conflict(name: &str) -> ApiError {
    ApiError::conflict(
        "rig_running",
        format!("rig {name:?} is running; stop it before booting it again"),
    )
}

pub(super) fn name_taken(name: &str) -> ApiError {
    ApiError::conflict(
        "rig_name_taken",
        format!("the name {name:?} is already used by another team or spec file"),
    )
}

/// Refuses a rig whose name another source owns, and a rig with a live seat. A spec
/// file may not take the id of a team draft either, even one never started: the team
/// could then never start under its own id.
pub(super) fn check_startable(
    st: &AppState,
    name: &str,
    source: &RigSource,
) -> Result<(), ApiError> {
    let (running, owner, drafted) = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let running = store.rig_snapshot(name)?.is_some_and(|s| {
            s.seats
                .iter()
                .any(|x| crate::reconciler::is_live(x.lifecycle))
        });
        let drafted = matches!(source, RigSource::File { .. }) && store.draft(name)?.is_some();
        (running, store.rig_source(name)?, drafted)
    };
    if drafted {
        return Err(name_taken(name));
    }
    if let Some(owner) = owner
        && !owner.same_owner(source)
    {
        return Err(name_taken(name));
    }
    if running {
        return Err(running_conflict(name));
    }
    Ok(())
}

pub(super) fn launch_claimed(
    st: &AppState,
    rig: CompiledRig,
    source: &RigSource,
    spec_dir: PathBuf,
    claim: BootGuard,
) -> Result<Value, ApiError> {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .upsert_rig(&rig, source, Desired::Running)?;
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .clear_controls_of(&rig.name)?;
    st.local_event(
        Actor::Human("local".into()),
        Subject::Rig(rig.name.clone()),
        Provenance::Operator,
        EventBody::RigBooted {
            spec_hash: rig.spec_hash.clone(),
            seat_count: rig.seats.len() as u32,
        },
    )?;
    let body = json!({ "rig": rig.name, "diagnostics": rig.diagnostics });
    tokio::spawn(boot_claimed(st.clone(), rig, spec_dir, claim));
    Ok(body)
}

#[must_use]
pub fn resolve_cwd(spec_dir: &Path, cwd: &str) -> PathBuf {
    let p = Path::new(cwd);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        spec_dir.join(p)
    }
}

/// Records Attention. `provenance` is Heuristic when controld inferred the problem
/// (a timeout, a missing driver or host) and ProtocolObserved when a host reported it.
pub(crate) fn attention(st: &AppState, seat: &str, provenance: Provenance, reason: String) {
    let recorded = st.local_event(
        crate::reconciler::controld(),
        Subject::Seat(seat.to_string()),
        provenance,
        EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Attention,
            reason: Some(reason.clone()),
        },
    );
    if let Err(e) = recorded {
        tracing::error!(%seat, %reason, error = %e, "cannot record attention");
    }
}

/// The seat's lifecycle; None if the seat is unknown or the store cannot be read (logged).
pub(crate) fn lifecycle(st: &AppState, seat: &str) -> Option<SeatLifecycle> {
    match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)
    {
        Ok(view) => view.map(|s| s.lifecycle),
        Err(e) => {
            tracing::error!(%seat, error = %e, "cannot read seat lifecycle");
            None
        }
    }
}

pub(super) fn settled(l: SeatLifecycle) -> bool {
    matches!(
        l,
        SeatLifecycle::Running
            | SeatLifecycle::Attention
            | SeatLifecycle::Failed
            | SeatLifecycle::Stopped
    )
}

pub(crate) async fn wait_settled(st: &AppState, seats: &[String]) {
    if seats.is_empty() {
        return;
    }
    let mut rx = st.hub.subscribe();
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if seats.iter().all(|s| lifecycle(st, s).is_some_and(settled)) {
            return;
        }
        if tokio::time::timeout_at(deadline, rx.recv()).await.is_err() {
            for s in seats {
                // Only seats still starting: a Stale seat keeps its lost-contact reason.
                if matches!(
                    lifecycle(st, s),
                    Some(SeatLifecycle::Launching | SeatLifecycle::Planned)
                ) {
                    attention(
                        st,
                        s,
                        Provenance::Heuristic,
                        format!("did not become ready within {}s", READY_TIMEOUT.as_secs()),
                    );
                }
            }
            return;
        }
    }
}

pub(super) fn still_wanted(st: &AppState, rig: &str) -> bool {
    match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(rig)
    {
        Ok(Some(snap)) => snap.desired == Desired::Running,
        Ok(None) => false,
        Err(e) => {
            tracing::error!(%rig, error = %e, "cannot read desired state; stopping boot");
            false
        }
    }
}

/// Launches each launch layer, waiting for it to settle before the next one. A second
/// boot of a rig that is already booting is a no-op; the desired state is re-read before
/// every layer so a stop issued mid-boot prevents later layers from launching.
pub async fn boot(st: AppState, rig: CompiledRig, spec_dir: PathBuf) {
    let Some(guard) = st.try_begin_boot(&rig.name) else {
        tracing::info!(rig = %rig.name, "boot already in progress; ignoring duplicate boot");
        return;
    };
    boot_claimed(st, rig, spec_dir, guard).await;
}

/// The boot body, for a caller that already holds the rig's boot claim (see
/// `AppState::try_begin_boot`). The claim is released when this returns.
pub async fn boot_claimed(st: AppState, rig: CompiledRig, spec_dir: PathBuf, guard: BootGuard) {
    let _guard = guard;
    for layer in &rig.launch_layers {
        if !still_wanted(&st, &rig.name) {
            tracing::info!(rig = %rig.name, "rig is no longer desired running; boot stopped before next layer");
            return;
        }
        let launched = launch_layer(&st, &rig, layer, &spec_dir).await;
        wait_settled(&st, &launched).await;
        if !still_wanted(&st, &rig.name) {
            tracing::info!(rig = %rig.name, seats = ?launched, "rig no longer desired running after layer settled; stopping seats this boot launched");
            stop_abandoned(&st, &launched).await;
            return;
        }
    }
}

/// Launches the layer's seats that are not live yet; returns the ones launched.
pub(super) async fn launch_layer(
    st: &AppState,
    rig: &CompiledRig,
    layer: &[String],
    spec_dir: &Path,
) -> Vec<String> {
    let mut launched = Vec::new();
    for seat_id in layer {
        if lifecycle(st, seat_id).is_some_and(crate::reconciler::is_live) {
            tracing::info!(seat = %seat_id, "seat already live; not relaunching");
            continue;
        }
        let seat = rig
            .seats
            .iter()
            .find(|s| &s.id == seat_id)
            .expect("layer seat belongs to rig");
        if launch_one(st, seat, spec_dir).await {
            launched.push(seat_id.clone());
        }
    }
    launched
}

/// Stops the seats an abandoned boot launched.
pub(super) async fn stop_abandoned(st: &AppState, launched: &[String]) {
    for seat_id in launched {
        if !lifecycle(st, seat_id).is_some_and(crate::reconciler::stoppable) {
            continue;
        }
        tracing::info!(seat = %seat_id, "stopping seat launched by an abandoned boot");
        let host = crate::launch::agent_host(st, seat_id);
        if let Err(e) = crate::reconciler::stop_on_host(st, &host, seat_id).await {
            tracing::warn!(seat = %seat_id, error = %e, "stop of abandoned-boot seat failed");
        }
    }
}

/// The local driver for the seat's runtime; None (with the seat in attention, saying why) when
/// this version or the local agent runner cannot run it.
pub(super) fn local_driver(
    st: &AppState,
    seat: &rigspec::CompiledSeat,
    host: &str,
) -> Option<&'static str> {
    let Some(driver) = crate::reconciler::driver_for(seat.runtime) else {
        attention(
            st,
            &seat.id,
            Provenance::Heuristic,
            format!(
                "{} agents can't run in this version yet",
                runtime_words(seat.runtime)
            ),
        );
        return None;
    };
    if !st.hosts.has_driver(host, driver) {
        let reason = if st.hosts.is_connected(host) {
            format!(
                "Sculptured's agent runner can't run {} agents",
                runtime_words(seat.runtime)
            )
        } else {
            crate::hosts::not_connected(host)
        };
        attention(st, &seat.id, Provenance::Heuristic, reason);
        return None;
    }
    Some(driver)
}

/// A Launch, or, for an agent its team's Start asked to resume, a Restart that resumes its last
/// conversation (on a runner that can; otherwise it starts fresh).
pub(super) fn start_command(
    st: &AppState,
    host: &str,
    seat: &str,
    driver: &str,
    mut params: host_proto::LaunchParams,
) -> HostCommand {
    let resume = st
        .start_resumes
        .lock()
        .expect("start_resumes mutex must not be poisoned")
        .remove(seat);
    match resume.filter(|_| st.hosts.host_supports(host, host_proto::features::RESTART)) {
        Some(crate::state::ResumeSession {
            native_session_id,
            usage_baseline,
        }) => {
            if st
                .hosts
                .host_supports(host, host_proto::features::USAGE_BASELINE)
            {
                params.usage_baseline = Some(usage_baseline);
            }
            HostCommand::Restart {
                seat: seat.to_string(),
                driver: driver.to_string(),
                params,
                mode: host_proto::LaunchMode::Resume { native_session_id },
                cause: model::TenureCause::Resume,
            }
        }
        None => HostCommand::Launch {
            seat: seat.to_string(),
            driver: driver.to_string(),
            params,
        },
    }
}

/// Launches one seat on the local host. False (with the seat in attention, saying why)
/// when it could not be launched.
pub(crate) async fn launch_one(
    st: &AppState,
    seat: &rigspec::CompiledSeat,
    spec_dir: &Path,
) -> bool {
    let seat_id = &seat.id;
    let rig = crate::reconciler::rig_of(seat_id).unwrap_or_default();
    // A spawned subagent runs where its parent runs (its folder is there); others on their own computer
    // or the team's.
    let parent_host = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        store
            .parent(seat_id)
            .ok()
            .flatten()
            .and_then(|p| store.seat(&p).ok().flatten())
            .and_then(|v| v.host)
    };
    let host = parent_host.unwrap_or_else(|| crate::launch::agent_host(st, seat_id));
    // A computer whose link just dropped gets a moment to come back before the agent needs attention.
    st.hosts.wait_connected(&host).await;
    let Some(driver) = local_driver(st, seat, &host) else {
        return false;
    };
    let params = match crate::reconciler::launch_params_with_connection(
        st, &host, rig, seat, spec_dir,
    )
    .await
    {
        Ok(p) => p,
        Err(reason) => {
            attention(st, seat_id, Provenance::ProtocolObserved, reason);
            return false;
        }
    };
    if let Err(e) = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .set_seat_cwd(seat_id, &params.cwd)
    {
        tracing::error!(seat = %seat_id, error = %e, "cannot record the agent's folder");
    }
    let cmd = start_command(st, &host, seat_id, driver, params);
    match st.hosts.command(&host, cmd).await {
        Ok(()) => true,
        Err(e) if e.contains("already running") => {
            tracing::info!(seat = %seat_id, error = %e, "host reports the seat is already running");
            true
        }
        Err(e) => {
            // hostd already recorded attention for driver launch failures, and
            // its link forwards that event before the command result.
            if lifecycle(st, seat_id) != Some(SeatLifecycle::Attention) {
                attention(st, seat_id, Provenance::ProtocolObserved, e);
            }
            false
        }
    }
}
