use super::{
    Actor, Arc, Driver, DriverError, DriverInfo, EventBody, FromHost, Future, Grant, HashMap,
    HostCommand, HostEvent, JoinHandle, Journal, LaunchMode, Mapped, Mutex, OutboundMessage,
    PendingToolCall, Provenance, SeatLifecycle, SeatToolsHost, Session, Subject, TenureCause,
    broadcast, watch,
};

pub(super) type SharedSession = Arc<tokio::sync::Mutex<Box<dyn Session>>>;

/// Shared by Stop and the pump so "Stopping" is never journaled after the
/// tenure's terminal lifecycle event.
#[derive(Default)]
pub(super) struct ExitGate {
    pub(super) stop_requested: bool,
    pub(super) exited: bool,
}

pub(super) struct LiveSeat {
    pub(super) session: SharedSession,
    pub(super) gate: Arc<Mutex<ExitGate>>,
    /// Flipped to true by Stop; in-flight send/interrupt futures are dropped so the
    /// session mutex frees up even when the agent is wedged.
    pub(super) cancel: watch::Sender<bool>,
    pub(super) tenure: u32,
    pub(super) pump: JoinHandle<()>,
    /// This tenure's seat-tools token, if it has one; forgotten when the process ends.
    pub(super) token: Option<String>,
}

pub(super) enum Slot {
    /// Id reserved while the driver launches (no map lock is held across that).
    Launching,
    Live(LiveSeat),
    /// Stop in progress; the entry stays until the old pump has drained.
    Stopping,
}

pub(super) struct Inner {
    pub(super) host_id: String,
    /// Connected computers: what the owner allows (`runner.json`). None for a local `serve`.
    pub(super) policy: Option<crate::policy::RunnerPolicy>,
    /// Local operations this runner answers (`ToHost::Op`). None: it answers none.
    pub(super) ops: Option<crate::ops::OpsHost>,
    pub(super) drivers: HashMap<String, Arc<dyn Driver>>,
    pub(super) seats: tokio::sync::Mutex<HashMap<String, Slot>>,
    pub(super) journal: Mutex<Journal>,
    pub(super) out: broadcast::Sender<HostEvent>,
    pub(super) seat_tools: Option<SeatToolsHost>,
    /// Seat-tools token → what it grants, while that seat's process runs.
    pub(super) tokens: Mutex<HashMap<String, Grant>>,
    /// Seat-tool calls on their way to controld; the link subscribes and forwards them.
    pub(super) tool_out: broadcast::Sender<FromHost>,
    /// Calls waiting for controld's reply, by call id: the call (sent again on every new link) and its waiter.
    pub(super) tool_pending: Mutex<HashMap<String, PendingToolCall>>,
}

/// Owns the live seat sessions on this host. Every event goes to the journal
/// first (durable), then to the broadcast in host_seq order (best-effort; the
/// link replays gaps from the journal).
#[derive(Clone)]
pub struct Supervisor {
    pub(super) inner: Arc<Inner>,
}

impl Supervisor {
    pub fn new(host_id: String, drivers: Vec<Arc<dyn Driver>>, journal: Journal) -> Supervisor {
        Self::with_seat_tools(host_id, drivers, journal, None)
    }

    /// A supervisor whose agents can reach Sculptured's tools through `seat_tools` (when Some).
    pub fn with_seat_tools(
        host_id: String,
        drivers: Vec<Arc<dyn Driver>>,
        journal: Journal,
        seat_tools: Option<SeatToolsHost>,
    ) -> Supervisor {
        if let Some(host) = &seat_tools {
            crate::supervisor::sweep_config_files(host);
        }
        let (out, _) = broadcast::channel(1024);
        let drivers = drivers
            .into_iter()
            .map(|d| (d.name().to_string(), d))
            .collect();
        Supervisor {
            inner: Arc::new(Inner {
                host_id,
                policy: None,
                ops: None,
                drivers,
                seats: tokio::sync::Mutex::new(HashMap::new()),
                journal: Mutex::new(journal),
                out,
                seat_tools,
                tokens: Mutex::default(),
                tool_out: broadcast::channel(1024).0,
                tool_pending: Mutex::default(),
            }),
        }
    }

