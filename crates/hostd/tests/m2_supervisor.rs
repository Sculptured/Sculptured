//! Restarts, answers and seat-tools tokens: live tests on the real Claude Code and Codex CLIs, and
//! plain ones for launches that fail before any agent starts.
mod common;

use common::{CLAUDE, Capture, require, require_codex};
use drivers::Driver;
use host_proto::{HostCommand, HostEvent, LaunchMode, LaunchParams, SeatToolsParams};
use hostd::supervisor::SeatToolsHost;
use hostd::{Journal, Supervisor};

use model::{Answer, EventBody, MessageIntent, PermissionPosture, SeatActivity, TenureCause};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::sync::broadcast::Receiver;

const SEAT: &str = "dev.a@r";
const CODEX: &str = "codex-app-server";
const OK: &str = "Reply with the single word OK and nothing else.";

fn idle(b: &EventBody) -> bool {
    matches!(
        b,
        EventBody::SeatActivityChanged {
            activity: SeatActivity::Idle
        }
    )
}
fn turn_ended(b: &EventBody) -> bool {
    matches!(b, EventBody::TurnEnded { .. })
}

fn sup_of(driver: Arc<dyn Driver>, tools_in: Option<&Path>) -> Supervisor {
    let host = tools_in.map(|d| SeatToolsHost {
        socket: d.join("seat-tools.sock"),
        shim: "/opt/sculptured".into(),
    });
    Supervisor::with_seat_tools(
        "local".into(),
        vec![driver],
        Journal::open_in_memory(1000).unwrap(),
        host,
    )
}

fn send(text: &str) -> HostCommand {
    HostCommand::Send {
        seat: SEAT.into(),
        message_id: "m1".into(),
        intent: MessageIntent::Inform,
        text: text.into(),
    }
}

fn stop(grace_ms: u64) -> HostCommand {
    HostCommand::Stop {
        seat: SEAT.into(),
        grace_ms,
        hard: false,
    }
}

/// The native session id a tenure started with.
fn native_id(got: &[EventBody]) -> String {
    got.iter()
        .find_map(|b| match b {
            EventBody::TenureStarted {
                native_session_id: Some(n),
                ..
            } => Some(n.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a tenure with a session id: {got:?}"))
}

/// Launches `driver` in `cwd`, has it answer one tiny message, stops it, and returns its session id.
async fn one_turn(
    s: &Supervisor,
    rx: &mut Receiver<HostEvent>,
    driver: &str,
    p: LaunchParams,
) -> String {
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: driver.into(),
        params: p,
    })
    .await
    .unwrap();
    let got = common::until(rx, idle).await;
    let id = native_id(&got);
    s.handle(send(OK)).await.unwrap();
    common::until(rx, turn_ended).await;
    s.handle(stop(1000)).await.unwrap();
    id
}

fn tools_params(cwd: &Path) -> LaunchParams {
    let mut p = common::params(cwd);
    p.seat_tools = Some(SeatToolsParams {
        approve: true,
        workflow: true,
    });
    p.posture = PermissionPosture::Restricted;
    p.usage_baseline = Some([10, 20, 30, 40]);
    p
}

fn config_of(c: &Capture) -> std::path::PathBuf {
    c.spec()
        .seat_tools
        .expect("seat tools")
        .config_file
        .expect("a config file")
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

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_restart_resumes_with_the_given_cause() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = common::sup();
    let mut rx = s.subscribe();
    let old = one_turn(&s, &mut rx, CLAUDE, common::params(dir.path())).await;
    s.handle(HostCommand::Restart {
        seat: SEAT.into(),
        driver: CLAUDE.into(),
        params: common::params(dir.path()),
        mode: LaunchMode::Resume {
            native_session_id: old.clone(),
        },
        cause: TenureCause::Resume,
    })
    .await
    .unwrap();
    let got = common::until(&mut rx, idle).await;
    assert!(got.iter().any(|b| matches!(b, EventBody::TenureStarted { tenure: 2, cause: TenureCause::Resume, native_session_id: Some(n), .. } if *n == old)), "{got:?}");
}

/// Claude Code cannot fork a session; Codex can.
#[tokio::test]
#[ignore = "live: real codex CLI; run with --ignored"]
async fn live_fork_restart_gets_a_new_session() {
    let Some(_) = require_codex() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup_of(Arc::new(common::codex(false)), None);
    let mut rx = s.subscribe();
    let p = LaunchParams {
        cwd: dir.path().display().to_string(),
        ..Default::default()
    };
    let old = one_turn(&s, &mut rx, CODEX, p.clone()).await;
    s.handle(HostCommand::Restart {
        seat: SEAT.into(),
        driver: CODEX.into(),
        params: p,
        mode: LaunchMode::Fork {
            native_session_id: old.clone(),
        },
        cause: TenureCause::Fork,
    })
    .await
    .unwrap();
    let got = common::until(&mut rx, idle).await;
    assert!(got.iter().any(|b| matches!(b, EventBody::TenureStarted { cause: TenureCause::Fork, native_session_id: Some(n), .. } if *n != old)), "{got:?}");
}

#[tokio::test]
async fn an_answer_to_a_seat_that_is_not_running_is_refused() {
    let err = common::sup()
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
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_answer_the_session_does_not_take_is_refused() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = common::sup();
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: CLAUDE.into(),
        params: common::params(dir.path()),
    })
    .await
    .unwrap();
    common::until(&mut rx, idle).await;
    // Claude Code asks no native questions, so there is nothing to answer.
    let err = s
        .handle(HostCommand::Answer {
            seat: SEAT.into(),
            question_id: "q".into(),
            answer: Answer::Allow,
        })
        .await
        .unwrap_err();
    assert!(err.contains("answer"), "{err}");
}

