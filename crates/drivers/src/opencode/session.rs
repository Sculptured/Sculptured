//! One `opencode acp` process: the handshake, the stdin writer, the stdout reader and the session.
use super::config;
use super::core::Core;
use super::parser::{AcpParser, current_model, provider_of};
use super::{Open, OpencodeAcpDriver};
use crate::process::{self, StderrTail};
use crate::{DriverError, DriverEvent, LaunchSpec, OutboundMessage, Session};
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use model::Answer;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::wrappers::UnboundedReceiverStream;

/// How long the handshake may take: OpenCode loads its config and providers before answering
/// `session/new` (about 0.3 s recorded, longer on a first run that installs a provider package).
const STARTUP: Duration = Duration::from_secs(90);
/// The ACP version this driver speaks.
const PROTOCOL: u64 = 1;

type StdoutLines = Lines<BufReader<ChildStdout>>;
type Writer = mpsc::UnboundedSender<Option<String>>;

fn startup_error(msg: impl std::fmt::Display) -> DriverError {
    DriverError::Io(std::io::Error::other(format!(
        "opencode acp did not start: {msg}"
    )))
}

async fn write_line(stdin: &mut ChildStdin, v: &Value) -> Result<(), String> {
    stdin
        .write_all(format!("{v}\n").as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

/// What the handshake reads and writes, and the events that arrive before `SessionReady`.
struct Handshake<'a> {
    stdin: &'a mut ChildStdin,
    lines: &'a mut StdoutLines,
    parser: &'a mut AcpParser,
    early: &'a mut Vec<DriverEvent>,
}

/// What the handshake learnt: the session id and the model OpenCode says it uses.
struct Opened {
    session: String,
    model: Option<String>,
}

impl Handshake<'_> {
    /// Sends request `id` and reads until its response.
    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value, String> {
        write_line(
            self.stdin,
            &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
        .await?;
        self.response(id, method).await
    }

    /// Reads until the response to request `id`; lines before it go through the parser.
    async fn response(&mut self, id: u64, method: &str) -> Result<Value, String> {
        loop {
            let line = self
                .lines
                .next_line()
                .await
                .map_err(|e| e.to_string())?
                .ok_or("it exited")?;
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && v["id"].as_u64() == Some(id)
                && v.get("method").is_none()
            {
                return match v.get("error") {
                    Some(e) => Err(format!(
                        "{method}: {}",
                        e["message"].as_str().unwrap_or("an error")
                    )),
                    None => Ok(v["result"].clone()),
                };
            }
            let parsed = self.parser.parse_line(&line);
            for r in &parsed.replies {
                write_line(self.stdin, r).await?;
            }
            self.early.extend(parsed.events);
        }
    }

    /// `initialize`: no file system or terminal offered; the agent must speak `PROTOCOL`.
    async fn initialize(&mut self) -> Result<(), String> {
        let init = self.request(1, "initialize", json!({ "protocolVersion": PROTOCOL,
            "clientCapabilities": { "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false },
            "clientInfo": { "name": "sculptured", "version": env!("CARGO_PKG_VERSION") } })).await?;
        match init["protocolVersion"].as_u64() {
            Some(PROTOCOL) => Ok(()),
            _ => Err(format!(
                "it speaks ACP version {}, and Sculptured speaks {PROTOCOL}",
                init["protocolVersion"]
            )),
        }
    }

    /// The agent's model, chosen for `session`; the model OpenCode then says it uses.
    async fn choose_model(&mut self, session: &str, m: &str) -> Result<Option<String>, String> {
        let set = json!({ "sessionId": session, "configId": "model", "value": m });
        let r = self.request(3, "session/set_config_option", set).await
            .map_err(|e| format!("OpenCode could not use the model {m} ({e}); check its name, like openrouter/deepseek/deepseek-chat-v3.1, and the key"))?;
        Ok(current_model(&r["configOptions"]))
    }

    async fn run(mut self, spec: &LaunchSpec, open: Open<'_>) -> Result<Opened, String> {
        self.initialize().await?;
        let mut params = json!({ "cwd": spec.cwd, "mcpServers": config::mcp_servers(spec) });
        let (method, resumed) = match open {
            Open::Start => ("session/new", None),
            Open::Resume(id) => ("session/resume", Some(id.to_string())),
        };
        if let Some(id) = &resumed {
            params["sessionId"] = json!(id);
        }
        let r = self.request(2, method, params).await?;
        let session = resumed
            .or_else(|| r["sessionId"].as_str().map(str::to_string))
            .ok_or("no session id")?;
        let model = match &spec.model {
            Some(m) => self.choose_model(&session, m).await?,
            None => current_model(&r["configOptions"]),
        };
        Ok(Opened { session, model })
    }
}

/// Ends a launch that failed, with the group (MCP servers OpenCode started) and stderr's last line.
async fn fail_startup(mut child: Child, stderr: StderrTail, why: String) -> DriverError {
    process::kill_group(&child);
    let _ = child.start_kill();
    let _ = child.wait().await;
    let bytes = stderr.finish().await;
    let text = String::from_utf8_lossy(&bytes);
    let last = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    startup_error(if last.is_empty() {
        why
    } else {
        format!("{why}: {last}")
    })
}

/// What the session reports first: the model it asked for and the one OpenCode says it uses.
fn model_observed(spec: &LaunchSpec, observed: Option<String>) -> DriverEvent {
    DriverEvent::ModelObserved {
        requested: spec.model.clone(),
        provider: observed.as_deref().and_then(provider_of),
        observed,
        effort: None,
        rerouted_from: None,
        reason: None,
    }
}

impl OpencodeAcpDriver {
    /// `opencode acp` in the agent's folder, with Sculptured's config added to its environment.
    fn start(
        &self,
        spec: &LaunchSpec,
    ) -> Result<(Child, ChildStdin, StdoutLines, StderrTail), DriverError> {
        crate::check_cwd(spec)?;
        let program = spec.program.clone().unwrap_or_else(|| self.program.clone());
        let args = std::iter::once("acp".to_string()).chain(spec.extra_args.iter().cloned());
        let mut cmd = process::command(&program, spec, args);
        cmd.envs(config::env(spec));
        let mut child = process::spawn(&mut cmd, &program)?;
        let stdin = child.stdin.take().expect("piped stdin");
        let lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
        let stderr = StderrTail::start(child.stderr.take().expect("piped stderr"));
        Ok((child, stdin, lines, stderr))
    }

    pub(super) async fn spawn(
        &self,
        spec: LaunchSpec,
        open: Open<'_>,
    ) -> Result<Box<dyn Session>, DriverError> {
        let (child, mut stdin, mut lines, stderr) = self.start(&spec)?;
        let mut parser = AcpParser::new(matches!(open, Open::Start));
        let mut early = Vec::new();
        let handshake = Handshake {
            stdin: &mut stdin,
            lines: &mut lines,
            parser: &mut parser,
            early: &mut early,
        }
        .run(&spec, open);
        let opened = match tokio::time::timeout(STARTUP, handshake).await {
            Ok(Ok(o)) => o,
            Ok(Err(why)) => return Err(fail_startup(child, stderr, why).await),
            Err(_) => {
                let why = format!("no answer within {}s", STARTUP.as_secs());
                return Err(fail_startup(child, stderr, why).await);
            }
        };
        early.insert(0, model_observed(&spec, opened.model));
        let running = Running {
            child,
            lines,
            stderr,
        };
        Ok(Box::new(AcpSession::start(
            running,
            spawn_writer(stdin),
            parser,
            opened.session,
            early,
        )))
    }
}

/// One writer owns stdin; `None` closes it (how `stop` asks OpenCode to end).
fn spawn_writer(stdin: ChildStdin) -> Writer {
    let (tx, mut rx) = mpsc::unbounded_channel::<Option<String>>();
    tokio::spawn(async move {
        let mut stdin = Some(stdin);
        while let Some(item) = rx.recv().await {
            match (item, stdin.as_mut()) {
                (Some(line), Some(s)) => {
                    let ok = s.write_all(format!("{line}\n").as_bytes()).await.is_ok()
                        && s.flush().await.is_ok();
                    if !ok {
                        stdin = None;
                    }
                }
                (None, _) => stdin = None,
                (Some(_), None) => {}
            }
        }
    });
    tx
}

/// A started process, its output and its stderr tail.
struct Running {
    child: Child,
    lines: StdoutLines,
    stderr: StderrTail,
}

/// The stdout reader's handles.
struct Reader {
    core: Arc<Mutex<Core>>,
    write: Writer,
    tx: mpsc::UnboundedSender<DriverEvent>,
}

impl Reader {
    fn on_line(&self, line: &str) {
        let out = self
            .core
            .lock()
            .expect("core mutex must not be poisoned")
            .on_line(line);
        for e in out.events {
            let _ = self.tx.send(e);
        }
        for w in out.writes {
            let _ = self.write.send(Some(w));
        }
    }

    async fn run(
        self,
        mut p: Running,
        mut kill_rx: oneshot::Receiver<()>,
        exited_tx: watch::Sender<bool>,
    ) {
        let killed = process::pump_lines(&mut p.lines, &mut kill_rx, |l| self.on_line(&l)).await;
        // Output ended or the grace ran out: end the group (commands, MCP servers) before reaping.
        process::kill_group(&p.child);
        let status = process::reap(&mut p.child, &mut kill_rx, killed).await;
        let stderr = p.stderr.finish().await;
        let _ = exited_tx.send(true);
        let _ = self.tx.send(process::exited(status, &stderr));
    }
}

struct AcpSession {
    core: Arc<Mutex<Core>>,
    write: Writer,
    events: Option<BoxStream<'static, DriverEvent>>,
    kill: Option<oneshot::Sender<()>>,
    exited: watch::Receiver<bool>,
    /// The event channel, for events the session itself adds; weak, so the stream ends after `Exited`.
    notes: mpsc::WeakUnboundedSender<DriverEvent>,
    pid: Option<u32>,
}

impl AcpSession {
    fn start(
        p: Running,
        write: Writer,
        parser: AcpParser,
        session: String,
        early: Vec<DriverEvent>,
    ) -> Self {
        let core = Arc::new(Mutex::new(Core::new(parser, session.clone(), 3)));
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(DriverEvent::SessionReady {
            native_session_id: session,
        });
        for e in early {
            let _ = tx.send(e);
        }
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let (exited_tx, exited_rx) = watch::channel(false);
        let notes = tx.downgrade();
        let pid = p.child.id();
        let reader = Reader {
            core: core.clone(),
            write: write.clone(),
            tx,
        };
        tokio::spawn(reader.run(p, kill_rx, exited_tx));
        Self {
            core,
            write,
            events: Some(UnboundedReceiverStream::new(rx).boxed()),
            kill: Some(kill_tx),
            exited: exited_rx,
            notes,
            pid,
        }
    }

    /// Writes the lines and emits the events of one call, unless the session has ended.
    fn apply(&self, out: super::core::Out) -> Result<(), DriverError> {
        if *self.exited.borrow() {
            return Err(DriverError::Closed);
        }
        if let Some(tx) = self.notes.upgrade() {
            for e in out.events {
                let _ = tx.send(e);
            }
        }
        for w in out.writes {
            self.write.send(Some(w)).map_err(|_| DriverError::Closed)?;
        }
        Ok(())
    }

    fn core(&self) -> std::sync::MutexGuard<'_, Core> {
        self.core.lock().expect("core mutex must not be poisoned")
    }
}

#[async_trait]
impl Session for AcpSession {
    fn process_id(&self) -> Option<u32> {
        self.pid
    }

    async fn send(&mut self, msg: OutboundMessage) -> Result<(), DriverError> {
        if *self.exited.borrow() {
            return Err(DriverError::Closed);
        }
        let out = self.core().send(msg);
        self.apply(out)
    }

    async fn interrupt(&mut self) -> Result<(), DriverError> {
        let out = self.core().interrupt();
        self.apply(out)
    }

    async fn stop(&mut self, grace: Duration) -> Result<(), DriverError> {
        let _ = self.write.send(None);
        process::stop_within(grace, &self.exited, &mut self.kill).await;
        Ok(())
    }

    async fn hard_stop(&mut self) -> Result<(), DriverError> {
        process::hard_stop(self.pid, &self.exited, &mut self.kill).await;
        let _ = self.write.send(None);
        Ok(())
    }

    async fn answer(&mut self, question_id: &str, answer: Answer) -> Result<(), DriverError> {
        let reply = self
            .core()
            .parser
            .answer_reply(question_id, &answer)
            .ok_or(DriverError::Unsupported(
                "an answer to a question this agent is not asking",
            ))?;
        self.apply(super::core::Out {
            events: vec![DriverEvent::QuestionResolved {
                question_id: question_id.to_string(),
            }],
            writes: vec![reply.to_string()],
        })
    }

    fn take_events(&mut self) -> Option<BoxStream<'static, DriverEvent>> {
        self.events.take()
    }
}