    /// Enforces `policy` on every launch (connected computers). Call before the supervisor is shared.
    ///
    /// # Panics
    /// Panics if another clone of this supervisor already exists.
    #[must_use]
    pub fn restricted(mut self, policy: crate::policy::RunnerPolicy) -> Supervisor {
        Arc::get_mut(&mut self.inner)
            .expect("restrict before sharing the supervisor")
            .policy = Some(policy);
        self
    }

    /// Answers local operations with `ops`. Call before the supervisor is shared.
    ///
    /// # Panics
    /// Panics if another clone of this supervisor already exists.
    #[must_use]
    pub fn with_ops(mut self, ops: crate::ops::OpsHost) -> Supervisor {
        Arc::get_mut(&mut self.inner)
            .expect("set ops before sharing the supervisor")
            .ops = Some(ops);
        self
    }

    /// Runs one local operation on this computer.
    pub async fn run_op(
        &self,
        op: localops::LocalOp,
    ) -> Result<serde_json::Value, localops::LocalError> {
        let Some(ops) = &self.inner.ops else {
            return Err(localops::LocalError::conflict(
                "needs_computer",
                "This runner does not do local operations",
            ));
        };
        let Some(pol) = &self.inner.policy else {
            return localops::execute(&ops.context, op).await;
        };
        let op = pol.check_op(&op)?;
        match op {
            localops::LocalOp::Describe => {
                Ok(serde_json::to_value(pol.info()).expect("serializes"))
            }
            // The top of a connected computer is its allowed folders.
            localops::LocalOp::ListFolder { path: None, .. } => Ok(serde_json::json!({
                "path": "", "parent": null, "home": "", "truncated": false,
                "entries": pol.allowed_folders.iter().map(|f| serde_json::json!({"name": f.display().to_string(), "path": f.display().to_string()})).collect::<Vec<_>>(),
            })),
            // Browse within the allowed folder that holds the path: it acts as home, so nothing above it shows.
            localops::LocalOp::ListFolder {
                path: Some(ref p), ..
            }
            | localops::LocalOp::CreateFolder { parent: ref p, .. } => {
                let mut cx = ops.context.clone();
                cx.home = pol
                    .root_of(std::path::Path::new(p))
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or(cx.home);
                localops::execute(&cx, op).await
            }
            op => localops::execute(&ops.context, op).await,
        }
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<HostEvent> {
        self.inner.out.subscribe()
    }

    /// This computer's rules, for the Hello (None for a local `serve`'s runner).
    #[must_use]
    pub fn policy_info(&self) -> Option<host_proto::HostPolicyInfo> {
        self.inner.policy.as_ref().map(|p| p.info())
    }

    #[must_use]
    pub fn host_id(&self) -> &str {
        &self.inner.host_id
    }

    #[must_use]
    pub fn driver_infos(&self) -> Vec<DriverInfo> {
        let mut v: Vec<DriverInfo> = self
            .inner
            .drivers
            .values()
            .map(|d| DriverInfo {
                name: d.name().to_string(),
                caps: d.capabilities(),
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Seats with a process or one starting: a reconnect during a launch must not mark them lost.
    /// The agent processes on record for `sculptured stop`: (seat, pid, identity).
    #[must_use]
    pub fn recorded_processes(&self) -> Vec<(String, u32, Option<String>)> {
        self.inner
            .journal
            .lock()
            .expect("journal mutex must not be poisoned")
            .processes()
            .unwrap_or_default()
    }

    pub async fn live_seats(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .inner
            .seats
            .lock()
            .await
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Live(_) | Slot::Launching))
            .map(|(k, _)| k.clone())
            .collect();
        v.sort();
        v
    }

    /// Stops every running seat at once (the runner is shutting down); returns how many it stopped.
    pub async fn stop_all(&self, grace_ms: u64) -> usize {
        let running: Vec<String> = self
            .inner
            .seats
            .lock()
            .await
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Live(_)))
            .map(|(k, _)| k.clone())
            .collect();
        let mut stops = tokio::task::JoinSet::new();
        for seat in running {
            let sup = self.clone();
            stops.spawn(async move { sup.stop(seat, grace_ms, false).await.is_ok() });
        }
        stops.join_all().await.into_iter().filter(|ok| *ok).count()
    }

