use async_trait::async_trait;
use drivers::stub::StubDriver;
use drivers::{Driver, DriverError, DriverEvent, LaunchSpec, OutboundMessage, Session};
use futures::stream::{BoxStream, StreamExt};
use host_proto::{HostCommand, HostEvent, LaunchParams};
use hostd::{Journal, Supervisor};

use model::{Caps, EventBody, MessageIntent, SeatActivity, SeatLifecycle, Subject};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::broadcast::Receiver;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;

const SEAT: &str = "dev.a@r";

fn sup() -> Supervisor {
    Supervisor::new(
        "local".into(),
        vec![Arc::new(StubDriver)],
        Journal::open_in_memory(1000).unwrap(),
    )
}
fn launch_in(cwd: &Path, driver: &str) -> HostCommand {
    HostCommand::Launch {
        seat: SEAT.into(),
        driver: driver.into(),
        params: LaunchParams {
            cwd: cwd.display().to_string(),
            ..Default::default()
        },
    }
}
fn send(id: &str, text: &str) -> HostCommand {
    HostCommand::Send {
        seat: SEAT.into(),
        message_id: id.into(),
        intent: MessageIntent::Inform,
        text: text.into(),
    }
}
fn is_lifecycle(b: &EventBody, want: SeatLifecycle) -> bool {
    matches!(b, EventBody::SeatLifecycleChanged { lifecycle, .. } if *lifecycle == want)
}
fn is_idle(b: &EventBody) -> bool {
    matches!(
        b,
        EventBody::SeatActivityChanged {
            activity: SeatActivity::Idle
        }
    )
}

/// Collects event bodies until `pred` matches one; panics after 3 s.
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

#[tokio::test]
async fn launch_reports_launching_tenure_running() {
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    let got = until(&mut rx, is_idle).await;
    assert!(is_lifecycle(&got[0], SeatLifecycle::Launching));
    assert!(
        matches!(&got[1], EventBody::TenureStarted { tenure: 1, driver, native_session_id: Some(n), .. }
        if driver == "stub" && n == "stub-dev.a@r")
    );
    assert!(is_lifecycle(&got[2], SeatLifecycle::Running));
    assert_eq!(s.live_seats().await, vec![SEAT.to_string()]);
    assert_eq!(s.driver_infos()[0].name, "stub");
}

#[tokio::test]
async fn send_is_delivered_consumed_and_answered() {
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    until(&mut rx, is_idle).await;
    s.handle(send("m1", "hello")).await.unwrap();
    let got = until(&mut rx, |b| matches!(b, EventBody::TurnEnded { .. })).await;
    assert!(got.contains(&EventBody::MessageConsumed {
        message_id: "m1".into(),
        turn_id: "turn-1".into()
    }));
    assert!(got.contains(&EventBody::AgentText {
        turn_id: Some("turn-1".into()),
        text: "stub received: hello".into()
    }));
    let all = s.unacked_after(0).unwrap();
    assert!(all.iter().any(|e| e.body
        == EventBody::MessageDelivered {
            message_id: "m1".into()
        }));
    let seqs: Vec<u64> = all.iter().map(|e| e.host_seq).collect();
    assert_eq!(
        seqs,
        (1..=seqs.len() as u64).collect::<Vec<_>>(),
        "journal seqs are contiguous from 1"
    );
}

#[tokio::test]
async fn second_launch_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    let err = s.handle(launch_in(dir.path(), "stub")).await.unwrap_err();
    assert_eq!(err, "seat dev.a@r is already running");
}

#[tokio::test]
async fn unknown_driver_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let err = sup()
        .handle(launch_in(dir.path(), "codex-app"))
        .await
        .unwrap_err();
    assert_eq!(err, "no driver \"codex-app\" on host local");
}

#[tokio::test]
async fn missing_cwd_goes_to_attention_naming_the_path() {
    let s = sup();
    let mut rx = s.subscribe();
    let err = s
        .handle(launch_in(Path::new("/definitely/not/here"), "stub"))
        .await
        .unwrap_err();
    assert!(err.contains("/definitely/not/here"), "{err}");
    let got = until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Attention)).await;
    assert!(is_lifecycle(&got[0], SeatLifecycle::Launching));
    match got.last().unwrap() {
        EventBody::SeatLifecycleChanged {
            reason: Some(r), ..
        } => {
            assert!(r.starts_with("launch failed: "), "{r}");
            assert!(r.contains("/definitely/not/here"), "{r}");
        }
        other => panic!("{other:?}"),
    }
    assert!(s.live_seats().await.is_empty());
}

