//! One `codex app-server` process: the handshake, the stdin writer, the stdout reader and the session.
use super::core::Core;
use super::parser::CodexParser;
use super::{CodexAppServerDriver, Open};
use crate::process::{self, StderrTail};
use crate::{check_cwd, DriverError, DriverEvent, LaunchSpec, OutboundMessage, Session};
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use model::Answer;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::wrappers::UnboundedReceiverStream;

/// How long `initialize` and the thread request may take before the launch fails.
const STARTUP: Duration = Duration::from_secs(30);

type StdoutLines = Lines<BufReader<ChildStdout>>;
type Writer = mpsc::UnboundedSender<Option<String>>;

fn startup_error(msg: impl std::fmt::Display) -> DriverError {
    DriverError::Io(std::io::Error::other(format!("codex app-server did not start: {msg}")))
}

async fn write_line(stdin: &mut ChildStdin, v: &Value) -> Result<(), String> {
    stdin.write_all(format!("{v}\n").as_bytes()).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())
}

/// What the handshake reads and writes: the server's stdin and stdout, the parser, and the
/// events that arrive before `SessionReady`.
struct Handshake<'a> {
    stdin: &'a mut ChildStdin,
    lines: &'a mut StdoutLines,
    parser: &'a mut CodexParser,
    early: &'a mut Vec<DriverEvent>,
}

impl Handshake<'_> {
    /// Reads until the response to request `want`. Lines before it go through the parser: its
    /// replies are written at once and its events are kept for after `SessionReady`.
    async fn await_response(&mut self, want: u64) -> Result<Value, String> {
        loop {
            let line = self.lines.next_line().await.map_err(|e| e.to_string())?.ok_or_else(|| "it exited".to_string())?;
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && v["id"].as_u64() == Some(want)
                && v.get("method").is_none()
            {
                return match v.get("error") {
                    Some(e) => Err(e["message"].as_str().unwrap_or("it answered with an error").to_string()),
                    None => Ok(v),
                };
            }
            let parsed = self.parser.parse_line(&line);
            for r in &parsed.replies {
                write_line(self.stdin, r).await?;
            }
            self.early.extend(parsed.events);
        }
    }

    /// `initialize`, `initialized`, then the thread request; returns the thread id.
    async fn run(mut self, method: &str, params: Value) -> Result<String, String> {
        let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                           "params": { "clientInfo": { "name": "sculptured", "version": env!("CARGO_PKG_VERSION") } } });
        write_line(self.stdin, &init).await?;
        self.await_response(1).await?;
        write_line(self.stdin, &json!({ "jsonrpc": "2.0", "method": "initialized" })).await?;
        write_line(self.stdin, &json!({ "jsonrpc": "2.0", "id": 2, "method": method, "params": params })).await?;
        let resp = self.await_response(2).await?;
        self.parser.note_model(&resp);
        resp["result"]["thread"]["id"].as_str().map(str::to_string).ok_or_else(|| format!("{method} answered without a thread id"))
    }
}

/// The thread request for `open`, with its parameters.
fn open_request(spec: &LaunchSpec, open: Open<'_>) -> (&'static str, Value) {
    let mut params = CodexAppServerDriver::thread_params(spec);
    let method = match open {
        Open::Start => "thread/start",
        Open::Resume(id) => {
            params["threadId"] = json!(id);
            "thread/resume"
        }
        Open::Fork(id) => {
            params["threadId"] = json!(id);
            "thread/fork"
        }
    };
    (method, params)
}

/// Ends a launch that failed: the whole group goes, since MCP servers the app-server already
/// started (the `rig` shim, the user's own) would otherwise outlive a failed launch or resume.
async fn fail_startup(mut child: Child, stderr: StderrTail, why: String) -> DriverError {
    process::kill_group(&child);
    let _ = child.start_kill();
    let _ = child.wait().await;
    let bytes = stderr.finish().await;
    let last = String::from_utf8_lossy(&bytes).lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    startup_error(if last.is_empty() { why } else { format!("{why}: {last}") })
}

impl CodexAppServerDriver {
    pub(super) async fn spawn(&self, spec: LaunchSpec, open: Open<'_>) -> Result<Box<dyn Session>, DriverError> {
        check_cwd(&spec)?;
        let program = spec.program.clone().unwrap_or_else(|| self.program.clone());
        let args = self.args().into_iter().chain(spec.extra_args.iter().cloned());
        let mut cmd = process::command(&program, &spec, args);
        cmd.envs(Self::remote_env(&spec));
        let mut child = process::spawn(&mut cmd, &program)?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
        let stderr = StderrTail::start(child.stderr.take().expect("piped stderr"));
        let mut parser = CodexParser::default();
        let mut early = Vec::new();
        let (method, params) = open_request(&spec, open);
        let handshake = Handshake { stdin: &mut stdin, lines: &mut lines, parser: &mut parser, early: &mut early }.run(method, params);
        let thread = match tokio::time::timeout(STARTUP, handshake).await {
            Ok(Ok(t)) => t,
            Ok(Err(why)) => return Err(fail_startup(child, stderr, why).await),
            Err(_) => return Err(fail_startup(child, stderr, format!("no answer within {}s", STARTUP.as_secs())).await),
        };
        parser.set_thread(&thread);
        let session = CodexSession::start(Running { child, lines, stderr }, spawn_writer(stdin), parser, thread, early);
        Ok(Box::new(session))
    }
}

