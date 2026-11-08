//! Shared setup for hostd's tests: a fake controld on the other end of a real host link, and agents
//! on the real Claude Code CLI with its cheapest model.
//!
//! A test that needs an agent is live: named `live_…`, marked `#[ignore = "live: …"]` (it needs
//! `claude` installed and signed in), and it starts with `let Some(_) = common::require() else
//! { return };`. Run them with `cargo test -p hostd --test <file> -- --ignored live_`. A live test
//! asserts on what hostd records and enforces, never on the model's exact words.
#![allow(dead_code)]
use async_trait::async_trait;
use drivers::claude::ClaudeStreamDriver;
use drivers::codex::CodexAppServerDriver;
use drivers::{Driver, DriverError, LaunchSpec, Session};
use futures::{SinkExt, StreamExt};
use host_proto::seat_tools::TOKEN_ENV;

use host_proto::{FromHost, HostCommand, LaunchParams, SeatToolsParams, ToHost};
use hostd::seat_tools::SocketLimits;
use hostd::supervisor::SeatToolsHost;
use hostd::{Journal, Supervisor};

use host_proto::HostEvent;
use model::{Caps, EventBody, Subject};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

pub type Ws = WebSocketStream<TcpStream>;

/// No seat-tool call in these tests may wait longer than this: a hung socket or relay fails the test
/// instead of hanging the suite.
pub const CALL_WAIT: Duration = Duration::from_secs(10);

pub async fn accept(listener: &TcpListener) -> Ws {
    let (tcp, _) = listener.accept().await.unwrap();
    tokio_tungstenite::accept_async(tcp).await.unwrap()
}

pub async fn recv(ws: &mut Ws) -> FromHost {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("frame in time")
            .expect("open")
            .unwrap();
        if let Message::Text(t) = m {
            return host_proto::decode(t.as_str()).unwrap();
        }
    }
}

pub async fn send(ws: &mut Ws, m: &ToHost) {
    ws.send(Message::text(host_proto::encode(m))).await.unwrap();
}

/// A linked supervisor and the fake controld's end, after Hello and Welcome.
pub async fn linked(sup: &Supervisor) -> Ws {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/host", listener.local_addr().unwrap());
    let s = sup.clone();
    tokio::spawn(async move { hostd::run_link(&url, "secret", &s).await });
    let mut ws = accept(&listener).await;
    assert!(matches!(recv(&mut ws).await, FromHost::Hello { .. }));
    send(&mut ws, &ToHost::Welcome { resume_after: 0 }).await;
    ws
}

/// The next SeatTool frame (events are skipped).
pub async fn next_call(ws: &mut Ws) -> (String, String, String, Value) {
    loop {
        if let FromHost::SeatTool {
            call_id,
            seat,
            tool,
            args,
        } = recv(ws).await
        {
            return (call_id, seat, tool, args);
        }
    }
}

/// Waits for a spawned call, failing the test (not hanging it) when it never returns.
pub async fn ended<T>(call: tokio::task::JoinHandle<T>) -> T {
    tokio::time::timeout(CALL_WAIT, call)
        .await
        .expect("the call ends in time")
        .unwrap()
}

// ---- live agents ----

/// The model every live Claude agent runs with: the cheapest one.
pub const CLAUDE_MODEL: &str = "haiku";

/// The real Claude Code driver's name, as a launch names it.
pub const CLAUDE: &str = "claude-stream";

/// How long a live wait lasts: a real model turn takes seconds to a minute or two.
pub const WAIT: Duration = Duration::from_secs(240);

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// The `claude` CLI, or None (with the reason printed) when this machine cannot run live tests.
pub fn require() -> Option<PathBuf> {
    let found = on_path("claude");
    if found.is_none() {
        eprintln!(
            "skipped: the claude CLI is not on PATH; live tests need it installed and signed in"
        );
    }
    found
}

/// The `codex` CLI, or None (with the reason printed).
pub fn require_codex() -> Option<PathBuf> {
    let found = on_path("codex");
    if found.is_none() {
        eprintln!(
            "skipped: the codex CLI is not on PATH; live Codex tests need it installed and signed in"
        );
    }
    found
}

/// The real Claude Code driver.
pub fn claude() -> ClaudeStreamDriver {
    ClaudeStreamDriver {
        program: on_path("claude").unwrap_or_else(|| "claude".into()),
    }
}

/// The real Codex driver; `native_questions` lets Codex ask its own approval questions.
pub fn codex(native_questions: bool) -> CodexAppServerDriver {
    CodexAppServerDriver {
        program: on_path("codex").unwrap_or_else(|| "codex".into()),
        native_questions,
    }
}