#[tokio::test]
async fn crash_goes_to_attention() {
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    until(&mut rx, is_idle).await;
    s.handle(send("m1", "crash")).await.unwrap();
    let got = until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Attention)).await;
    assert!(got.iter().any(|b| matches!(
        b,
        EventBody::TenureEnded {
            tenure: 1,
            exit_code: Some(3),
            ..
        }
    )));
    match got.last().unwrap() {
        EventBody::SeatLifecycleChanged {
            reason: Some(r), ..
        } => {
            assert!(r.starts_with("exited unexpectedly (code 3)"), "{r}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn stop_then_relaunch_starts_tenure_2() {
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    until(&mut rx, is_idle).await;
    s.handle(HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms: 1000,
        hard: false,
    })
    .await
    .unwrap();
    let got = until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Stopped)).await;
    assert!(got.iter().any(|b| is_lifecycle(b, SeatLifecycle::Stopping)));
    assert!(s.live_seats().await.is_empty());
    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    let got = until(&mut rx, |b| matches!(b, EventBody::TenureStarted { .. })).await;
    assert!(matches!(
        got.last().unwrap(),
        EventBody::TenureStarted { tenure: 2, .. }
    ));
}

#[tokio::test]
async fn commands_to_a_seat_that_is_not_running_fail() {
    let s = sup();
    assert_eq!(
        s.handle(send("m1", "hi")).await.unwrap_err(),
        "seat dev.a@r is not running"
    );
    assert_eq!(
        s.handle(HostCommand::Interrupt { seat: SEAT.into() })
            .await
            .unwrap_err(),
        "seat dev.a@r is not running"
    );
    assert_eq!(
        s.handle(HostCommand::Stop {
            seat: SEAT.into(),
            grace_ms: 10,
            hard: false
        })
        .await
        .unwrap_err(),
        "seat dev.a@r is not running"
    );
}

/// Test driver: `send` never completes (a wedged agent whose stdin is full). `stop`
/// optionally waits for `release` before emitting `Exited`, to hold the pump open.
struct BlockingDriver {
    release: Option<Arc<Notify>>,
}

struct BlockingSession {
    tx: UnboundedSender<DriverEvent>,
    rx: Option<UnboundedReceiver<DriverEvent>>,
    release: Option<Arc<Notify>>,
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
        Ok(Box::new(BlockingSession {
            tx,
            rx: Some(rx),
            release: self.release.clone(),
        }))
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
        if let Some(r) = &self.release {
            r.notified().await;
        }
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

fn sup_with(release: Option<Arc<Notify>>) -> Supervisor {
    Supervisor::new(
        "local".into(),
        vec![Arc::new(StubDriver), Arc::new(BlockingDriver { release })],
        Journal::open_in_memory(1000).unwrap(),
    )
}
fn cmd_for(seat: &str, driver: &str, cwd: &Path) -> HostCommand {
    let mut c = launch_in(cwd, driver);
    if let HostCommand::Launch { seat: s, .. } = &mut c {
        *s = seat.into();
    }
    c
}
async fn within<T>(what: &str, f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(2), f)
        .await
        .unwrap_or_else(|_| panic!("{what} timed out"))
}

