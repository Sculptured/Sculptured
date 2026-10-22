mod common;

use controld::store::Store;
use controld::{AppState, reconciler};
use drivers::Driver;
use drivers::stub::StubDriver;
use hostd::journal::Journal;
use hostd::supervisor::Supervisor;
use model::{Desired, EventBody, SeatLifecycle};
use std::sync::Arc;

const TOKEN: &str = "test-token";
const SPEC: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: stub, cwd: "." }
    edges: [{ kind: delegates_to, from: a, to: b }]
"#;

/// Serves `app` on an ephemeral port; allowed_hosts includes the real address
/// so Task 11's Host guard accepts the hostd connection.
async fn serve() -> (AppState, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        TOKEN.into(),
        vec![addr.to_string()],
    );
    let router = controld::app(st.clone(), None);
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (st, format!("ws://{addr}/host"))
}

fn supervisor() -> Supervisor {
    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::new(StubDriver)];
    Supervisor::new(
        "local".into(),
        drivers,
        Journal::open_in_memory(10_000).unwrap(),
    )
}

fn boot_rig(st: &AppState, yaml: &str) -> rigspec::CompiledRig {
    let rig = rigspec::compile(yaml).unwrap();
    st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    rig
}

fn reason(st: &AppState, seat: &str) -> Option<String> {
    st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .and_then(|s| s.lifecycle_reason)
}

fn first_launching_seq(st: &AppState, seat: &str) -> i64 {
    st.store
        .lock()
        .unwrap()
        .events_for_seat(seat, 0, 100)
        .unwrap()
        .into_iter()
        .find(|e| {
            matches!(
                e.body,
                EventBody::SeatLifecycleChanged {
                    lifecycle: SeatLifecycle::Launching,
                    ..
                }
            )
        })
        .expect("a launching event")
        .seq
}

#[tokio::test]
async fn boot_runs_layers_in_order_and_survives_link_loss() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = supervisor();

    let (s1, u1) = (sup.clone(), url.clone());
    let link = tokio::spawn(async move { hostd::link::run_link(&u1, TOKEN, &s1).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;

    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert_eq!(
        crate::common::lifecycle(&st, "dev.a@r"),
        Some(SeatLifecycle::Running)
    );
    assert_eq!(
        crate::common::lifecycle(&st, "dev.b@r"),
        Some(SeatLifecycle::Running)
    );
    assert!(
        first_launching_seq(&st, "dev.a@r") < first_launching_seq(&st, "dev.b@r"),
        "a (delegator) launches first"
    );

    link.abort();
    crate::common::wait_for("seats stale", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Stale)
            && crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Stale)
    })
    .await;
    assert_eq!(
        reason(&st, "dev.a@r").as_deref(),
        Some("Lost contact with this agent's runner; reconnecting…")
    );
    assert!(!st.hosts.is_connected("local"));

    let (s2, u2) = (sup.clone(), url.clone());
    let _link2 = tokio::spawn(async move { hostd::link::run_link(&u2, TOKEN, &s2).await });
    crate::common::wait_for("seats running again", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Running)
            && crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Running)
    })
    .await;
    assert_eq!(
        reason(&st, "dev.a@r").as_deref(),
        Some("Reconnected; the agent kept running")
    );
}

