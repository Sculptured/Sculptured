//! The supervisor: launch, messages, stop and crashes of real Claude Code agents (live tests), and
//! its refusals (plain tests). The slot rules are live too: a real CLI held stopped (SIGSTOP)
//! reads nothing, so a send to it wedges and its stop drains for the whole grace, without any
//! message reaching the model.
mod common;

use common::{CLAUDE, require};
use host_proto::{HostCommand, HostEvent};
use hostd::Supervisor;

use model::{EventBody, MessageIntent, SeatActivity, SeatLifecycle, Subject};
use std::path::Path;
use std::time::Duration;
use tokio::sync::broadcast::Receiver;

const SEAT: &str = "dev.a@r";

fn sup() -> Supervisor {
    common::sup()
}
fn launch_in(cwd: &Path, driver: &str) -> HostCommand {
    HostCommand::Launch {
        seat: SEAT.into(),
        driver: driver.into(),
        params: common::params(cwd),
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
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_launch_reports_launching_tenure_running() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    let got = common::until(&mut rx, is_idle).await;
    assert!(is_lifecycle(&got[0], SeatLifecycle::Launching));
    assert!(
        matches!(&got[1], EventBody::TenureStarted { tenure: 1, driver, native_session_id: Some(n), .. }
        if driver == CLAUDE && !n.is_empty()),
        "{got:?}"
    );
    assert!(is_lifecycle(&got[2], SeatLifecycle::Running));
    assert_eq!(s.live_seats().await, vec![SEAT.to_string()]);
    assert_eq!(s.driver_infos()[0].name, CLAUDE);
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_send_is_delivered_consumed_and_answered() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    common::until(&mut rx, is_idle).await;
    s.handle(send(
        "m1",
        "Reply with the single word OK and nothing else.",
    ))
    .await
    .unwrap();
    let got = common::until(&mut rx, |b| matches!(b, EventBody::TurnEnded { .. })).await;
    let turn = got
        .iter()
        .find_map(|b| match b {
            EventBody::MessageConsumed {
                message_id,
                turn_id,
            } if message_id == "m1" => Some(turn_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("m1 consumed: {got:?}"));
    assert!(
        got.iter()
            .any(|b| matches!(b, EventBody::AgentText { turn_id: Some(t), .. } if *t == turn)),
        "the agent answered in that turn: {got:?}"
    );
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
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_second_launch_is_refused() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    let err = s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap_err();
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
        .handle(launch_in(Path::new("/definitely/not/here"), CLAUDE))
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

/// The agent's CLI process is killed (as a crash would end it).
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_crash_goes_to_attention() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    common::until(&mut rx, is_idle).await;
    common::crash(&s, SEAT);
    let got = common::until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Attention)).await;
    assert!(
        got.iter()
            .any(|b| matches!(b, EventBody::TenureEnded { tenure: 1, .. })),
        "{got:?}"
    );
    match got.last().unwrap() {
        EventBody::SeatLifecycleChanged {
            reason: Some(r), ..
        } => {
            assert!(r.starts_with("exited unexpectedly"), "{r}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stop_then_relaunch_starts_tenure_2() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    common::until(&mut rx, is_idle).await;
    s.handle(HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms: 1000,
        hard: false,
    })
    .await
    .unwrap();
    let got = common::until(&mut rx, |b| is_lifecycle(b, SeatLifecycle::Stopped)).await;
    assert!(got.iter().any(|b| is_lifecycle(b, SeatLifecycle::Stopping)));
    assert!(s.live_seats().await.is_empty());
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    let got = common::until(&mut rx, |b| matches!(b, EventBody::TenureStarted { .. })).await;
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

fn cmd_for(seat: &str, driver: &str, cwd: &Path) -> HostCommand {
    let mut c = launch_in(cwd, driver);
    if let HostCommand::Launch { seat: s, .. } = &mut c {
        *s = seat.into();
    }
    c
}
async fn within<T>(what: &str, f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(20), f)
        .await
        .unwrap_or_else(|_| panic!("{what} timed out"))
}
/// Waits for `seat`'s lifecycle to reach `want`.
async fn until_seat(rx: &mut Receiver<HostEvent>, seat: &str, want: SeatLifecycle) {
    within("lifecycle", async {
        loop {
            let ev = rx.recv().await.expect("open");
            if ev.subject == Subject::Seat(seat.into()) && is_lifecycle(&ev.body, want) {
                return;
            }
        }
    })
    .await
}
/// Stops `seat` on its own task, giving it `grace_ms` to exit.
fn stop_later(
    s: &Supervisor,
    seat: &str,
    grace_ms: u64,
) -> tokio::task::JoinHandle<Result<(), String>> {
    let (s, seat) = (s.clone(), seat.to_string());
    tokio::spawn(async move {
        s.handle(HostCommand::Stop {
            seat,
            grace_ms,
            hard: false,
        })
        .await
    })
}

/// x's CLI is held stopped, so a large send to it blocks on the full pipe; nothing reaches the model.
#[tokio::test]
#[ignore = "live: real claude CLI (no message reaches the model); run with --ignored"]
async fn live_a_wedged_send_does_not_freeze_other_seats_or_its_own_stop() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    let x = "dev.x@r";
    let y = "dev.y@r";
    within("launch x", s.handle(cmd_for(x, CLAUDE, dir.path())))
        .await
        .unwrap();
    until_seat(&mut rx, x, SeatLifecycle::Running).await;
    let _frozen = common::freeze(&s, x);
    let hung = {
        let s = s.clone();
        tokio::spawn(async move {
            s.handle(HostCommand::Send {
                seat: x.into(),
                message_id: "w".into(),
                intent: MessageIntent::Inform,
                text: common::wedging_text(),
            })
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!hung.is_finished(), "the send must be stuck");

    within("launch y", s.handle(cmd_for(y, CLAUDE, dir.path())))
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
        within("the wedged send", hung).await.unwrap().unwrap_err(),
        format!("seat {x} is stopping")
    );
    assert!(s.live_seats().await.is_empty());
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stop_journals_terminal_events_before_returning_and_before_a_relaunch() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    common::until(&mut rx, is_idle).await;
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

    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    common::until(&mut rx, |b| {
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

/// The CLI is held stopped, so it cannot exit on its own and the stop drains for its whole grace.
#[tokio::test]
#[ignore = "live: real claude CLI (no message reaches the model); run with --ignored"]
async fn live_seats_lists_only_live_slots() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    s.handle(launch_in(dir.path(), CLAUDE)).await.unwrap();
    until_seat(&mut rx, SEAT, SeatLifecycle::Running).await;
    assert_eq!(s.live_seats().await, vec![SEAT.to_string()]);
    let _frozen = common::freeze(&s, SEAT);
    let stopper = stop_later(&s, SEAT, 5000);
    until_seat(&mut rx, SEAT, SeatLifecycle::Stopping).await;
    assert!(
        s.live_seats().await.is_empty(),
        "a Stopping seat is not live"
    );
    within("stop", stopper).await.unwrap().unwrap();
}

/// The CLI is held stopped, so its old tenure drains for the whole grace of the stop.
#[tokio::test]
#[ignore = "live: real claude CLI (no message reaches the model); run with --ignored"]
async fn live_launch_is_refused_while_the_old_tenure_drains() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup();
    let mut rx = s.subscribe();
    within("launch", s.handle(launch_in(dir.path(), CLAUDE)))
        .await
        .unwrap();
    until_seat(&mut rx, SEAT, SeatLifecycle::Running).await;
    let frozen = common::freeze(&s, SEAT);
    let stopper = stop_later(&s, SEAT, 5000);
    until_seat(&mut rx, SEAT, SeatLifecycle::Stopping).await;
    let err = within("relaunch", s.handle(launch_in(dir.path(), CLAUDE)))
        .await
        .unwrap_err();
    assert_eq!(err, format!("seat {SEAT} is still stopping"));
    within("stop", stopper).await.unwrap().unwrap();
    drop(frozen);
    let evs = s.unacked_after(0).unwrap();
    assert!(
        evs.iter()
            .any(|e| is_lifecycle(&e.body, SeatLifecycle::Stopped))
    );
    within(
        "relaunch after drain",
        s.handle(launch_in(dir.path(), CLAUDE)),
    )
    .await
    .unwrap();
    within("stop the new tenure", stop_later(&s, SEAT, 1000))
        .await
        .unwrap()
        .unwrap();
}
