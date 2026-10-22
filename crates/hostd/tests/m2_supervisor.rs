use async_trait::async_trait;
use drivers::stub::StubDriver;
use drivers::{Driver, DriverError, LaunchSpec, Session};
use host_proto::seat_tools::TOKEN_ENV;
use host_proto::{HostCommand, HostEvent, LaunchMode, LaunchParams, SeatToolsParams};
use hostd::supervisor::SeatToolsHost;
use hostd::{Journal, Supervisor};

use model::{Answer, Caps, EventBody, MessageIntent, PermissionPosture, SeatActivity, TenureCause};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::broadcast::Receiver;

const SEAT: &str = "dev.a@r";

fn params(cwd: &Path) -> LaunchParams {
    LaunchParams {
        cwd: cwd.display().to_string(),
        ..Default::default()
    }
}
async fn until(rx: &mut Receiver<HostEvent>, pred: impl Fn(&EventBody) -> bool) -> Vec<EventBody> {
    let mut seen = Vec::new();
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(3), rx.recv())
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
fn idle(b: &EventBody) -> bool {
    matches!(
        b,
        EventBody::SeatActivityChanged {
            activity: SeatActivity::Idle
        }
    )
}
fn stub_sup() -> Supervisor {
    Supervisor::new(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(1000).unwrap(),
    )
}

#[tokio::test]
async fn restart_resumes_with_the_given_cause() {
    let dir = tempfile::tempdir().unwrap();
    let s = stub_sup();
    let mut rx = s.subscribe();
    let mode = LaunchMode::Resume {
        native_session_id: "old-1".into(),
    };
    s.handle(HostCommand::Restart {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: params(dir.path()),
        mode,
        cause: TenureCause::Resume,
    })
    .await
    .unwrap();
    let got = until(&mut rx, idle).await;
    assert!(got.iter().any(|b| matches!(b, EventBody::TenureStarted { cause: TenureCause::Resume, native_session_id: Some(n), .. } if n == "old-1")), "{got:?}");
}

#[tokio::test]
async fn fork_restart_gets_a_new_session() {
    let dir = tempfile::tempdir().unwrap();
    let s = stub_sup();
    let mut rx = s.subscribe();
    let mode = LaunchMode::Fork {
        native_session_id: "old-1".into(),
    };
    s.handle(HostCommand::Restart {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: params(dir.path()),
        mode,
        cause: TenureCause::Fork,
    })
    .await
    .unwrap();
    let got = until(&mut rx, idle).await;
    assert!(got.iter().any(|b| matches!(b, EventBody::TenureStarted { cause: TenureCause::Fork, native_session_id: Some(n), .. } if n.starts_with("stub-fork-"))), "{got:?}");
}

#[tokio::test]
async fn an_answer_the_session_does_not_take_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = stub_sup();
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    // The stub answers questions from Task 3 on; a question it is not asking is refused.
    let err = s
        .handle(HostCommand::Answer {
            seat: SEAT.into(),
            question_id: "q".into(),
            answer: Answer::Allow,
        })
        .await
        .unwrap_err();
    assert!(err.contains("answer"), "{err}");
    let err = s
        .handle(HostCommand::Answer {
            seat: "dev.zz@r".into(),
            question_id: "q".into(),
            answer: Answer::Allow,
        })
        .await
        .unwrap_err();
    assert!(err.contains("not running"), "{err}");
}

#[tokio::test]
async fn answer_reaches_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let rig = Rigged::default();
    let s = rigged_sup(&rig, None);
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    let answer = Answer::Text {
        text: "Blue".into(),
    };
    s.handle(HostCommand::Answer {
        seat: SEAT.into(),
        question_id: "q1".into(),
        answer: answer.clone(),
    })
    .await
    .unwrap();
    assert_eq!(
        *rig.answers.lock().unwrap(),
        vec![("q1".to_string(), answer)]
    );
}