#[tokio::test]
async fn hostd_restart_puts_dead_seats_in_attention() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let (s1, u1) = (supervisor(), url.clone());
    let link = tokio::spawn(async move { hostd::link::run_link(&u1, TOKEN, &s1).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert_eq!(
        crate::common::lifecycle(&st, "dev.a@r"),
        Some(SeatLifecycle::Running)
    );

    // hostd dies with its agents. A fresh hostd (new supervisor, nothing live) reconnects.
    link.abort();
    crate::common::wait_for("seats stale", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Stale)
            && crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Stale)
    })
    .await;
    let (s2, u2) = (supervisor(), url.clone());
    let _link2 = tokio::spawn(async move { hostd::link::run_link(&u2, TOKEN, &s2).await });
    crate::common::wait_for("seats need attention", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Attention)
            && crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Attention)
    })
    .await;
    assert_eq!(
        reason(&st, "dev.b@r").as_deref(),
        Some(
            "This agent's process ended when Sculptured restarted. Start it again from its file to relaunch it."
        )
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn send_and_stop_through_the_link() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = supervisor();
    let s1 = sup.clone();
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &s1).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;

    let id = reconciler::send_message(
        &st,
        "dev.a@r",
        "hello".into(),
        model::MessageIntent::Inform,
        false,
    )
    .await
    .unwrap();
    crate::common::wait_for("message consumed", || {
        st.store.lock().unwrap().message_state(&id).unwrap() == Some(model::MessageState::Consumed)
    })
    .await;
    let texts: Vec<String> = st
        .store
        .lock()
        .unwrap()
        .events_for_seat("dev.a@r", 0, 100)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::AgentText { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["stub received: hello"]);

    reconciler::stop_rig(st.clone(), "r".into()).await.unwrap();
    crate::common::wait_for("seats stopped", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Stopped)
            && crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Stopped)
    })
    .await;
    assert_eq!(
        st.store
            .lock()
            .unwrap()
            .rig_snapshot("r")
            .unwrap()
            .unwrap()
            .desired,
        Desired::Stopped
    );
}

#[tokio::test]
async fn wrong_token_is_rejected() {
    let (_st, url) = serve().await;
    let err = hostd::link::run_link(&url, "nope", &supervisor())
        .await
        .expect_err("must be rejected");
    assert!(err.to_string().contains("401"), "{err}");
}

#[tokio::test]
async fn unsupported_runtime_needs_attention() {
    let (st, _url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(
        &st,
        &SPEC.replace(
            "runtime: stub, cwd: \".\" }\n      - { id: b",
            "runtime: pi, cwd: \".\" }\n      - { id: b",
        ),
    );
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert_eq!(
        crate::common::lifecycle(&st, "dev.a@r"),
        Some(SeatLifecycle::Attention)
    );
    assert_eq!(
        reason(&st, "dev.a@r").as_deref(),
        Some("Pi agents can't run in this version yet")
    );
}

#[tokio::test]
async fn disconnected_host_needs_attention() {
    let (st, _url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert_eq!(
        crate::common::lifecycle(&st, "dev.b@r"),
        Some(SeatLifecycle::Attention)
    );
    assert_eq!(
        reason(&st, "dev.b@r").as_deref(),
        Some("Sculptured's agent runner is not connected")
    );
}

// ---- fix round 1 ----

use drivers::{DriverError, LaunchSpec, Session};
use futures::{SinkExt, StreamExt};
use host_proto::{FromHost, PROTOCOL_VERSION};
use model::TurnStatus;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderValue};

fn seq_where(st: &AppState, seat: &str, f: impl Fn(&EventBody) -> bool) -> Option<i64> {
    st.store
        .lock()
        .unwrap()
        .events_for_seat(seat, 0, 1000)
        .unwrap()
        .into_iter()
        .find(|e| f(&e.body))
        .map(|e| e.seq)
}

fn lc(l: SeatLifecycle) -> impl Fn(&EventBody) -> bool {
    move |b| matches!(b, EventBody::SeatLifecycleChanged { lifecycle, .. } if *lifecycle == l)
}

fn set_lifecycle(st: &AppState, seat: &str, l: SeatLifecycle) {
    st.local_event(
        model::Actor::System("test".into()),
        model::Subject::Seat(seat.into()),
        model::Provenance::ProtocolObserved,
        EventBody::SeatLifecycleChanged {
            lifecycle: l,
            reason: Some("test".into()),
        },
    )
    .unwrap();
}

async fn connect_raw(
    url: &str,
    token: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let mut req = url.into_client_request().unwrap();
    req.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

async fn expect_closed_after_hello(url: &str, host_id: &str, protocol: u32) {
    let mut ws = connect_raw(url, TOKEN).await;
    let hello = FromHost::Hello {
        host_id: host_id.into(),
        protocol,
        drivers: vec![],
        live_seats: vec![],
        launching_seats: vec![],
        features: vec![],
        policy: None,
    };
    ws.send(Message::text(host_proto::encode(&hello)))
        .await
        .unwrap();
    let closed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match ws.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(Message::Text(t))) => panic!("must not be welcomed: {t}"),
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok(), "socket was not closed");
}