    /// Seats whose process is still starting (a subset of `live_seats`).
    pub async fn launching_seats(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .inner
            .seats
            .lock()
            .await
            .iter()
            .filter(|(_, slot)| matches!(slot, Slot::Launching))
            .map(|(k, _)| k.clone())
            .collect();
        v.sort();
        v
    }

    /// What this hostd can do beyond M1, for its Hello. Seat tools only when it serves their socket.
    pub fn features(&self) -> Vec<String> {
        use host_proto::features::{
            ANSWER, CLAUDE_SETUP, POSTURE, RESTART, SEAT_TOOLS, USAGE_BASELINE,
        };
        let mut f = vec![POSTURE, RESTART, ANSWER, USAGE_BASELINE, CLAUDE_SETUP];
        if self.inner.seat_tools.is_some() {
            f.push(SEAT_TOOLS);
        }
        if self.guard_program().is_some() {
            f.push(host_proto::features::PROTECTION);
        }
        if self.inner.seat_tools.is_some() {
            f.push(host_proto::features::SKILLS);
        }
        f.push(host_proto::features::INTEGRATIONS);
        f.push(host_proto::features::COMPONENTS);
        f.push(host_proto::features::COMPONENT_KINDS);
        if self.inner.ops.is_some() {
            f.push(host_proto::features::LOCAL_OPS);
            f.push(host_proto::features::USAGE_REPORTS);
        }
        f.into_iter().map(String::from).collect()
    }

    pub fn unacked_after(&self, after: u64) -> rusqlite::Result<Vec<HostEvent>> {
        self.inner
            .journal
            .lock()
            .expect("journal mutex must not be poisoned")
            .unacked_after(after)
    }

    /// Makes sure the next event is numbered after `resume_after` (controld's cursor).
    /// Returns the journal's previous last host_seq when it had to fast-forward.
    pub fn resume_after(&self, resume_after: u64) -> rusqlite::Result<Option<u64>> {
        let mut journal = self
            .inner
            .journal
            .lock()
            .expect("journal mutex must not be poisoned");
        let last = journal.last_seq();
        if resume_after <= last {
            return Ok(None);
        }
        journal.fast_forward(resume_after + 1)?;
        Ok(Some(last))
    }

    pub fn ack(&self, upto: u64) -> rusqlite::Result<()> {
        self.inner
            .journal
            .lock()
            .expect("journal mutex must not be poisoned")
            .ack(upto)
    }

    pub(super) fn emit(&self, seat: &str, m: Mapped) {
        // Broadcast while still holding the journal guard. Two emitters (pump tasks, or
        // `handle`'s MessageDelivered next to a pump) then broadcast in host_seq order,
        // and the link never skips an event. `broadcast::Sender::send` never blocks.
        let mut journal = self
            .inner
            .journal
            .lock()
            .expect("journal mutex must not be poisoned");
        match journal.append(
            crate::supervisor::now_ms(),
            m.actor,
            Subject::Seat(seat.to_string()),
            m.provenance,
            m.body,
        ) {
            Ok(ev) => {
                let _ = self.inner.out.send(ev);
            }
            Err(e) => tracing::error!(error = %e, seat, "journal append failed; event dropped"),
        }
    }

    pub(super) fn emit_system(&self, seat: &str, body: EventBody) {
        self.emit(
            seat,
            Mapped {
                actor: Actor::System("hostd".into()),
                provenance: Provenance::ProtocolObserved,
                body,
            },
        );
    }

    /// Looks up a live seat and clones what a command needs, releasing the map lock.
    pub(super) async fn live(
        &self,
        seat: &str,
    ) -> Result<(SharedSession, watch::Receiver<bool>), String> {
        match self.inner.seats.lock().await.get(seat) {
            Some(Slot::Live(l)) => Ok((l.session.clone(), l.cancel.subscribe())),
            Some(Slot::Launching) => Err(format!("seat {seat} is still launching")),
            Some(Slot::Stopping) => Err(format!("seat {seat} is stopping")),
            None => Err(format!("seat {seat} is not running")),
        }
    }

