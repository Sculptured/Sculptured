use async_trait::async_trait;
use drivers::stub::StubDriver;
use drivers::{Driver, DriverError, DriverEvent, LaunchSpec, OutboundMessage, Session};
use futures::stream::BoxStream;
use futures::{SinkExt, StreamExt};

use host_proto::{FromHost, HostCommand, LaunchParams, PROTOCOL_VERSION, ToHost};
use hostd::{Journal, Supervisor};

use model::{Caps, EventBody, MessageIntent, SeatLifecycle};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

type Ws = WebSocketStream<TcpStream>;

/// Plays controld: accepts one connection and checks the bearer token.
#[allow(clippy::result_large_err)] // the handshake callback's error type is tungstenite's
async fn accept(listener: &TcpListener) -> Ws {
    let (tcp, _) = listener.accept().await.unwrap();
    tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, resp: Response| {
        let auth = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        assert_eq!(auth, Some("Bearer secret"));
        Ok::<Response, ErrorResponse>(resp)
    })
    .await
    .unwrap()
}

async fn recv(ws: &mut Ws) -> FromHost {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("frame in time")
            .expect("socket open")
            .unwrap();
        if let Message::Text(t) = m {
            return host_proto::decode(t.as_str()).unwrap();
        }
    }
}

async fn send(ws: &mut Ws, m: &ToHost) {
    ws.send(Message::text(host_proto::encode(m))).await.unwrap();
}

/// A listener for the link to dial, and its URL.
async fn listen() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/host", listener.local_addr().unwrap());
    (listener, url)
}

async fn command(ws: &mut Ws, command_id: &str, command: HostCommand) {
    send(
        ws,
        &ToHost::Command {
            command_id: command_id.into(),
            command: Box::new(command),
        },
    )
    .await;
}

fn launch(seat: &str, driver: &str, dir: &tempfile::TempDir) -> HostCommand {
    let params = LaunchParams {
        cwd: dir.path().display().to_string(),
        ..Default::default()
    };
    HostCommand::Launch {
        seat: seat.into(),
        driver: driver.into(),
        params,
    }
}