#[tokio::test]
async fn next_layer_launches_after_previous_layer_is_running() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = supervisor();
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    let a_running = seq_where(&st, "dev.a@r", lc(SeatLifecycle::Running)).unwrap();
    let b_launching = seq_where(&st, "dev.b@r", lc(SeatLifecycle::Launching)).unwrap();
    assert!(
        b_launching > a_running,
        "b launched at {b_launching} before a was running at {a_running}"
    );
}

#[tokio::test]
async fn protocol_mismatch_closes_the_socket() {
    let (_st, url) = serve().await;
    expect_closed_after_hello(&url, "local", PROTOCOL_VERSION + 1).await;
}

#[tokio::test]
async fn reserved_host_id_closes_the_socket() {
    let (st, url) = serve().await;
    expect_closed_after_hello(&url, controld::store::LOCAL_EVENT_HOST, PROTOCOL_VERSION).await;
    assert!(!st.hosts.is_connected(controld::store::LOCAL_EVENT_HOST));
}

#[tokio::test]
async fn send_without_host_records_message_failed() {
    let (st, _url) = serve().await;
    boot_rig(&st, SPEC);
    let err = reconciler::send_message(
        &st,
        "dev.a@r",
        "hi".into(),
        model::MessageIntent::Inform,
        false,
    )
    .await
    .expect_err("no host");
    assert_eq!(err.status, axum::http::StatusCode::BAD_GATEWAY);
    assert_eq!(err.code, "host_unreachable");
    let failed = st
        .store
        .lock()
        .unwrap()
        .events_for_seat("dev.a@r", 0, 100)
        .unwrap()
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::MessageFailed { message_id, .. } => Some(message_id),
            _ => None,
        })
        .expect("a MessageFailed event");
    assert_eq!(
        st.store.lock().unwrap().message_state(&failed).unwrap(),
        Some(model::MessageState::Failed)
    );
}

#[tokio::test]
async fn launching_and_stopping_seats_go_stale_on_link_loss() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = supervisor();
    let link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    set_lifecycle(&st, "dev.a@r", SeatLifecycle::Launching);
    set_lifecycle(&st, "dev.b@r", SeatLifecycle::Stopping);
    link.abort();
    crate::common::wait_for("both stale", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Stale)
            && crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Stale)
    })
    .await;
}

#[tokio::test]
async fn interrupt_reaches_the_seat_before_the_instruction() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = supervisor();
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;

    let first = reconciler::send_message(
        &st,
        "dev.a@r",
        "hang".into(),
        model::MessageIntent::Inform,
        false,
    )
    .await
    .unwrap();
    crate::common::wait_for("hang consumed", || {
        st.store.lock().unwrap().message_state(&first).unwrap()
            == Some(model::MessageState::Consumed)
    })
    .await;
    let second = reconciler::send_message(
        &st,
        "dev.a@r",
        "next".into(),
        model::MessageIntent::Instruct,
        true,
    )
    .await
    .unwrap();
    crate::common::wait_for("instruction consumed", || {
        st.store.lock().unwrap().message_state(&second).unwrap()
            == Some(model::MessageState::Consumed)
    })
    .await;
    let events = st
        .store
        .lock()
        .unwrap()
        .events_for_seat("dev.a@r", 0, 1000)
        .unwrap();
    let interrupted = events
        .iter()
        .find(|e| matches!(&e.body, EventBody::TurnEnded { turn_id, status: TurnStatus::Interrupted } if turn_id == "turn-1"))
        .expect("turn-1 interrupted")
        .seq;
    let consumed = events
        .iter()
        .find(|e| matches!(&e.body, EventBody::MessageConsumed { message_id, turn_id } if *message_id == second && turn_id == "turn-2"))
        .expect("turn-2 consumed the instruction")
        .seq;
    assert!(interrupted < consumed);
}