/// A restricted Codex agent asks before running a command that changes files; the answer reaches it
/// and its turn goes on.
#[tokio::test]
#[ignore = "live: real codex CLI; run with --ignored"]
async fn live_answer_reaches_the_session() {
    let Some(_) = require_codex() else { return };
    let dir = tempfile::tempdir().unwrap();
    let s = sup_of(Arc::new(common::codex(true)), None);
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: CODEX.into(),
        params: LaunchParams {
            cwd: dir.path().display().to_string(),
            posture: PermissionPosture::Restricted,
            ..Default::default()
        },
    })
    .await
    .unwrap();
    common::until(&mut rx, idle).await;
    s.handle(send(
        "Use your shell tool to run exactly this command in the current folder: touch made.txt",
    ))
    .await
    .unwrap();
    let got = common::until(&mut rx, |b| {
        matches!(b, EventBody::QuestionRaised { .. }) || turn_ended(b)
    })
    .await;
    let Some(EventBody::QuestionRaised { question_id, .. }) = got.last() else {
        panic!("the agent asked before running the command: {got:?}")
    };
    s.handle(HostCommand::Answer {
        seat: SEAT.into(),
        question_id: question_id.clone(),
        answer: Answer::Allow,
    })
    .await
    .unwrap();
    // Further questions in the same turn are allowed too, until it ends.
    loop {
        let got = common::until(&mut rx, |b| {
            matches!(b, EventBody::QuestionRaised { .. }) || turn_ended(b)
        })
        .await;
        match got.last() {
            Some(EventBody::QuestionRaised { question_id, .. }) => s
                .handle(HostCommand::Answer {
                    seat: SEAT.into(),
                    question_id: question_id.clone(),
                    answer: Answer::Allow,
                })
                .await
                .unwrap(),
            _ => break,
        }
    }
}