#[tokio::test]
async fn stop_cancels_a_pending_answer() {
    let dir = tempfile::tempdir().unwrap();
    let rig = Rigged {
        answer_hangs: true,
        ..Rigged::default()
    };
    let s = rigged_sup(&rig, None);
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    let s2 = s.clone();
    let pending = tokio::spawn(async move {
        s2.handle(HostCommand::Answer {
            seat: SEAT.into(),
            question_id: "q1".into(),
            answer: Answer::Allow,
        })
        .await
    });
    wait_until("the answer reached the session", || {
        !rig.answers.lock().unwrap().is_empty()
    })
    .await;
    s.handle(HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms: 100,
        hard: false,
    })
    .await
    .unwrap();
    let err = tokio::time::timeout(Duration::from_secs(3), pending)
        .await
        .expect("cancelled in time")
        .unwrap()
        .unwrap_err();
    assert!(err.contains("is stopping"), "{err}");
}

async fn wait_until(what: &str, f: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting: {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A stub-backed driver whose sessions record answers (or hang in them), whose stop can hang, and
/// whose launch can be held and made to fail, so each way a seat-tools token is forgotten can be seen.
#[derive(Clone, Default)]
struct Rigged {
    seen: Arc<Mutex<Option<LaunchSpec>>>,
    answers: Arc<Mutex<Vec<(String, Answer)>>>,
    answer_hangs: bool,
    stop_hangs: bool,
    hold: Option<Arc<Notify>>,
    fail_launch: bool,
}
struct RiggedSession {
    inner: Box<dyn Session>,
    rig: Rigged,
}
#[async_trait]
impl Driver for Rigged {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        *self.seen.lock().unwrap() = Some(spec.clone());
        // Like the Claude driver: the config file is written before the process starts.
        if let Some(path) = spec
            .seat_tools
            .as_ref()
            .and_then(|t| t.config_file.as_ref())
        {
            std::fs::write(path, "{}")?;
        }
        if let Some(h) = &self.hold {
            h.notified().await;
        }
        if self.fail_launch {
            return Err(DriverError::Cwd("rigged to fail".into()));
        }
        Ok(Box::new(RiggedSession {
            inner: StubDriver.launch(spec).await?,
            rig: self.clone(),
        }))
    }
    async fn resume(&self, spec: LaunchSpec, id: &str) -> Result<Box<dyn Session>, DriverError> {
        Ok(Box::new(RiggedSession {
            inner: StubDriver.resume(spec, id).await?,
            rig: self.clone(),
        }))
    }
}
#[async_trait]
impl Session for RiggedSession {
    async fn send(&mut self, msg: drivers::OutboundMessage) -> Result<(), DriverError> {
        self.inner.send(msg).await
    }
    async fn interrupt(&mut self) -> Result<(), DriverError> {
        self.inner.interrupt().await
    }
    async fn stop(&mut self, grace: Duration) -> Result<(), DriverError> {
        if self.rig.stop_hangs {
            std::future::pending::<()>().await;
        }
        self.inner.stop(grace).await
    }
    async fn answer(&mut self, question_id: &str, answer: Answer) -> Result<(), DriverError> {
        self.rig
            .answers
            .lock()
            .unwrap()
            .push((question_id.to_string(), answer));
        if self.rig.answer_hangs {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
    fn take_events(&mut self) -> Option<futures::stream::BoxStream<'static, drivers::DriverEvent>> {
        self.inner.take_events()
    }
}

fn rigged_sup(rig: &Rigged, dir: Option<&Path>) -> Supervisor {
    let host = dir.map(|d| SeatToolsHost {
        socket: d.join("seat-tools.sock"),
        shim: "/opt/sculptured".into(),
    });
    Supervisor::with_seat_tools(
        "local".into(),
        vec![Arc::new(rig.clone())],
        Journal::open_in_memory(1000).unwrap(),
        host,
    )
}

fn tools_params(cwd: &Path) -> LaunchParams {
    let mut p = params(cwd);
    p.seat_tools = Some(SeatToolsParams {
        approve: true,
        workflow: true,
    });
    p.posture = PermissionPosture::Restricted;
    p.usage_baseline = Some([10, 20, 30, 40]);
    p
}

fn config_of(rig: &Rigged) -> std::path::PathBuf {
    rig.seen
        .lock()
        .unwrap()
        .clone()
        .expect("launched")
        .seat_tools
        .expect("seat tools")
        .config_file
        .expect("a config file")
}

fn token_of(rig: &Rigged) -> String {
    let spec = rig.seen.lock().unwrap().clone().expect("launched");
    let tools = spec.seat_tools.expect("seat tools");
    tools
        .env
        .iter()
        .find(|(k, _)| k == TOKEN_ENV)
        .unwrap()
        .1
        .clone()
}

#[tokio::test]
async fn the_token_is_forgotten_when_the_launch_fails() {
    let dir = tempfile::tempdir().unwrap();
    let hold = Arc::new(Notify::new());
    let rig = Rigged {
        hold: Some(hold.clone()),
        fail_launch: true,
        ..Rigged::default()
    };
    let s = rigged_sup(&rig, Some(dir.path()));
    let s2 = s.clone();
    let p = tools_params(dir.path());
    let launch = tokio::spawn(async move {
        s2.handle(HostCommand::Launch {
            seat: SEAT.into(),
            driver: "stub".into(),
            params: p,
        })
        .await
    });
    wait_until("the driver got the spec", || {
        rig.seen.lock().unwrap().is_some()
    })
    .await;
    let token = token_of(&rig);
    assert_eq!(
        s.seat_for_token(&token).as_deref(),
        Some(SEAT),
        "valid while the launch runs"
    );
    hold.notify_one();
    assert!(launch.await.unwrap().is_err());
    assert_eq!(
        s.seat_for_token(&token),
        None,
        "a failed launch leaves no token behind"
    );
    assert!(!config_of(&rig).exists(), "nor its config file");
}

#[tokio::test]
async fn the_token_is_forgotten_when_the_process_exits_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    let rig = Rigged::default();
    let s = rigged_sup(&rig, Some(dir.path()));
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    let token = token_of(&rig);
    assert_eq!(s.seat_for_token(&token).as_deref(), Some(SEAT));
    let send = HostCommand::Send {
        seat: SEAT.into(),
        message_id: "m1".into(),
        intent: MessageIntent::Inform,
        text: "crash".into(),
    };
    s.handle(send).await.unwrap();
    until(&mut rx, |b| matches!(b, EventBody::TenureEnded { .. })).await;
    wait_until("the token is forgotten", || {
        s.seat_for_token(&token).is_none()
    })
    .await;
    assert!(
        !config_of(&rig).exists(),
        "the config file goes with the token"
    );
}

#[tokio::test]
async fn the_token_is_forgotten_when_a_stop_times_out() {
    let dir = tempfile::tempdir().unwrap();
    let rig = Rigged {
        stop_hangs: true,
        ..Rigged::default()
    };
    let s = rigged_sup(&rig, Some(dir.path()));
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    let token = token_of(&rig);
    // The session never stops, so its events never end: only Stop's own cleanup can forget the token.
    let err = s
        .handle(HostCommand::Stop {
            seat: SEAT.into(),
            grace_ms: 0,
            hard: false,
        })
        .await
        .unwrap_err();
    assert_eq!(err, "stop timed out");
    assert_eq!(s.seat_for_token(&token), None);
    assert!(
        !config_of(&rig).exists(),
        "the config file goes with the token"
    );
}

/// Delegates to the stub but keeps the LaunchSpec it was given, and can hold launch until released.
struct Capture {
    seen: Arc<Mutex<Option<LaunchSpec>>>,
    hold: Option<Arc<Notify>>,
}
#[async_trait]
impl Driver for Capture {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        *self.seen.lock().unwrap() = Some(spec.clone());
        if let Some(h) = &self.hold {
            h.notified().await;
        }
        StubDriver.launch(spec).await
    }
    async fn resume(&self, spec: LaunchSpec, id: &str) -> Result<Box<dyn Session>, DriverError> {
        StubDriver.resume(spec, id).await
    }
}