#[tokio::test]
async fn concurrent_boots_do_not_mark_running_seats_attention() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = supervisor();
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    let p = dir.path().to_path_buf();
    tokio::join!(
        reconciler::boot(st.clone(), rig.clone(), p.clone()),
        reconciler::boot(st.clone(), rig.clone(), p.clone())
    );
    for seat in ["dev.a@r", "dev.b@r"] {
        assert_eq!(
            crate::common::lifecycle(&st, seat),
            Some(SeatLifecycle::Running),
            "{seat}"
        );
        assert!(
            seq_where(&st, seat, lc(SeatLifecycle::Attention)).is_none(),
            "{seat} was marked attention"
        );
    }
    assert!(!st.is_booting("r"));
    // A later boot of a fully running rig relaunches nothing.
    reconciler::boot(st.clone(), rig, p).await;
    assert!(seq_where(&st, "dev.a@r", lc(SeatLifecycle::Attention)).is_none());
}

/// Stub driver whose launch takes a while, so a stop can land while layer 0 is booting.
struct SlowStub;

#[async_trait::async_trait]
impl Driver for SlowStub {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> model::Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        StubDriver.launch(spec).await
    }
    async fn resume(
        &self,
        spec: LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn Session>, DriverError> {
        StubDriver.resume(spec, native_id).await
    }
}

#[tokio::test]
async fn stop_during_layer_zero_prevents_layer_one() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::new(SlowStub)];
    let sup = Supervisor::new(
        "local".into(),
        drivers,
        Journal::open_in_memory(10_000).unwrap(),
    );
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    let (st2, p) = (st.clone(), dir.path().to_path_buf());
    let booting = tokio::spawn(async move { reconciler::boot(st2, rig, p).await });
    crate::common::wait_for("boot started", || st.is_booting("r")).await;
    reconciler::stop_rig(st.clone(), "r".into()).await.unwrap();
    booting.await.unwrap();
    assert!(
        seq_where(&st, "dev.b@r", lc(SeatLifecycle::Launching)).is_none(),
        "layer 1 launched after stop"
    );
    assert!(!st.is_booting("r"));
    crate::common::wait_for("launched seat stopped", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Stopped)
    })
    .await;
    for seat in ["dev.a@r", "dev.b@r"] {
        assert_ne!(
            crate::common::lifecycle(&st, seat),
            Some(SeatLifecycle::Running),
            "{seat} still running after stop"
        );
    }
}

#[tokio::test]
async fn attention_seat_with_live_process_recovers_on_reconnect_and_can_be_stopped() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = supervisor();
    let (s1, u1) = (sup.clone(), url.clone());
    let link = tokio::spawn(async move { hostd::link::run_link(&u1, TOKEN, &s1).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;

    set_lifecycle(&st, "dev.a@r", SeatLifecycle::Attention);
    link.abort();
    crate::common::wait_for("b stale", || {
        crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Stale)
    })
    .await;
    let (s2, u2) = (sup.clone(), url.clone());
    let _link2 = tokio::spawn(async move { hostd::link::run_link(&u2, TOKEN, &s2).await });
    crate::common::wait_for("a recovered", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Running)
    })
    .await;
    assert_eq!(
        reason(&st, "dev.a@r").as_deref(),
        Some("Reconnected; the agent's process is still running")
    );

    set_lifecycle(&st, "dev.a@r", SeatLifecycle::Attention);
    reconciler::stop_rig(st.clone(), "r".into()).await.unwrap();
    crate::common::wait_for("seats stopped", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Stopped)
            && crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Stopped)
    })
    .await;
}

// ---- fix round 2 ----

/// Wraps the stub so a session's first event (SessionReady) arrives 300ms late: the
/// Launch command returns while the seat is still Launching.
struct DelayedReady;

struct DelayedSession(Box<dyn Session>);

#[async_trait::async_trait]
impl Session for DelayedSession {
    async fn send(&mut self, msg: drivers::OutboundMessage) -> Result<(), DriverError> {
        self.0.send(msg).await
    }
    async fn interrupt(&mut self) -> Result<(), DriverError> {
        self.0.interrupt().await
    }
    async fn stop(&mut self, grace: std::time::Duration) -> Result<(), DriverError> {
        self.0.stop(grace).await
    }
    fn take_events(&mut self) -> Option<futures::stream::BoxStream<'static, drivers::DriverEvent>> {
        let inner = self.0.take_events()?;
        Some(
            inner
                .enumerate()
                .then(|(i, e)| async move {
                    if i == 0 {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    }
                    e
                })
                .boxed(),
        )
    }
}

