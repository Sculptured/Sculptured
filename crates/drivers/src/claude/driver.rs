use super::{
    Arc, AsyncBufReadExt, AsyncWriteExt, BoxStream, BufReader, Caps, Child, ChildStdin,
    ClaudeParser, Driver, DriverError, DriverEvent, Duration, LaunchSpec, Mutex, OutboundMessage,
    PathBuf, Session, StderrTail, StreamExt, UnboundedReceiverStream, Value, async_trait, json,
    mpsc, oneshot, process, watch,
};

pub struct ClaudeStreamDriver {
    pub program: PathBuf,
}

impl Default for ClaudeStreamDriver {
    fn default() -> Self {
        Self {
            program: PathBuf::from("claude"),
        }
    }
}

impl ClaudeStreamDriver {
    pub(super) async fn spawn(
        &self,
        spec: LaunchSpec,
        resume: Option<&str>,
    ) -> Result<Box<dyn Session>, DriverError> {
        crate::claude::check_cwd(&spec)?;
        let session_id = resume
            .map(str::to_string)
            .or_else(|| spec.native_session_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let mut child = self.start_child(&spec, resume, &session_id)?;
        let stdin = child.stdin.take();
        let pid = child.id();
        let parser = Arc::new(Mutex::new(
            spec.usage_baseline
                .map(ClaudeParser::with_usage_baseline)
                .unwrap_or_default(),
        ));
        let (tx, rx) = mpsc::unbounded_channel();
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let (exited_tx, exited_rx) = watch::channel(false);
        let _ = tx.send(DriverEvent::SessionReady {
            native_session_id: session_id,
        });
        tokio::spawn(run_output(child, parser.clone(), tx, kill_rx, exited_tx));
        Ok(Box::new(ClaudeSession {
            stdin,
            parser,
            events: Some(UnboundedReceiverStream::new(rx).boxed()),
            kill: Some(kill_tx),
            exited: exited_rx,
            interrupt_seq: 0,
            pid,
        }))
    }

    /// Writes the seat-tools config file (if any) and starts `claude`.
    pub(super) fn start_child(
        &self,
        spec: &LaunchSpec,
        resume: Option<&str>,
        session_id: &str,
    ) -> Result<Child, DriverError> {
        if let Some(t) = &spec.seat_tools
            && let Some(path) = &t.config_file
        {
            write_private(path, &Self::mcp_config(t))?;
        }
        if let Some(path) = spec
            .remote_mcp_file
            .as_ref()
            .filter(|_| !spec.remote_mcp.is_empty())
        {
            write_private(path, &Self::remote_mcp_config(&spec.remote_mcp))?;
        }
        let program = spec.program.clone().unwrap_or_else(|| self.program.clone());
        let mut cmd = process::command(&program, spec, Self::args(spec, resume, session_id));
        if spec.seat_tools.is_some() {
            // A request_input or approve call may wait for the whole question timeout.
            cmd.env("MCP_TOOL_TIMEOUT", crate::MCP_TOOL_TIMEOUT_MS);
        }
        if let Some((k, v)) = Self::protection_env(spec) {
            cmd.env(k, v);
        }
        process::spawn(&mut cmd, &program)
    }
}

/// Feeds stdout to the parser until it ends or Stop's kill fires, then reaps the child (still
/// honouring the kill) and sends `Exited`.
pub(super) async fn run_output(
    mut child: Child,
    parser: Arc<Mutex<ClaudeParser>>,
    tx: mpsc::UnboundedSender<DriverEvent>,
    mut kill_rx: oneshot::Receiver<()>,
    exited_tx: watch::Sender<bool>,
) {
    let stderr = StderrTail::start(child.stderr.take().expect("piped stderr"));
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let killed = process::pump_lines(&mut lines, &mut kill_rx, |l| {
        let matching_parser = parser
            .lock()
            .expect("parser mutex must not be poisoned")
            .parse_line(&l);
        for e in matching_parser {
            let _ = tx.send(e);
        }
    })
    .await;
    if killed {
        process::kill_group(&child);
    }
    let status = process::reap(&mut child, &mut kill_rx, killed).await;
    let stderr = stderr.finish().await;
    let _ = tx.send(process::exited(status, &stderr));
    let _ = exited_tx.send(true);
}

#[async_trait]
impl Driver for ClaudeStreamDriver {
    fn name(&self) -> &'static str {
        "claude-stream"
    }
    fn capabilities(&self) -> Caps {
        // M0 Caps table: fork was not probed, questions are unknown, and a mid-turn
        // message queues instead of steering.
        Caps {
            interrupt: true,
            resume: true,
            usage: true,
            subagent_events: true,
            fork: false,
            questions: false,
            mid_turn_input: false,
        }
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        self.spawn(spec, None).await
    }
    async fn resume(
        &self,
        spec: LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn Session>, DriverError> {
        self.spawn(spec, Some(native_id)).await
    }
}

pub(super) struct ClaudeSession {
    pub(super) stdin: Option<ChildStdin>,
    pub(super) parser: Arc<Mutex<ClaudeParser>>,
    pub(super) events: Option<BoxStream<'static, DriverEvent>>,
    pub(super) kill: Option<oneshot::Sender<()>>,
    pub(super) exited: watch::Receiver<bool>,
    pub(super) interrupt_seq: u32,
    /// The child's pid, which leads the process group the driver made for it.
    pub(super) pid: Option<u32>,
}

impl ClaudeSession {
    pub(super) async fn write_line(&mut self, v: Value) -> Result<(), DriverError> {
        let stdin = self.stdin.as_mut().ok_or(DriverError::Closed)?;
        stdin
            .write_all(format!("{v}\n").as_bytes())
            .await
            .map_err(|_| DriverError::Closed)?;
        stdin.flush().await.map_err(|_| DriverError::Closed)
    }
}

#[async_trait]
impl Session for ClaudeSession {
    fn process_id(&self) -> Option<u32> {
        self.pid
    }

    async fn send(&mut self, msg: OutboundMessage) -> Result<(), DriverError> {
        if *self.exited.borrow() {
            return Err(DriverError::Closed);
        }
        self.parser
            .lock()
            .expect("parser mutex must not be poisoned")
            .on_sent(msg.message_id);
        self.write_line(json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": msg.text }] }
        }))
        .await
    }

    async fn interrupt(&mut self) -> Result<(), DriverError> {
        self.interrupt_seq += 1;
        self.parser
            .lock()
            .expect("parser mutex must not be poisoned")
            .on_interrupt();
        let id = format!("interrupt-{}", self.interrupt_seq);
        self.write_line(json!({ "type": "control_request", "request_id": id, "request": { "subtype": "interrupt" } }))
            .await
    }

    async fn stop(&mut self, grace: Duration) -> Result<(), DriverError> {
        self.stdin.take(); // EOF: `claude -p` finishes and exits
        process::stop_within(grace, &self.exited, &mut self.kill).await;
        Ok(())
    }

    async fn hard_stop(&mut self) -> Result<(), DriverError> {
        process::hard_stop(self.pid, &self.exited, &mut self.kill).await;
        self.stdin.take();
        Ok(())
    }

    fn take_events(&mut self) -> Option<BoxStream<'static, DriverEvent>> {
        self.events.take()
    }
}

/// Windows: files under the user's profile are readable by that user only (its default access rules).
#[cfg(windows)]
pub(super) fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

/// Writes `text` to `path`, readable by this user only (0600), replacing what was there.
#[cfg(unix)]
pub(super) fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // `mode` applies only on create; an existing file is narrowed too.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(text.as_bytes())
}