#[tokio::test]
async fn seat_tools_token_lives_as_long_as_the_process() {
    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(None));
    let host = SeatToolsHost {
        socket: dir.path().join("seat-tools.sock"),
        shim: "/opt/sculptured".into(),
    };
    let s = Supervisor::with_seat_tools(
        "local".into(),
        vec![Arc::new(Capture {
            seen: seen.clone(),
            hold: None,
        })],
        Journal::open_in_memory(1000).unwrap(),
        Some(host),
    );
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    let spec = seen.lock().unwrap().clone().unwrap();
    assert_eq!(spec.posture, PermissionPosture::Restricted);
    assert_eq!(
        spec.usage_baseline,
        Some([10, 20, 30, 40]),
        "the resume baseline reaches the driver"
    );
    let tools = spec.seat_tools.expect("seat tools");
    assert!(tools.approve);
    assert_eq!(tools.args, vec!["seat-tools".to_string()]);
    let token = tools
        .env
        .iter()
        .find(|(k, _)| k == TOKEN_ENV)
        .unwrap()
        .1
        .clone();
    assert_eq!(s.seat_for_token(&token).as_deref(), Some(SEAT));
    s.handle(HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms: 100,
        hard: false,
    })
    .await
    .unwrap();
    assert_eq!(
        s.seat_for_token(&token),
        None,
        "the token dies with the process"
    );
}