#[async_trait::async_trait]
impl Driver for DelayedReady {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> model::Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        Ok(Box::new(DelayedSession(StubDriver.launch(spec).await?)))
    }
    async fn resume(
        &self,
        spec: LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn Session>, DriverError> {
        Ok(Box::new(DelayedSession(
            StubDriver.resume(spec, native_id).await?,
        )))
    }
}

async fn delayed_host(st: &AppState, url: String) {
    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::new(DelayedReady)];
    let sup = Supervisor::new(
        "local".into(),
        drivers,
        Journal::open_in_memory(10_000).unwrap(),
    );
    tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
}

#[tokio::test]
async fn layer_wait_holds_next_layer_until_previous_is_running() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    delayed_host(&st, url).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    let a_running = seq_where(&st, "dev.a@r", lc(SeatLifecycle::Running)).unwrap();
    let b_launching = seq_where(&st, "dev.b@r", lc(SeatLifecycle::Launching)).unwrap();
    assert!(
        b_launching > a_running,
        "b launched at {b_launching} before a was running at {a_running}"
    );
}

#[tokio::test]
async fn duplicate_boot_is_an_immediate_no_op() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    delayed_host(&st, url).await;
    let (st1, r1, p1) = (st.clone(), rig.clone(), dir.path().to_path_buf());
    let first = tokio::spawn(async move { reconciler::boot(st1, r1, p1).await });
    crate::common::wait_for("boot started", || st.is_booting("r")).await;
    let started = std::time::Instant::now();
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert!(
        started.elapsed() < std::time::Duration::from_millis(150),
        "duplicate boot waited: {:?}",
        started.elapsed()
    );
    first.await.unwrap();
    for seat in ["dev.a@r", "dev.b@r"] {
        let launches = st
            .store
            .lock()
            .unwrap()
            .events_for_seat(seat, 0, 1000)
            .unwrap()
            .into_iter()
            .filter(|e| lc(SeatLifecycle::Launching)(&e.body))
            .count();
        assert_eq!(launches, 1, "{seat}");
        assert_eq!(
            crate::common::lifecycle(&st, seat),
            Some(SeatLifecycle::Running)
        );
    }
}

#[tokio::test]
async fn send_to_seat_not_running_on_connected_host_fails_end_to_end() {
    let (st, url) = serve().await;
    boot_rig(&st, SPEC);
    let sup = supervisor();
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    let err = reconciler::send_message(
        &st,
        "dev.a@r",
        "hi".into(),
        model::MessageIntent::Inform,
        false,
    )
    .await
    .expect_err("seat has no process");
    assert_eq!(err.status, axum::http::StatusCode::BAD_GATEWAY);
    assert!(err.message.contains("not running"), "{}", err.message);
    let failed = st
        .store
        .lock()
        .unwrap()
        .events_for_seat("dev.a@r", 0, 100)
        .unwrap()
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::MessageFailed { message_id, reason } => Some((message_id, reason)),
            _ => None,
        })
        .expect("MessageFailed recorded");
    assert!(failed.1.contains("not running"));
}

// ---- final fix wave ----

/// I8: controld's cursor for the host is 50 but hostd's journal is fresh. The new
/// events must still be ingested (the journal fast-forwards to 51).
#[tokio::test]
async fn reset_host_journal_events_are_still_ingested() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let old = host_proto::HostEvent {
        host_seq: 50,
        at_ms: 1,
        actor: model::Actor::System("hostd".into()),
        subject: model::Subject::Host("local".into()),
        provenance: model::Provenance::ProtocolObserved,
        body: EventBody::EventsLost { count: 0 },
    };
    st.store.lock().unwrap().ingest("local", old).unwrap();
    assert_eq!(st.store.lock().unwrap().host_cursor("local").unwrap(), 50);
    let sup = supervisor();
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reconciler::boot(st.clone(), rig, dir.path().to_path_buf()),
    )
    .await
    .expect("boot settles quickly once events are ingested");
    assert_eq!(
        crate::common::lifecycle(&st, "dev.a@r"),
        Some(SeatLifecycle::Running)
    );
    let first = st
        .store
        .lock()
        .unwrap()
        .events_for_seat("dev.a@r", 0, 100)
        .unwrap()
        .into_iter()
        .find(|e| e.host == "local")
        .unwrap();
    assert_eq!(first.host_seq, 51);
}