#[tokio::test]
async fn a_wedged_send_does_not_freeze_other_seats_or_its_own_stop() {
    let dir = tempfile::tempdir().unwrap();
    let s = sup_with(None);
    let x = "dev.x@r";
    let y = "dev.y@r";
    within("launch x", s.handle(cmd_for(x, "blocking", dir.path())))
        .await
        .unwrap();
    let hung = {
        let s = s.clone();
        tokio::spawn(async move {
            s.handle(HostCommand::Send {
                seat: x.into(),
                message_id: "w".into(),
                intent: MessageIntent::Inform,
                text: "x".into(),
            })
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!hung.is_finished(), "the send must be stuck");

    within("launch y", s.handle(cmd_for(y, "stub", dir.path())))
        .await
        .unwrap();
    within(
        "send y",
        s.handle(HostCommand::Send {
            seat: y.into(),
            message_id: "m".into(),
            intent: MessageIntent::Inform,
            text: "hi".into(),
        }),
    )
    .await
    .unwrap();
    within("live_seats", s.live_seats()).await;
    within(
        "stop y",
        s.handle(HostCommand::Stop {
            seat: y.into(),
            grace_ms: 500,
            hard: false,
        }),
    )
    .await
    .unwrap();

    within(
        "stop x",
        s.handle(HostCommand::Stop {
            seat: x.into(),
            grace_ms: 500,
            hard: false,
        }),
    )
    .await
    .unwrap();
    let evs = s.unacked_after(0).unwrap();
    assert!(
        evs.iter().any(|e| e.subject == Subject::Seat(x.into())
            && is_lifecycle(&e.body, SeatLifecycle::Stopped))
    );
    assert_eq!(
        hung.await.unwrap().unwrap_err(),
        format!("seat {x} is stopping")
    );
    assert!(s.live_seats().await.is_empty());
}

#[tokio::test]
async fn stop_journals_terminal_events_before_returning_and_before_a_relaunch() {
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    until(&mut rx, is_idle).await;
    s.handle(HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms: 1000,
        hard: false,
    })
    .await
    .unwrap();
    // No await since Stop returned: the old tenure's terminal events must already be journaled.
    let evs = s.unacked_after(0).unwrap();
    assert!(
        evs.iter()
            .any(|e| matches!(e.body, EventBody::TenureEnded { tenure: 1, .. })),
        "{evs:?}"
    );
    let stopped = evs
        .iter()
        .find(|e| is_lifecycle(&e.body, SeatLifecycle::Stopped))
        .expect("Stopped journaled before Stop returned")
        .host_seq;

    s.handle(launch_in(dir.path(), "stub")).await.unwrap();
    until(&mut rx, |b| {
        matches!(b, EventBody::TenureStarted { tenure: 2, .. })
    })
    .await;
    let evs = s.unacked_after(0).unwrap();
    let t2 = evs
        .iter()
        .find(|e| matches!(e.body, EventBody::TenureStarted { tenure: 2, .. }))
        .unwrap();
    assert!(t2.host_seq > stopped);
    assert_eq!(
        evs.iter()
            .filter(|e| is_lifecycle(&e.body, SeatLifecycle::Stopped))
            .count(),
        1,
        "no Stopped after tenure 2 began"
    );
}

#[tokio::test]
async fn live_seats_lists_only_live_slots() {
    let dir = tempfile::tempdir().unwrap();
    let release = Arc::new(Notify::new());
    let s = sup_with(Some(release.clone()));
    let mut rx = s.subscribe();
    s.handle(cmd_for(SEAT, "blocking", dir.path()))
        .await
        .unwrap();
    until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Running)).await;
    assert_eq!(s.live_seats().await, vec![SEAT.to_string()]);
    let stopper = {
        let s = s.clone();
        tokio::spawn(async move {
            s.handle(HostCommand::Stop {
                seat: SEAT.into(),
                grace_ms: 500,
                hard: false,
            })
            .await
        })
    };
    until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Stopping)).await;
    assert!(
        s.live_seats().await.is_empty(),
        "a Stopping seat is not live"
    );
    release.notify_one();
    within("stop", stopper).await.unwrap().unwrap();
}

#[tokio::test]
async fn launch_is_refused_while_the_old_tenure_drains() {
    let dir = tempfile::tempdir().unwrap();
    let release = Arc::new(Notify::new());
    let s = sup_with(Some(release.clone()));
    let mut rx = s.subscribe();
    within("launch", s.handle(cmd_for(SEAT, "blocking", dir.path())))
        .await
        .unwrap();
    until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Running)).await;
    let stopper = {
        let s = s.clone();
        tokio::spawn(async move {
            s.handle(HostCommand::Stop {
                seat: SEAT.into(),
                grace_ms: 500,
                hard: false,
            })
            .await
        })
    };
    until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Stopping)).await;
    let err = within("relaunch", s.handle(cmd_for(SEAT, "blocking", dir.path())))
        .await
        .unwrap_err();
    assert_eq!(err, format!("seat {SEAT} is still stopping"));
    release.notify_one();
    within("stop", stopper).await.unwrap().unwrap();
    let evs = s.unacked_after(0).unwrap();
    assert!(
        evs.iter()
            .any(|e| is_lifecycle(&e.body, SeatLifecycle::Stopped))
    );
    within(
        "relaunch after drain",
        s.handle(cmd_for(SEAT, "blocking", dir.path())),
    )
    .await
    .unwrap();
}
