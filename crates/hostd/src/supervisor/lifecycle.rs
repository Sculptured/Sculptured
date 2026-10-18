//! A seat's process from launch to exit: reserving the seat, opening the session, the pump that
//! journals its events, and Stop.
use super::{ExitGate, LiveSeat, SharedSession, Slot, Supervisor};
use crate::mapping::{map_event, PumpCtx};
use drivers::{Driver, DriverEvent, LaunchSpec, Session};
use futures::stream::{BoxStream, StreamExt};
use host_proto::{LaunchMode, LaunchParams};
use model::{EventBody, IdError, SeatId, SeatLifecycle, TenureCause};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, watch};

type Events = BoxStream<'static, DriverEvent>;

/// One launch or restart: the seat, its driver and how to start it.
struct Launch {
    seat: String,
    seat_id: SeatId,
    drv: Arc<dyn Driver>,
    params: LaunchParams,
    mode: LaunchMode,
    cause: TenureCause,
}

/// A session that started, with its events and its tenure number.
struct Opened {
    session: Box<dyn Session>,
    events: Events,
    tenure: u32,
}

impl Supervisor {
    pub(super) async fn launch(
        &self,
        seat: String,
        driver: String,
        params: LaunchParams,
        mode: LaunchMode,
        cause: TenureCause,
    ) -> Result<(), String> {
        let drv = self
            .inner
            .drivers
            .get(&driver)
            .cloned()
            .ok_or_else(|| format!("no driver {driver:?} on host {}", self.inner.host_id))?;
        let seat_id: SeatId = seat.parse().map_err(|e: IdError| e.to_string())?;
        self.reserve(&seat).await?;
        self.emit_system(&seat, EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Launching, reason: None });
        let result = self.start(Launch { seat: seat.clone(), seat_id, drv, params, mode, cause }).await;
        if let Err(reason) = &result {
            self.inner.seats.lock().await.remove(&seat);
            self.attention(&seat, format!("launch failed: {reason}"));
        }
        result
    }

    /// Reserves the seat id while the driver launches (no map lock is held across that).
    async fn reserve(&self, seat: &str) -> Result<(), String> {
        let mut seats = self.inner.seats.lock().await;
        match seats.get(seat) {
            Some(Slot::Stopping) => return Err(format!("seat {seat} is still stopping")),
            Some(_) => return Err(format!("seat {seat} is already running")),
            None => {}
        }
        seats.insert(seat.to_string(), Slot::Launching);
        Ok(())
    }

    /// Runs with the seat id reserved and no lock held; on Err the caller clears the
    /// reservation and journals Attention.
    async fn start(&self, req: Launch) -> Result<(), String> {
        let token = self.new_token(req.params.seat_tools.as_ref());
        match self.open(&req, token.as_deref()).await {
            Ok(opened) => {
                self.go_live(&req, opened, token).await;
                Ok(())
            }
            Err(e) => {
                self.forget_token(&token);
                Err(e)
            }
        }
    }

    /// Folders the agent may work in besides its own: the team's items folder (made here) on a connected
    /// computer; the folders local mode names, which a connected computer never takes from the server.
    fn add_dirs(&self, p: &host_proto::LaunchParams) -> Result<Vec<PathBuf>, String> {
        let mut dirs: Vec<PathBuf> = if self.inner.policy.is_none() { p.add_dirs.iter().map(PathBuf::from).collect() } else { vec![] };
        if let (Some(team), Some(ops)) = (&p.item_copies, &self.inner.ops) {
            if self.inner.policy.as_ref().is_some_and(|pol| !pol.caps().copies) {
                return Err("Copies of work items are turned off on this computer (sculptured connect … --enable copies)".into());
            }
            if !localops::copies::valid_name(team, team) {
                return Err(format!("{team} has no letters or digits to name its items folder with."));
            }
            let dir = localops::copies::items_root(&ops.context.copies_root, team);
            std::fs::create_dir_all(&dir).map_err(|e| format!("Can't make {}: {e}", dir.display()))?;
            dirs.push(dir);
        }
        Ok(dirs)
    }

    fn launch_spec(&self, req: &Launch, token: Option<&str>) -> Result<LaunchSpec, String> {
        let p = &req.params;
        let mut cwd = match &self.inner.policy { Some(pol) => pol.check(p)?, None => PathBuf::from(&p.cwd) };
        if let Some(own) = &p.own_copy {
            let ops = self.inner.ops.as_ref().ok_or("this runner cannot make agents' own copies")?;
            if let Some(pol) = &self.inner.policy {
                cwd = PathBuf::from(pol.whole_repo(cwd.display().to_string()).map_err(|e| e.message)?);
            }
            cwd = localops::copies::prepare(&ops.context.copies_root, &own.team, &req.seat, own.parent.as_deref(), &cwd).map_err(|e| e.message)?;
        }
        let mut spec = LaunchSpec::new(req.seat_id.clone(), cwd);
        spec.model = p.model.clone();
        spec.program = p.program.as_ref().map(PathBuf::from);
        spec.env = p.env.clone();
        spec.extra_args = p.extra_args.clone();
        spec.posture = match &self.inner.policy { Some(pol) => pol.posture(p.posture), None => p.posture };
        spec.sandboxed = self.inner.policy.as_ref().is_some_and(|pol| pol.sandboxed());
        spec.usage_baseline = p.usage_baseline;
        spec.claude_setup = p.claude_setup;
        spec.protection = p.protection.clone().and_then(|rules| self.guard_program().map(|guard| drivers::ProtectionLaunch { rules, guard }));
        spec.add_dirs = self.add_dirs(p)?;
        if let (Some(tools), Some(t)) = (&p.seat_tools, token) {
            spec.seat_tools = self.grant_tools(&req.seat, req.drv.seat_tools_config_file(), tools, t)?;
        }
        if !p.skills.is_empty() {
            if req.drv.name() == "claude-stream" {
                spec.plugin_dirs.extend(self.skills_plugin(&req.seat, &p.skills)?);
            } else if let Some(t) = spec.seat_tools.as_mut() {
                // Other CLIs read a skill with `read_skill` when its description applies.
                t.env.push((host_proto::seat_tools::SKILLS_ENV.to_string(), "1".to_string()));
            }
        }
        let safety = p.protection.as_ref().map(|r| r.safety.as_slice()).unwrap_or_default();
        (spec.mcp_servers, spec.tool_hooks) = self.components(&req.seat, &p.components, &p.integrations, safety);
        Ok(spec)
    }

    /// Starts the session, takes its events and records the new tenure. A session that started
    /// but cannot go on is stopped.
    async fn open(&self, req: &Launch, token: Option<&str>) -> Result<Opened, String> {
        let spec = self.launch_spec(req, token)?;
        let started = match &req.mode {
            LaunchMode::Fresh => req.drv.launch(spec).await,
            LaunchMode::Resume { native_session_id } => req.drv.resume(spec, native_session_id).await,
            LaunchMode::Fork { native_session_id } => req.drv.fork(spec, native_session_id).await,
        };
        let mut session = started.map_err(|e| e.to_string())?;
        let Some(events) = session.take_events() else {
            let _ = session.stop(Duration::from_millis(0)).await;
            return Err("driver returned a session without events".to_string());
        };
        match self.next_tenure(&req.seat, req.drv.name()) {
            Ok(tenure) => {
                self.record_process(&req.seat, session.process_id());
                Ok(Opened { session, events, tenure })
            }
            Err(e) => {
                let _ = session.stop(Duration::from_millis(0)).await;
                Err(e.to_string())
            }
        }
    }

    /// Keeps the agent's process on record for `sculptured stop`, which works without the server.
    fn record_process(&self, seat: &str, pid: Option<u32>) {
        let Some(pid) = pid else { return };
        let ident = crate::local_stop::identity(pid);
        if let Err(e) = self.inner.journal.lock().unwrap().record_process(seat, pid, ident.as_deref()) {
            tracing::warn!(error = %e, seat, "cannot record the agent's process");
        }
    }

    fn next_tenure(&self, seat: &str, driver: &str) -> rusqlite::Result<u32> {
        let mut j = self.inner.journal.lock().unwrap();
        let t = j.seat_tenure(seat)?;
        j.record_seat(seat, driver, None, t + 1)?;
        Ok(t + 1)
    }

    /// Makes the seat live. Its pump starts only once the seat is in the map, so an early exit
    /// finds the entry it removes.
    async fn go_live(&self, req: &Launch, opened: Opened, token: Option<String>) {
        let Opened { session, events, tenure } = opened;
        let gate = Arc::new(Mutex::new(ExitGate::default()));
        let ctx = PumpCtx { tenure, driver: req.drv.name().to_string(), caps: req.drv.capabilities(), ready: false, cause: req.cause };
        let (go_tx, go_rx) = oneshot::channel::<()>();
        let pump = tokio::spawn({
            let (this, seat, gate, token) = (self.clone(), req.seat.clone(), gate.clone(), token.clone());
            async move {
                let _ = go_rx.await;
                this.pump(seat, ctx, events, gate, token).await
            }
        });
        let (cancel, _) = watch::channel(false);
        let session: SharedSession = Arc::new(tokio::sync::Mutex::new(session));
        let live = LiveSeat { session, gate, cancel, tenure, pump, token };
        self.inner.seats.lock().await.insert(req.seat.clone(), Slot::Live(live));
        let _ = go_tx.send(());
    }

    /// Forwards one tenure's driver events until it exits.
    async fn pump(self, seat: String, mut ctx: PumpCtx, mut events: Events, gate: Arc<Mutex<ExitGate>>, token: Option<String>) {
        while let Some(ev) = events.next().await {
            if let DriverEvent::SessionReady { native_session_id } = &ev {
                let recorded =
                    self.inner.journal.lock().unwrap().record_seat(&seat, &ctx.driver, Some(native_session_id), ctx.tenure);
                if let Err(e) = recorded {
                    tracing::error!(error = %e, seat, "record_seat failed");
                }
            }
            if matches!(ev, DriverEvent::Exited { .. }) {
                self.on_exit(&seat, &mut ctx, ev, &gate).await;
                break;
            }
            for m in map_event(&mut ctx, &seat, ev, false) {
                self.emit(&seat, m);
            }
        }
        self.forget_token(&token);
    }

    async fn on_exit(&self, seat: &str, ctx: &mut PumpCtx, ev: DriverEvent, gate: &Mutex<ExitGate>) {
        {
            // Held across the emits so Stop cannot journal "Stopping" after them.
            let mut g = gate.lock().unwrap();
            g.exited = true;
            for m in map_event(ctx, seat, ev, g.stop_requested) {
                self.emit(seat, m);
            }
        }
        if let Err(e) = self.inner.journal.lock().unwrap().clear_process(seat) {
            tracing::warn!(error = %e, seat, "cannot clear the agent's process record");
        }
        let mut seats = self.inner.seats.lock().await;
        // A Stop owns removal of its own entry.
        if matches!(seats.get(seat), Some(Slot::Live(l)) if l.tenure == ctx.tenure) {
            seats.remove(seat);
        }
    }

    /// Stop never waits on a wedged send: it cancels in-flight commands (dropping their
    /// futures frees the session mutex), stops the process, then awaits the pump so the
    /// tenure's terminal events are journaled before returning.
    pub(super) async fn stop(&self, seat: String, grace_ms: u64, hard: bool) -> Result<(), String> {
        let LiveSeat { session, gate, cancel, mut pump, token, .. } = self.take_for_stop(&seat).await?;
        self.mark_stopping(&seat, &gate);
        let _ = cancel.send(true);
        let grace = if hard { drivers::HARD_STOP_FORCE_AFTER } else { Duration::from_millis(grace_ms) };
        let bound = grace + Duration::from_secs(5);
        let stop = async {
            let mut s = session.lock().await;
            if hard { s.hard_stop().await } else { s.stop(grace).await }
        };
        let Ok(stopped) = tokio::time::timeout(bound, stop).await else {
            pump.abort();
            self.attention(&seat, "stop timed out".to_string());
            self.release(&seat, &token).await;
            return Err("stop timed out".to_string());
        };
        let stopped = stopped.map_err(|e| e.to_string());
        if let Err(e) = &stopped {
            self.attention(&seat, format!("stop failed: {e}"));
        }
        if tokio::time::timeout(bound, &mut pump).await.is_err() {
            pump.abort();
            if stopped.is_ok() {
                self.attention(&seat, "stop: no exit event within grace".to_string());
            }
        }
        self.release(&seat, &token).await;
        stopped
    }

    /// Takes a live seat out of the map, leaving `Stopping` until its pump has drained.
    async fn take_for_stop(&self, seat: &str) -> Result<LiveSeat, String> {
        let mut seats = self.inner.seats.lock().await;
        match seats.get(seat) {
            Some(Slot::Live(_)) => {}
            Some(Slot::Launching) => return Err(format!("seat {seat} is still launching")),
            Some(Slot::Stopping) => return Err(format!("seat {seat} is already stopping")),
            None => return Err(format!("seat {seat} is not running")),
        }
        match seats.insert(seat.to_string(), Slot::Stopping) {
            Some(Slot::Live(l)) => Ok(l),
            _ => unreachable!("checked Live above"),
        }
    }

    /// Decides and journals "Stopping" atomically with respect to the pump's exit handling.
    fn mark_stopping(&self, seat: &str, gate: &Mutex<ExitGate>) {
        let mut g = gate.lock().unwrap();
        if !g.exited {
            g.stop_requested = true;
            self.emit_system(seat, EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Stopping, reason: None });
        }
    }

    async fn release(&self, seat: &str, token: &Option<String>) {
        self.inner.seats.lock().await.remove(seat);
        self.forget_token(token);
    }
}