fn provenance(st: &AppState, seat: &str) -> Option<model::Provenance> {
    st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .and_then(|s| s.lifecycle_provenance)
}

/// M1: lifecycle changes controld infers are heuristic; the host's own are protocol-observed.
#[tokio::test]
async fn inferred_lifecycle_changes_are_labelled_heuristic() {
    use model::Provenance::{Heuristic, ProtocolObserved};
    // No driver for the runtime, and no host connected.
    let (st, _url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(
        &st,
        &SPEC.replace(
            "runtime: stub, cwd: \".\" }\n      - { id: b",
            "runtime: pi, cwd: \".\" }\n      - { id: b",
        ),
    );
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert_eq!(provenance(&st, "dev.a@r"), Some(Heuristic), "no driver");
    assert_eq!(
        provenance(&st, "dev.b@r"),
        Some(Heuristic),
        "host not connected"
    );

    // Running comes from the host; unreachable and reconnected-without-process are inferred.
    let (st, url) = serve().await;
    let rig = boot_rig(&st, SPEC);
    let (s1, u1) = (supervisor(), url.clone());
    let link = tokio::spawn(async move { hostd::link::run_link(&u1, TOKEN, &s1).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert_eq!(provenance(&st, "dev.a@r"), Some(ProtocolObserved));
    link.abort();
    crate::common::wait_for("a stale", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Stale)
    })
    .await;
    assert_eq!(
        provenance(&st, "dev.a@r"),
        Some(Heuristic),
        "host unreachable"
    );
    let (s2, u2) = (supervisor(), url.clone());
    let _link2 = tokio::spawn(async move { hostd::link::run_link(&u2, TOKEN, &s2).await });
    crate::common::wait_for("a attention", || {
        crate::common::lifecycle(&st, "dev.a@r") == Some(SeatLifecycle::Attention)
    })
    .await;
    assert_eq!(
        provenance(&st, "dev.a@r"),
        Some(Heuristic),
        "reconnected without this seat's process"
    );
}

/// I7b: stopping an attention seat whose process is gone records it as stopped,
/// instead of leaving it in attention for good.
#[tokio::test]
async fn stopping_an_attention_seat_with_no_process_marks_it_stopped() {
    let (st, url) = serve().await;
    boot_rig(&st, SPEC);
    let sup = supervisor();
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", "stub")).await;
    set_lifecycle(&st, "dev.a@r", SeatLifecycle::Attention);
    set_lifecycle(&st, "dev.b@r", SeatLifecycle::Attention);

    reconciler::stop_seat(&st, "dev.a@r").await.unwrap();
    assert_eq!(
        crate::common::lifecycle(&st, "dev.a@r"),
        Some(SeatLifecycle::Stopped)
    );
    assert_eq!(
        reason(&st, "dev.a@r").as_deref(),
        Some("stopped by operator; no process was running")
    );

    let failed = reconciler::stop_rig(st.clone(), "r".into()).await.unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(
        crate::common::lifecycle(&st, "dev.b@r"),
        Some(SeatLifecycle::Stopped)
    );
}

/// A runner that is connected but has no driver for the agent's type says so in plain words.
#[tokio::test]
async fn a_connected_runner_without_the_driver_says_what_it_cannot_run() {
    let (st, url) = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let rig = boot_rig(&st, SPEC);
    let sup = Supervisor::new(
        "local".into(),
        Vec::new(),
        Journal::open_in_memory(10_000).unwrap(),
    );
    let _link = tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || st.hosts.is_connected("local")).await;
    reconciler::boot(st.clone(), rig, dir.path().to_path_buf()).await;
    assert_eq!(
        crate::common::lifecycle(&st, "dev.b@r"),
        Some(SeatLifecycle::Attention)
    );
    assert_eq!(
        reason(&st, "dev.b@r").as_deref(),
        Some("Sculptured's agent runner can't run Practice agents")
    );
}
