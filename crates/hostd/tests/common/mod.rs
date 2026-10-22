//! Shared setup for the seat-tools tests: a fake controld on the other end of a real host link, and a
//! supervisor serving its socket with a running stub agent.
#![allow(dead_code)]
use async_trait::async_trait;
use drivers::stub::StubDriver;
use drivers::{Driver, DriverError, LaunchSpec, Session};
use futures::{SinkExt, StreamExt};
use host_proto::seat_tools::TOKEN_ENV;

use host_proto::{FromHost, HostCommand, LaunchParams, SeatToolsParams, ToHost};
use hostd::seat_tools::SocketLimits;
use hostd::supervisor::SeatToolsHost;
use hostd::{Journal, Supervisor};

use model::{Caps, EventBody, Subject};
use serde_json::Value;
use std::path::Path;
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

pub fn stub_sup() -> Supervisor {
    Supervisor::new(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(1000).unwrap(),
    )
}

/// Delegates to the stub but keeps the LaunchSpec it was given (for its token).
pub struct Capture(pub Arc<Mutex<Option<LaunchSpec>>>);
#[async_trait]
impl Driver for Capture {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        *self.0.lock().unwrap() = Some(spec.clone());
        StubDriver.launch(spec).await
    }
    async fn resume(&self, spec: LaunchSpec, id: &str) -> Result<Box<dyn Session>, DriverError> {
        StubDriver.resume(spec, id).await
    }
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

/// A supervisor serving its socket in `dir`, linked to a fake controld; the stub agent `dev.a@r` runs
/// with Sculptured's tools. Returns its token too.
pub async fn tools_host(dir: &Path) -> (Supervisor, Ws, std::path::PathBuf, String) {
    tools_host_with(dir, false, SocketLimits::default()).await
}

/// `tools_host` with the agent launched with or without `approve`, and the socket's limits.
pub async fn tools_host_with(
    dir: &Path,
    approve: bool,
    limits: SocketLimits,
) -> (Supervisor, Ws, std::path::PathBuf, String) {
    let socket = dir.join("seat-tools.sock");
    let seen = Arc::new(Mutex::new(None));
    let host = SeatToolsHost {
        socket: socket.clone(),
        shim: "/opt/sculptured".into(),
    };
    let sup = Supervisor::with_seat_tools(
        "local".into(),
        vec![Arc::new(Capture(seen.clone()))],
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
        cwd: dir.display().to_string(),
        seat_tools: Some(SeatToolsParams {
            approve,
            workflow: true,
        }),
        ..Default::default()
    };
    sup.handle(HostCommand::Launch {
        seat: "dev.a@r".into(),
        driver: "stub".into(),
        params,
    })
    .await
    .unwrap();
    let token = seen
        .lock()
        .unwrap()
        .clone()
        .unwrap()
        .seat_tools
        .unwrap()
        .env
        .0
        .into_iter()
        .find(|(k, _)| k == TOKEN_ENV)
        .unwrap()
        .1;
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