/// A supervisor offering the real Claude Code CLI. Nothing runs until a test launches.
pub fn sup() -> Supervisor {
    Supervisor::new(
        "local".into(),
        vec![Arc::new(claude())],
        Journal::open_in_memory(1000).unwrap(),
    )
}

/// Launch parameters for an agent working in `cwd` on the cheapest Claude model.
pub fn params(cwd: &Path) -> LaunchParams {
    LaunchParams {
        cwd: cwd.display().to_string(),
        model: Some(CLAUDE_MODEL.into()),
        ..Default::default()
    }
}

/// A real driver that keeps the LaunchSpec it was given, and can hold launch until released. It
/// changes nothing about the agent.
#[derive(Clone)]
pub struct Capture {
    pub inner: Arc<dyn Driver>,
    pub seen: Arc<Mutex<Option<LaunchSpec>>>,
    pub hold: Option<Arc<tokio::sync::Notify>>,
}
impl Capture {
    pub fn of(inner: Arc<dyn Driver>) -> Capture {
        Capture {
            inner,
            seen: Arc::default(),
            hold: None,
        }
    }
    /// The real Claude Code driver, captured.
    pub fn claude() -> Capture {
        Capture::of(Arc::new(claude()))
    }
    /// The spec of the last launch.
    pub fn spec(&self) -> LaunchSpec {
        self.seen.lock().unwrap().clone().expect("launched")
    }
    /// The seat-tools token of the last launch.
    pub fn token(&self) -> String {
        token_in(&self.spec())
    }
}
#[async_trait]
impl Driver for Capture {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn capabilities(&self) -> Caps {
        self.inner.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        *self.seen.lock().unwrap() = Some(spec.clone());
        if let Some(h) = &self.hold {
            h.notified().await;
        }
        self.inner.launch(spec).await
    }
    async fn resume(&self, spec: LaunchSpec, id: &str) -> Result<Box<dyn Session>, DriverError> {
        *self.seen.lock().unwrap() = Some(spec.clone());
        self.inner.resume(spec, id).await
    }
    async fn fork(&self, spec: LaunchSpec, id: &str) -> Result<Box<dyn Session>, DriverError> {
        *self.seen.lock().unwrap() = Some(spec.clone());
        self.inner.fork(spec, id).await
    }
    fn seat_tools_config_file(&self) -> bool {
        self.inner.seat_tools_config_file()
    }
}

/// The seat-tools token a launch was given.
pub fn token_in(spec: &LaunchSpec) -> String {
    spec.seat_tools
        .as_ref()
        .expect("seat tools")
        .env
        .iter()
        .find(|(k, _)| k == TOKEN_ENV)
        .expect("a token")
        .1
        .clone()
}

/// Collects event bodies until `pred` matches one, waiting up to `WAIT`.
pub async fn until(
    rx: &mut tokio::sync::broadcast::Receiver<HostEvent>,
    pred: impl Fn(&EventBody) -> bool,
) -> Vec<EventBody> {
    let mut seen = Vec::new();
    loop {
        let ev = tokio::time::timeout(WAIT, rx.recv())
            .await
            .expect("event in time")
            .expect("open");
        let hit = pred(&ev.body);
        seen.push(ev.body);
        if hit {
            return seen;
        }
    }
}

/// The pid of `seat`'s real CLI process, checked to be a child of this test process: only a
/// process the test started is ever signalled.
pub fn own_agent_pid(sup: &Supervisor, seat: &str) -> i32 {
    let pid = sup
        .recorded_processes()
        .into_iter()
        .find(|(s, ..)| s == seat)
        .map(|(_, pid, _)| pid)
        .expect("the agent's process is on record");
    assert_eq!(
        parent_of(pid as i32),
        Some(std::process::id()),
        "only this test's own agent is signalled"
    );
    pid as i32
}

fn parent_of(pid: i32) -> Option<u32> {
    stat_field(pid, 1)
}

/// A numeric field of /proc/<pid>/stat after the command name (0 is the state).
fn stat_field(pid: i32, n: usize) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(n))
        .and_then(|p| p.parse().ok())
}

/// Kills the real CLI process of `seat`, as a crash would. Only a child of this test process is
/// ever killed.
pub fn crash(sup: &Supervisor, seat: &str) {
    let pid = own_agent_pid(sup, seat);
    // SAFETY: kill(2) on a child process this test started, checked by `own_agent_pid`.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
}

/// A real CLI held stopped (SIGSTOP to its process group): it reads nothing, so a write to its
/// stdin blocks once the pipe is full, and it never calls the model. Dropping it kills the group
/// if its leader is still this test's child, so a failing test leaves no stopped process behind.
pub struct Frozen(i32);