/// Launches `dev.a@r` on the first connection; its Launching event arrives before the result.
async fn launch_and_see_running(ws: &mut Ws, dir: &tempfile::TempDir) {
    command(ws, "c1", launch("dev.a@r", "stub", dir)).await;
    let (mut got_result, mut saw_launching, mut saw_running) = (false, false, false);
    while !(got_result && saw_running) {
        match recv(ws).await {
            FromHost::CommandResult {
                command_id,
                ok,
                error,
            } => {
                assert_eq!(command_id, "c1");
                assert!(ok, "{error:?}");
                assert!(
                    saw_launching,
                    "events emitted while handling a command arrive before its result"
                );
                got_result = true;
            }
            FromHost::Event { event } => {
                if matches!(
                    event.body,
                    EventBody::SeatLifecycleChanged {
                        lifecycle: SeatLifecycle::Launching,
                        ..
                    }
                ) {
                    saw_launching = true;
                }
                if matches!(
                    event.body,
                    EventBody::SeatLifecycleChanged {
                        lifecycle: SeatLifecycle::Running,
                        ..
                    }
                ) {
                    saw_running = true;
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Second connection: hello lists the live seat; everything after resume_after replays in order.
async fn reconnect_and_replay(sup: &Supervisor, listener: &TcpListener, url: String) -> (Ws, u64) {
    let s2 = sup.clone();
    tokio::spawn(async move { hostd::run_link(&url, "secret", &s2).await });
    let mut ws = accept(listener).await;
    match recv(&mut ws).await {
        FromHost::Hello { live_seats, .. } => assert_eq!(live_seats, vec!["dev.a@r".to_string()]),
        other => panic!("expected hello, got {other:?}"),
    }
    let total = sup.unacked_after(0).unwrap().len() as u64;
    assert!(
        total >= 4,
        "launching, tenure, running, idle were journaled"
    );
    send(&mut ws, &ToHost::Welcome { resume_after: 2 }).await;
    let mut replayed = Vec::new();
    while (replayed.len() as u64) < total - 2 {
        match recv(&mut ws).await {
            FromHost::Event { event } => replayed.push(event.host_seq),
            other => panic!("expected replayed event, got {other:?}"),
        }
    }
    assert_eq!(replayed, (3..=total).collect::<Vec<u64>>());
    (ws, total)
}

#[tokio::test]
async fn link_runs_commands_and_replays_after_reconnect() {
    let (listener, url) = listen().await;
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(1000).unwrap(),
    );

    // First connection: hello, welcome, launch.
    let (s1, u1) = (sup.clone(), url.clone());
    let link1 = tokio::spawn(async move { hostd::run_link(&u1, "secret", &s1).await });
    let mut ws = accept(&listener).await;
    match recv(&mut ws).await {
        FromHost::Hello {
            host_id,
            protocol,
            drivers,
            live_seats,
            ..
        } => {
            assert_eq!(host_id, "local");
            assert_eq!(protocol, PROTOCOL_VERSION);
            assert!(drivers.iter().any(|d| d.name == "stub"));
            assert!(live_seats.is_empty());
        }
        other => panic!("expected hello, got {other:?}"),
    }
    send(&mut ws, &ToHost::Welcome { resume_after: 0 }).await;
    launch_and_see_running(&mut ws, &dir).await;

    // controld disappears without acking anything; the agent keeps running.
    drop(ws);
    let res = tokio::time::timeout(Duration::from_secs(3), link1)
        .await
        .expect("link notices")
        .unwrap();
    assert!(res.is_err(), "the link reports the disconnect");
    assert_eq!(sup.live_seats().await, vec!["dev.a@r".to_string()]);
    let (mut ws, total) = reconnect_and_replay(&sup, &listener, url).await;

    // An ack prunes the journal.
    send(&mut ws, &ToHost::Ack { upto: total }).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while !sup.unacked_after(0).unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("journal pruned after ack");
}

/// Test driver: `send` never completes (a wedged agent whose stdin is full).
struct BlockingDriver;
struct BlockingSession {
    tx: UnboundedSender<DriverEvent>,
    rx: Option<UnboundedReceiver<DriverEvent>>,
}

#[async_trait]
impl Driver for BlockingDriver {
    fn name(&self) -> &'static str {
        "blocking"
    }
    fn capabilities(&self) -> Caps {
        Caps::default()
    }
    async fn launch(&self, _spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = tx.send(DriverEvent::SessionReady {
            native_session_id: "blocking".into(),
        });
        Ok(Box::new(BlockingSession { tx, rx: Some(rx) }))
    }
    async fn resume(
        &self,
        spec: LaunchSpec,
        _native_id: &str,
    ) -> Result<Box<dyn Session>, DriverError> {
        self.launch(spec).await
    }
}

#[async_trait]
impl Session for BlockingSession {
    async fn send(&mut self, _msg: OutboundMessage) -> Result<(), DriverError> {
        std::future::pending().await
    }
    async fn interrupt(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
    async fn stop(&mut self, _grace: Duration) -> Result<(), DriverError> {
        let _ = self.tx.send(DriverEvent::Exited {
            exit_code: Some(0),
            signal: None,
            stderr_tail: String::new(),
        });
        Ok(())
    }
    fn take_events(&mut self) -> Option<BoxStream<'static, DriverEvent>> {
        self.rx
            .take()
            .map(|rx| UnboundedReceiverStream::new(rx).boxed())
    }
}

#[tokio::test]
async fn a_wedged_send_does_not_stop_the_link_from_reading_stop() {
    let (listener, url) = listen().await;
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new(
        "local".into(),
        vec![Arc::new(BlockingDriver)],
        Journal::open_in_memory(1000).unwrap(),
    );
    let s = sup.clone();
    let _link = tokio::spawn(async move { hostd::run_link(&url, "secret", &s).await });
    let mut ws = accept(&listener).await;
    assert!(matches!(recv(&mut ws).await, FromHost::Hello { .. }));
    send(&mut ws, &ToHost::Welcome { resume_after: 0 }).await;

    let seat = "dev.x@r";
    command(&mut ws, "l", launch(seat, "blocking", &dir)).await;
    loop {
        if let FromHost::CommandResult {
            command_id,
            ok,
            error,
        } = recv(&mut ws).await
        {
            assert_eq!(command_id, "l");
            assert!(ok, "{error:?}");
            break;
        }
    }
    // The Send wedges inside the driver and never produces a result.
    let wedged = HostCommand::Send {
        seat: seat.into(),
        message_id: "m".into(),
        intent: MessageIntent::Inform,
        text: "x".into(),
    };
    command(&mut ws, "w", wedged).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    command(
        &mut ws,
        "s",
        HostCommand::Stop {
            seat: seat.into(),
            grace_ms: 500,
            hard: false,
        },
    )
    .await;
    let mut results = Vec::new();
    while !results.iter().any(|(id, _): &(String, bool)| id == "s") {
        if let FromHost::CommandResult { command_id, ok, .. } = recv(&mut ws).await {
            results.push((command_id, ok));
        }
    }
    assert!(results.contains(&("s".to_string(), true)), "{results:?}");
}

/// I8: a fresh journal behind controld's cursor fast-forwards, so the next event is
/// numbered after resume_after instead of being dropped by controld as a duplicate.
#[tokio::test]
async fn fresh_journal_fast_forwards_past_controlds_cursor() {
    let (listener, url) = listen().await;
    let dir = tempfile::tempdir().unwrap();
    let sup = Supervisor::new(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(1000).unwrap(),
    );
    let s1 = sup.clone();
    let _link = tokio::spawn(async move { hostd::run_link(&url, "secret", &s1).await });
    let mut ws = accept(&listener).await;
    assert!(matches!(recv(&mut ws).await, FromHost::Hello { .. }));
    send(&mut ws, &ToHost::Welcome { resume_after: 50 }).await;
    command(&mut ws, "c1", launch("dev.a@r", "stub", &dir)).await;
    let mut seqs = Vec::new();
    while seqs.len() < 2 {
        if let FromHost::Event { event } = recv(&mut ws).await {
            seqs.push(event.host_seq);
        }
    }
    assert_eq!(
        seqs,
        [51, 52],
        "the first event after a Welcome{{resume_after: 50}} is 51"
    );
}

#[tokio::test]
async fn the_runner_can_dial_secure_websocket_urls() {
    // A listener that accepts and stays silent: the TCP connection succeeds, so the client must try TLS.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = vec![];
        while let Ok((s, _)) = listener.accept().await {
            held.push(s);
        }
    });
    let req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
        format!("wss://{addr}/host"),
    )
    .unwrap();
    let res = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio_tungstenite::connect_async(req),
    )
    .await;
    if let Ok(Err(err)) = res {
        assert!(
            !matches!(
                err,
                tokio_tungstenite::tungstenite::Error::Url(
                    tokio_tungstenite::tungstenite::error::UrlError::TlsFeatureNotEnabled
                )
            ),
            "{err:?}"
        );
    }
    // A timeout means the TLS handshake started and is waiting on the silent peer: wss is supported.
}

#[tokio::test]
async fn a_silent_link_is_given_up_so_the_runner_can_reconnect() {
    // A controld that welcomes the runner, then goes silent (a sleeping laptop's dead connection).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/host", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let _hello = ws.next().await;
        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&ToHost::Welcome { resume_after: 0 })
                .unwrap()
                .into(),
        ))
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await; // never reads again, so never answers pings
        drop(ws);
    });
    let sup = Supervisor::new(
        "dev-1".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(100).unwrap(),
    );
    let keep = hostd::link::Keepalive {
        every: Duration::from_millis(100),
        dead_after: Duration::from_millis(400),
    };
    let res = tokio::time::timeout(
        Duration::from_secs(5),
        hostd::link::run_link_with(&url, "t", &sup, keep),
    )
    .await;
    assert!(
        matches!(res, Ok(Err(_))),
        "the link must end with an error well before the peer gives up: {res:?}"
    );
}