#[tokio::test]
async fn a_launching_seat_counts_as_live_for_reconnects() {
    let dir = tempfile::tempdir().unwrap();
    let hold = Arc::new(Notify::new());
    let s = Supervisor::new(
        "local".into(),
        vec![Arc::new(Capture {
            seen: Arc::default(),
            hold: Some(hold.clone()),
        })],
        Journal::open_in_memory(1000).unwrap(),
    );
    let s2 = s.clone();
    let cwd = dir.path().to_path_buf();
    let launch = tokio::spawn(async move {
        s2.handle(HostCommand::Launch {
            seat: SEAT.into(),
            driver: "stub".into(),
            params: params(&cwd),
        })
        .await
    });
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while s.live_seats().await.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "launching seat never listed"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    hold.notify_one();
    launch.await.unwrap().unwrap();
    assert_eq!(s.live_seats().await, vec![SEAT.to_string()]);
}

#[tokio::test]
async fn the_config_file_lives_in_a_private_folder_until_stop() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let rig = Rigged::default();
    let s = rigged_sup(&rig, Some(dir.path()));
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    let file = config_of(&rig);
    let folder = dir.path().join("seat-tools");
    assert_eq!(
        file.parent(),
        Some(folder.as_path()),
        "next to the seat-tools socket"
    );
    assert!(
        file.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("dev.a@r."),
        "{file:?}"
    );
    assert_eq!(
        std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(file.exists());
    s.handle(HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms: 100,
        hard: false,
    })
    .await
    .unwrap();
    assert!(!file.exists(), "removed on stop");
}

/// A driver that hands the seat-tools config to its CLI without a file (Codex: the thread
/// `config`, over stdin) gets no file name, and hostd makes no folder for one; the token works.
struct NoFile(Capture);
#[async_trait]
impl Driver for NoFile {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        self.0.launch(spec).await
    }
    async fn resume(&self, spec: LaunchSpec, id: &str) -> Result<Box<dyn Session>, DriverError> {
        self.0.resume(spec, id).await
    }
    fn seat_tools_config_file(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn a_driver_that_takes_no_config_file_gets_none() {
    let dir = tempfile::tempdir().unwrap();
    let seen = Arc::new(Mutex::new(None));
    let host = SeatToolsHost {
        socket: dir.path().join("seat-tools.sock"),
        shim: "/opt/sculptured".into(),
    };
    let s = Supervisor::with_seat_tools(
        "local".into(),
        vec![Arc::new(NoFile(Capture {
            seen: seen.clone(),
            hold: None,
        }))],
        Journal::open_in_memory(1000).unwrap(),
        Some(host),
    );
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: "stub".into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    until(&mut rx, idle).await;
    let tools = seen
        .lock()
        .unwrap()
        .clone()
        .expect("launched")
        .seat_tools
        .expect("seat tools");
    assert_eq!(tools.config_file, None);
    assert!(
        !dir.path().join("seat-tools").exists(),
        "no folder for a file nobody writes"
    );
    let token = tools
        .env
        .iter()
        .find(|(k, _)| k == TOKEN_ENV)
        .unwrap()
        .1
        .clone();
    assert_eq!(s.seat_for_token(&token).as_deref(), Some(SEAT));
    s.handle(HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms: 100,
        hard: false,
    })
    .await
    .unwrap();
    assert_eq!(s.seat_for_token(&token), None, "forgotten on stop");
}