/// Stops `seat`'s real CLI (a child of this test process, leading the process group the driver
/// made for it) until hostd kills it.
pub fn freeze(sup: &Supervisor, seat: &str) -> Frozen {
    let pid = own_agent_pid(sup, seat);
    assert_eq!(
        stat_field(pid, 2),
        Some(pid as u32),
        "the agent leads its own group"
    );
    // SAFETY: SIGSTOP to the group of a child process this test started, checked just above.
    unsafe {
        libc::killpg(pid, libc::SIGSTOP);
    }
    Frozen(pid)
}

impl Drop for Frozen {
    fn drop(&mut self) {
        if parent_of(self.0) == Some(std::process::id()) {
            // SAFETY: the leader is still this test's child (not reaped), so the group is its own.
            unsafe {
                libc::killpg(self.0, libc::SIGKILL);
            }
        }
    }
}

/// A message far bigger than a pipe holds: writing it to a stopped CLI's stdin cannot finish.
pub fn wedging_text() -> String {
    "x".repeat(4 << 20)
}

pub fn blocking_call(
    socket: &Path,
    token: &str,
    tool: &'static str,
    args: Value,
) -> tokio::task::JoinHandle<Result<Value, String>> {
    let (socket, token) = (socket.to_path_buf(), token.to_string());
    tokio::task::spawn_blocking(move || host_proto::seat_tools::call(&socket, &token, tool, args))
}

/// A supervisor serving its socket in `dir`, linked to a fake controld; agent `dev.a@r` runs on the
/// real Claude Code CLI with Sculptured's tools (no message is sent, so no model is called). Returns
/// its token too.
pub async fn tools_host(dir: &Path) -> (Supervisor, Ws, std::path::PathBuf, String) {
    tools_host_with(dir, false, SocketLimits::default()).await
}

/// `tools_host` with the agent launched with or without `approve`, and the socket's limits.
pub async fn tools_host_with(
    dir: &Path,
    approve: bool,
    limits: SocketLimits,
) -> (Supervisor, Ws, std::path::PathBuf, String) {
    assert!(
        require().is_some(),
        "these tests launch the real claude CLI (no message is sent to it)"
    );
    let socket = dir.join("seat-tools.sock");
    let capture = Capture::claude();
    let host = SeatToolsHost {
        socket: socket.clone(),
        shim: "/opt/sculptured".into(),
    };
    let sup = Supervisor::with_seat_tools(
        "local".into(),
        vec![Arc::new(capture.clone())],
        Journal::open_in_memory(1000).unwrap(),
        Some(host),
    );
    tokio::spawn(hostd::seat_tools::serve_socket_with(
        sup.clone(),
        socket.clone(),
        limits,
    ));
    let deadline = tokio::time::Instant::now() + CALL_WAIT;
    while !socket.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the socket never appeared"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let ws = linked(&sup).await;
    let params = LaunchParams {
        seat_tools: Some(SeatToolsParams {
            approve,
            workflow: true,
        }),
        ..params(dir)
    };
    sup.handle(HostCommand::Launch {
        seat: "dev.a@r".into(),
        driver: CLAUDE.into(),
        params,
    })
    .await
    .unwrap();
    let token = capture.token();
    (sup, ws, socket, token)
}

/// Sends raw bytes on the socket and returns the reply line (blocking, on its own thread).
pub fn raw_call(socket: &Path, bytes: Vec<u8>) -> tokio::task::JoinHandle<Value> {
    let socket = socket.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::{BufRead, Write};
        let mut s = std::os::unix::net::UnixStream::connect(&socket).unwrap();
        s.write_all(&bytes).unwrap();
        let mut line = String::new();
        std::io::BufReader::new(s).read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap_or(Value::Null)
    })
}

/// The ReportRejected evidence hostd journals for a refused call (it reaches controld as an event).
pub async fn rejected_event(ws: &mut Ws) -> (String, String, String) {
    loop {
        if let FromHost::Event { event } = recv(ws).await
            && let EventBody::ReportRejected { tool, reason, .. } = event.body
        {
            let Subject::Seat(seat) = event.subject else {
                panic!("a seat event")
            };
            return (seat, tool, reason);
        }
    }
}

/// No SeatTool frame reaches the fake controld within a short while.
pub async fn no_call_within(ws: &mut Ws, wait: Duration) {
    let got = tokio::time::timeout(wait, async {
        loop {
            if let FromHost::SeatTool { tool, .. } = recv(ws).await {
                return tool;
            }
        }
    })
    .await;
    assert!(got.is_err(), "a call reached controld: {got:?}");
}