/// The CLI cannot start (its program does not exist) after the driver wrote the config file: the
/// token and the file both go. The launch is held first to see the token valid while it runs.
#[tokio::test]
async fn the_token_is_forgotten_when_the_launch_fails() {
    let dir = tempfile::tempdir().unwrap();
    let hold = Arc::new(Notify::new());
    let c = Capture {
        hold: Some(hold.clone()),
        ..Capture::claude()
    };
    let s = sup_of(Arc::new(c.clone()), Some(dir.path()));
    let s2 = s.clone();
    let mut p = tools_params(dir.path());
    p.program = Some(dir.path().join("no-such-claude").display().to_string());
    let launch = tokio::spawn(async move {
        s2.handle(HostCommand::Launch {
            seat: SEAT.into(),
            driver: CLAUDE.into(),
            params: p,
        })
        .await
    });
    wait_until("the driver got the spec", || {
        c.seen.lock().unwrap().is_some()
    })
    .await;
    let token = c.token();
    assert_eq!(
        s.seat_for_token(&token).as_deref(),
        Some(SEAT),
        "valid while the launch runs"
    );
    hold.notify_one();
    let err = launch.await.unwrap().unwrap_err();
    assert!(err.contains("no-such-claude"), "{err}");
    assert_eq!(
        s.seat_for_token(&token),
        None,
        "a failed launch leaves no token behind"
    );
    assert!(!config_of(&c).exists(), "nor its config file");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_the_token_is_forgotten_when_the_process_exits_by_itself() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::claude();
    let s = sup_of(Arc::new(c.clone()), Some(dir.path()));
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: CLAUDE.into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    common::until(&mut rx, idle).await;
    let token = c.token();
    assert_eq!(s.seat_for_token(&token).as_deref(), Some(SEAT));
    assert!(config_of(&c).exists());
    common::crash(&s, SEAT);
    common::until(&mut rx, |b| matches!(b, EventBody::TenureEnded { .. })).await;
    wait_until("the token is forgotten", || {
        s.seat_for_token(&token).is_none()
    })
    .await;
    assert!(
        !config_of(&c).exists(),
        "the config file goes with the token"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_seat_tools_token_lives_as_long_as_the_process() {
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::claude();
    let s = sup_of(Arc::new(c.clone()), Some(dir.path()));
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: CLAUDE.into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    common::until(&mut rx, idle).await;
    let spec = c.spec();
    assert_eq!(spec.posture, PermissionPosture::Restricted);
    assert_eq!(
        spec.usage_baseline,
        Some([10, 20, 30, 40]),
        "the resume baseline reaches the driver"
    );
    let token = common::token_in(&spec);
    let tools = spec.seat_tools.expect("seat tools");
    assert!(tools.approve);
    assert_eq!(tools.args, vec!["seat-tools".to_string()]);
    assert_eq!(s.seat_for_token(&token).as_deref(), Some(SEAT));
    s.handle(stop(100)).await.unwrap();
    assert_eq!(
        s.seat_for_token(&token),
        None,
        "the token dies with the process"
    );
}

/// The launch is held in the real driver, then fails (its folder does not exist): no agent starts.
#[tokio::test]
async fn a_launching_seat_counts_as_live_for_reconnects() {
    let hold = Arc::new(Notify::new());
    let c = Capture {
        hold: Some(hold.clone()),
        ..Capture::claude()
    };
    let s = sup_of(Arc::new(c), None);
    let s2 = s.clone();
    let launch = tokio::spawn(async move {
        s2.handle(HostCommand::Launch {
            seat: SEAT.into(),
            driver: CLAUDE.into(),
            params: common::params(Path::new("/definitely/not/here")),
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
    assert!(launch.await.unwrap().is_err());
    assert!(
        s.live_seats().await.is_empty(),
        "a failed launch is not live"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_the_config_file_lives_in_a_private_folder_until_stop() {
    use std::os::unix::fs::PermissionsExt;
    let Some(_) = require() else { return };
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::claude();
    let s = sup_of(Arc::new(c.clone()), Some(dir.path()));
    let mut rx = s.subscribe();
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: CLAUDE.into(),
        params: tools_params(dir.path()),
    })
    .await
    .unwrap();
    common::until(&mut rx, idle).await;
    let file = config_of(&c);
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
    s.handle(stop(100)).await.unwrap();
    assert!(!file.exists(), "removed on stop");
}

/// Codex takes the seat-tools config without a file (the thread `config`, over stdin): it gets no
/// file name, hostd makes no folder for one, and the token works.
#[tokio::test]
#[ignore = "live: real codex CLI; run with --ignored"]
async fn live_a_driver_that_takes_no_config_file_gets_none() {
    let Some(_) = require_codex() else { return };
    let dir = tempfile::tempdir().unwrap();
    let c = Capture::of(Arc::new(common::codex(false)));
    let s = sup_of(Arc::new(c.clone()), Some(dir.path()));
    let mut rx = s.subscribe();
    let mut p = tools_params(dir.path());
    p.model = None;
    s.handle(HostCommand::Launch {
        seat: SEAT.into(),
        driver: CODEX.into(),
        params: p,
    })
    .await
    .unwrap();
    common::until(&mut rx, idle).await;
    let spec = c.spec();
    let token = common::token_in(&spec);
    assert_eq!(spec.seat_tools.expect("seat tools").config_file, None);
    assert!(
        !dir.path().join("seat-tools").exists(),
        "no folder for a file nobody writes"
    );
    assert_eq!(s.seat_for_token(&token).as_deref(), Some(SEAT));
    s.handle(stop(100)).await.unwrap();
    assert_eq!(s.seat_for_token(&token), None, "forgotten on stop");
}
