//! hostd's Hello says what this build can do (`features`) and which seats are still starting, so a
//! controld can refuse what an older hostd would get wrong and never mistake a starting seat for a
//! running one.
use async_trait::async_trait;
use drivers::stub::StubDriver;
use drivers::{Driver, DriverError, LaunchSpec, Session};
use futures::StreamExt;
use host_proto::{FromHost, HostCommand, LaunchParams, features};
use hostd::supervisor::SeatToolsHost;
use hostd::{Journal, Supervisor};
use model::Caps;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::Message;

const SEAT: &str = "dev.a@r";

/// Holds every launch until released.
struct Held(Arc<Notify>);
#[async_trait]
impl Driver for Held {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        self.0.notified().await;
        StubDriver.launch(spec).await
    }
    async fn resume(&self, spec: LaunchSpec, id: &str) -> Result<Box<dyn Session>, DriverError> {
        StubDriver.resume(spec, id).await
    }
}

async fn hello_of(sup: &Supervisor) -> FromHost {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/host", listener.local_addr().unwrap());
    let sup = sup.clone();
    let link = tokio::spawn(async move { hostd::run_link(&url, "secret", &sup).await });
    let (tcp, _) = listener.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
    let hello = loop {
        let m = tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("hello in time")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = m {
            break host_proto::decode::<FromHost>(t.as_str()).unwrap();
        }
    };
    link.abort();
    hello
}

#[tokio::test]
async fn hello_lists_features_and_seat_tools_only_when_offered() {
    let plain = Supervisor::new(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(100).unwrap(),
    );
    let FromHost::Hello { features: f, .. } = hello_of(&plain).await else {
        panic!("not a hello")
    };
    for want in [
        features::POSTURE,
        features::RESTART,
        features::ANSWER,
        features::USAGE_BASELINE,
    ] {
        assert!(f.iter().any(|x| x == want), "missing {want} in {f:?}");
    }
    assert!(
        !f.iter().any(|x| x == features::SEAT_TOOLS),
        "no seat-tools socket, so no seat-tools: {f:?}"
    );

    let dir = tempfile::tempdir().unwrap();
    let host = SeatToolsHost {
        socket: dir.path().join("s.sock"),
        shim: "/opt/sculptured".into(),
    };
    let with = Supervisor::with_seat_tools(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(100).unwrap(),
        Some(host),
    );
    let FromHost::Hello { features: f, .. } = hello_of(&with).await else {
        panic!("not a hello")
    };
    assert!(f.iter().any(|x| x == features::SEAT_TOOLS), "{f:?}");
}

#[tokio::test]
async fn hello_names_seats_that_are_still_launching() {
    let dir = tempfile::tempdir().unwrap();
    let hold = Arc::new(Notify::new());
    let sup = Supervisor::new(
        "local".into(),
        vec![Arc::new(Held(hold.clone()))],
        Journal::open_in_memory(100).unwrap(),
    );
    let s2 = sup.clone();
    let params = LaunchParams {
        cwd: dir.path().display().to_string(),
        ..Default::default()
    };
    let launch = tokio::spawn(async move {
        s2.handle(HostCommand::Launch {
            seat: SEAT.into(),
            driver: "stub".into(),
            params,
        })
        .await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while sup.live_seats().await.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "launch never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let FromHost::Hello {
        live_seats,
        launching_seats,
        ..
    } = hello_of(&sup).await
    else {
        panic!("not a hello")
    };
    assert_eq!(
        (live_seats, launching_seats),
        (vec![SEAT.to_string()], vec![SEAT.to_string()])
    );
    hold.notify_one();
    launch.await.unwrap().unwrap();
    let FromHost::Hello {
        live_seats,
        launching_seats,
        ..
    } = hello_of(&sup).await
    else {
        panic!("not a hello")
    };
    assert_eq!(
        (live_seats, launching_seats),
        (vec![SEAT.to_string()], vec![])
    );
}

#[tokio::test]
async fn a_starting_hostd_sweeps_config_files_left_by_an_earlier_one() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("seat-tools");
    std::fs::create_dir_all(&folder).unwrap();
    let left = folder.join("dev.a@r.1234abcd.json");
    std::fs::write(&left, r#"{"mcpServers":{}}"#).unwrap();
    let other = folder.join("notes.txt");
    std::fs::write(&other, "not ours").unwrap();
    let host = SeatToolsHost {
        socket: dir.path().join("seat-tools.sock"),
        shim: "/opt/sculptured".into(),
    };
    let _sup = Supervisor::with_seat_tools(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(100).unwrap(),
        Some(host),
    );
    assert!(
        !left.exists(),
        "a config file from a previous runner names a dead token"
    );
    assert!(other.exists(), "only config files are swept");
}