    pub async fn handle(&self, cmd: HostCommand) -> Result<(), String> {
        match cmd {
            HostCommand::Launch {
                seat,
                driver,
                params,
            } => {
                self.launch(seat, driver, params, LaunchMode::Fresh, TenureCause::Fresh)
                    .await
            }
            HostCommand::Restart {
                seat,
                driver,
                params,
                mode,
                cause,
            } => self.launch(seat, driver, params, mode, cause).await,
            HostCommand::Send {
                seat,
                message_id,
                intent,
                text,
            } => {
                let msg = OutboundMessage {
                    message_id: message_id.clone(),
                    intent,
                    text,
                };
                self.on_session(&seat, |s| async move { s.lock().await.send(msg).await })
                    .await?;
                self.emit_system(&seat, EventBody::MessageDelivered { message_id });
                Ok(())
            }
            HostCommand::Interrupt { seat } => {
                self.on_session(&seat, |s| async move { s.lock().await.interrupt().await })
                    .await
            }
            HostCommand::Stop {
                seat,
                grace_ms,
                hard,
            } => self.stop(seat, grace_ms, hard).await,
            HostCommand::Answer {
                seat,
                question_id,
                answer,
            } => {
                self.on_session(&seat, |s| async move {
                    s.lock().await.answer(&question_id, answer).await
                })
                .await
            }
            HostCommand::RunCheck {
                seat,
                command,
                cwd,
                own_copy,
            } => {
                let mut cwd = match &self.inner.policy {
                    Some(pol) if !pol.caps().checks => {
                        return Err("Checks are turned off on this computer (sculptured connect … --enable checks)".into());
                    }
                    Some(pol) => pol
                        .resolve(&cwd)
                        .map_err(|e| e.message)?
                        .display()
                        .to_string(),
                    None => cwd,
                };
                // The agent works in its own copy, so its check runs there (the same copy its launch made).
                if let Some(own) = own_copy {
                    let ops = self
                        .inner
                        .ops
                        .as_ref()
                        .ok_or("this runner cannot find agents' own copies")?;
                    if let Some(pol) = &self.inner.policy {
                        cwd = pol.whole_repo(cwd).map_err(|e| e.message)?;
                    }
                    cwd = localops::copies::prepare(
                        &ops.context.copies_root,
                        &own.team,
                        &seat,
                        own.parent.as_deref(),
                        std::path::Path::new(&cwd),
                    )
                    .map_err(|e| e.message)?
                    .display()
                    .to_string();
                }
                crate::ops::queue_check(self.clone(), seat, command, cwd);
                Ok(())
            }
        }
    }

    /// Records a check's progress for `seat`: Sculptured ran the command itself (verified evidence).
    pub(crate) fn emit_check(&self, seat: &str, body: EventBody) {
        self.emit(
            seat,
            Mapped {
                actor: Actor::System("check".into()),
                provenance: Provenance::Verified,
                body,
            },
        );
    }

    /// Runs `op` on a live seat's session. Stop cancels it: dropping the future frees the session
    /// mutex even when the agent is wedged.
    pub(super) async fn on_session<F, Fut>(&self, seat: &str, op: F) -> Result<(), String>
    where
        F: FnOnce(SharedSession) -> Fut,
        Fut: Future<Output = Result<(), DriverError>>,
    {
        let (session, mut cancel) = self.live(seat).await?;
        tokio::select! {
            r = op(session) => r.map_err(|e| e.to_string()),
            _ = cancel.wait_for(|c| *c) => Err(format!("seat {seat} is stopping")),
        }
    }

    /// Journals that the seat needs attention, and why.
    pub(super) fn attention(&self, seat: &str, reason: String) {
        self.emit_system(
            seat,
            EventBody::SeatLifecycleChanged {
                lifecycle: SeatLifecycle::Attention,
                reason: Some(reason),
            },
        );
    }
}