/// One writer owns stdin; `None` closes it (how `stop` asks the server to end).
fn spawn_writer(stdin: ChildStdin) -> Writer {
    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Option<String>>();
    tokio::spawn(async move {
        let mut stdin = Some(stdin);
        while let Some(item) = write_rx.recv().await {
            match (item, stdin.as_mut()) {
                (Some(line), Some(s)) => {
                    if s.write_all(format!("{line}\n").as_bytes()).await.is_err() || s.flush().await.is_err() {
                        stdin = None;
                    }
                }
                (None, _) => stdin = None,
                (Some(_), None) => {}
            }
        }
    });
    write_tx
}

/// A started server's process, its output and its stderr tail.
struct Running {
    child: Child,
    lines: StdoutLines,
    stderr: StderrTail,
}

/// A refused message waits out its pause, then goes if nothing else (a turn/completed) has sent it already.
fn schedule_resend(core: Arc<Mutex<Core>>, write: Writer, at: Instant) {
    tokio::spawn(async move {
        tokio::time::sleep_until(at.into()).await;
        let line = core.lock().unwrap().next_turn(Instant::now());
        if let Some(l) = line {
            let _ = write.send(Some(l));
        }
    });
}

/// The stdout reader's handles: the shared core, the writer and the event channel.
struct Reader {
    core: Arc<Mutex<Core>>,
    write: Writer,
    tx: mpsc::UnboundedSender<DriverEvent>,
}

impl Reader {
    fn on_line(&self, line: &str) {
        let out = self.core.lock().unwrap().on_line(line, Instant::now());
        for e in out.events {
            let _ = self.tx.send(e);
        }
        for w in out.writes {
            let _ = self.write.send(Some(w));
        }
        if let Some(at) = out.wake {
            schedule_resend(self.core.clone(), self.write.clone(), at);
        }
    }

    async fn run(self, mut p: Running, mut kill_rx: oneshot::Receiver<()>, exited_tx: watch::Sender<bool>) {
        let killed = process::pump_lines(&mut p.lines, &mut kill_rx, |l| self.on_line(&l)).await;
        // The output ended (exit, EOF, read error) or Stop's grace ran out. The server is not
        // reaped yet, so its group id is still ours: end the group, including a command an
        // interrupt left running and the MCP servers it started.
        process::kill_group(&p.child);
        let status = process::reap(&mut p.child, &mut kill_rx, killed).await;
        let stderr = p.stderr.finish().await;
        // Closed before `Exited` is seen, so a send right after `Exited` is refused, not lost.
        let _ = exited_tx.send(true);
        let _ = self.tx.send(process::exited(status, &stderr));
    }
}

struct CodexSession {
    core: Arc<Mutex<Core>>,
    write: Writer,
    events: Option<BoxStream<'static, DriverEvent>>,
    kill: Option<oneshot::Sender<()>>,
    exited: watch::Receiver<bool>,
    /// The event channel, for the driver's own notes; weak, so the stream still ends after `Exited`.
    notes: mpsc::WeakUnboundedSender<DriverEvent>,
    /// The app-server's pid, which leads the process group the driver made for it.
    pid: Option<u32>,
}

impl CodexSession {
    fn start(p: Running, write: Writer, parser: CodexParser, thread: String, early: Vec<DriverEvent>) -> Self {
        let core = Arc::new(Mutex::new(Core::new(parser, 2)));
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(DriverEvent::SessionReady { native_session_id: thread });
        for e in early {
            let _ = tx.send(e);
        }
        let (kill_tx, kill_rx) = oneshot::channel::<()>();
        let (exited_tx, exited_rx) = watch::channel(false);
        let notes = tx.downgrade();
        let pid = p.child.id();
        let reader = Reader { core: core.clone(), write: write.clone(), tx };
        tokio::spawn(reader.run(p, kill_rx, exited_tx));
        let events = Some(UnboundedReceiverStream::new(rx).boxed());
        Self { core, write, events, kill: Some(kill_tx), exited: exited_rx, notes, pid }
    }

    fn write(&self, line: String) -> Result<(), DriverError> {
        if *self.exited.borrow() {
            return Err(DriverError::Closed);
        }
        self.write.send(Some(line)).map_err(|_| DriverError::Closed)
    }

    /// Adds a driver note to the event stream, unless the session has ended.
    fn note(&self, e: DriverEvent) {
        if let Some(tx) = self.notes.upgrade()
            && !*self.exited.borrow()
        {
            let _ = tx.send(e);
        }
    }

    fn write_some(&self, line: Option<String>) -> Result<(), DriverError> {
        match line {
            Some(l) => self.write(l),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl Session for CodexSession {
    fn process_id(&self) -> Option<u32> {
        self.pid
    }

    /// Starts a turn now, or queues the message until the running turn completes.
    async fn send(&mut self, msg: OutboundMessage) -> Result<(), DriverError> {
        if *self.exited.borrow() {
            return Err(DriverError::Closed);
        }
        let line = self.core.lock().unwrap().send(msg, Instant::now());
        self.write_some(line)
    }

    /// Ends the running turn, or the one being started as soon as its id arrives. The command
    /// Codex is running keeps running (see the driver's doc). During a re-send pause nothing runs:
    /// the transcript gets an "interrupt not applied" note instead.
    async fn interrupt(&mut self) -> Result<(), DriverError> {
        let (line, note) = {
            let mut core = self.core.lock().unwrap();
            let note = core.paused_note();
            (core.interrupt(), note)
        };
        if let Some(n) = note {
            self.note(n);
        }
        self.write_some(line)
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
            .core
            .lock()
            .unwrap()
            .parser
            .answer_reply(question_id, &answer)
            .ok_or(DriverError::Unsupported("an answer to a question this agent is not asking"))?;
        self.write(reply.to_string())
    }

    fn take_events(&mut self) -> Option<BoxStream<'static, DriverEvent>> {
        self.events.take()
    }
}
