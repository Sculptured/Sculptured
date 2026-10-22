//! The Codex driver against fake-codex: what the hostd supervisor and the handoff dispatcher rely on.
use drivers::codex::CodexAppServerDriver;

use drivers::{Driver, DriverEvent, LaunchSpec, OutboundMessage, SeatToolsLaunch, Session};
use futures::StreamExt;
use futures::stream::BoxStream;
use model::{Answer, MessageIntent, PermissionPosture, QuestionSource, TurnStatus};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

type Events = BoxStream<'static, DriverEvent>;

/// How long one event may take. fake-codex answers in milliseconds; the margin is for a loaded
/// machine (a cold `cargo test --workspace` runs many process-spawning tests at once, and 5 s was
/// seen to flake, progress.md Task 3). A passing test never waits this long.
const EVENT_WAIT: Duration = Duration::from_secs(30);

fn driver(native_questions: bool) -> CodexAppServerDriver {
    CodexAppServerDriver {
        program: PathBuf::from(env!("CARGO_BIN_EXE_fake-codex")),
        native_questions,
    }
}
fn spec(dir: &Path) -> LaunchSpec {
    let mut s = LaunchSpec::new("dev.lead@r".parse().unwrap(), dir.to_path_buf());
    s.env = vec![
        (
            "FAKE_CODEX_STATE".into(),
            dir.join("state").display().to_string(),
        ),
        (
            "FAKE_CODEX_LOG".into(),
            dir.join("codex.log").display().to_string(),
        ),
    ]
    .into();
    s
}
fn msg(id: &str, text: &str) -> OutboundMessage {
    OutboundMessage {
        message_id: id.into(),
        intent: MessageIntent::Inform,
        text: text.into(),
    }
}
async fn next(ev: &mut Events) -> DriverEvent {
    tokio::time::timeout(EVENT_WAIT, ev.next())
        .await
        .expect("event in time")
        .expect("stream open")
}
async fn until(ev: &mut Events, pred: impl Fn(&DriverEvent) -> bool) -> Vec<DriverEvent> {
    let mut seen = Vec::new();
    loop {
        let e = next(ev).await;
        let done = pred(&e);
        seen.push(e);
        if done {
            return seen;
        }
    }
}
fn turn_ended(e: &DriverEvent) -> bool {
    matches!(e, DriverEvent::TurnEnded { .. })
}
async fn ready(s: &mut Box<dyn Session>) -> (Events, String) {
    let mut ev = s.take_events().unwrap();
    match next(&mut ev).await {
        DriverEvent::SessionReady { native_session_id } => (ev, native_session_id),
        other => panic!("{other:?}"),
    }
}
/// The JSON of each line fake-codex received whose method is `method`.
fn received(dir: &Path, method: &str) -> Vec<Value> {
    std::fs::read_to_string(dir.join("codex.log"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("in: "))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["method"] == method)
        .collect()
}

#[tokio::test]
async fn a_turn_reads_one_message_and_completes() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, thread) = ready(&mut s).await;
    assert!(thread.starts_with("thr-"), "{thread}");
    s.send(msg("m1", "hello")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    let DriverEvent::TurnStarted { turn_id } = &got[0] else {
        panic!("{got:?}")
    };
    assert_eq!(
        got[1],
        DriverEvent::MessageConsumed {
            message_id: "m1".into(),
            turn_id: turn_id.clone()
        }
    );
    assert!(got.contains(&DriverEvent::Text {
        turn_id: Some(turn_id.clone()),
        text: "codex received: hello".into()
    }));
    assert!(
        got.contains(&DriverEvent::Usage {
            turn_id: Some(turn_id.clone()),
            input_tokens: 60,
            output_tokens: 10,
            cache_read_tokens: 40,
            cache_write_tokens: 0,
            cost: None,
            in_turn: false
        }),
        "fake-model is not priced"
    );
    assert_eq!(
        got.last(),
        Some(&DriverEvent::TurnEnded {
            turn_id: turn_id.clone(),
            status: TurnStatus::Completed
        })
    );
    let start = &received(dir.path(), "turn/start")[0];
    assert_eq!(start["params"]["threadId"], thread.as_str());
    assert_eq!(start["params"]["input"][0]["text"], "hello");
    // D7 unrestricted (the default): approvals off and no sandbox, exactly the M0 probe.
    let thread_start = &received(dir.path(), "thread/start")[0]["params"];
    assert_eq!(
        (
            thread_start["approvalPolicy"].as_str(),
            thread_start["sandbox"].as_str()
        ),
        (Some("never"), Some("danger-full-access"))
    );
}

#[tokio::test]
async fn queued_messages_each_get_their_own_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "tool:ls -1")).await.unwrap();
    s.send(msg("m2", "second")).await.unwrap();
    let mut got = until(&mut ev, turn_ended).await;
    got.extend(until(&mut ev, turn_ended).await);
    let consumed: Vec<(String, String)> = got
        .iter()
        .filter_map(|e| match e {
            DriverEvent::MessageConsumed {
                message_id,
                turn_id,
            } => Some((message_id.clone(), turn_id.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        consumed.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>(),
        ["m1", "m2"]
    );
    assert_ne!(consumed[0].1, consumed[1].1, "one turn per message");
    assert!(
        !got.iter()
            .any(|e| matches!(e, DriverEvent::Unrecognized { .. })),
        "no turn/start was refused: {got:?}"
    );
    assert!(got.iter().any(|e| matches!(e, DriverEvent::ToolCall { name, input, .. } if name == "commandExecution" && input["command"] == "/bin/bash -lc 'ls -1'")));
}

/// Carry-forward (Task 12 ruling C1): a message sent while a turn runs is held by the driver and
/// starts its own turn after `turn/completed`. The real server would steer a mid-turn `turn/start`
/// into the running turn; fake-codex refuses it (-32600), so a mid-turn send would show here.
#[tokio::test]
async fn a_message_sent_during_a_turn_waits_for_turn_completed() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "hang")).await.unwrap();
    let mut got = until(&mut ev, |e| matches!(e, DriverEvent::ToolCall { .. })).await;
    s.send(msg("m2", "after the hang")).await.unwrap();
    // Long enough for a wrongly sent turn/start to reach fake-codex and be logged.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        received(dir.path(), "turn/start").len(),
        1,
        "nothing is sent while the turn runs"
    );
    s.interrupt().await.unwrap();
    got.extend(until(&mut ev, turn_ended).await);
    assert!(matches!(
        got.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Interrupted,
            ..
        })
    ));
    let next_turn = until(&mut ev, turn_ended).await;
    assert!(
        next_turn.iter().any(
            |e| matches!(e, DriverEvent::MessageConsumed { message_id, .. } if message_id == "m2")
        ),
        "{next_turn:?}"
    );
    assert!(next_turn.contains(&DriverEvent::Text {
        turn_id: next_turn.iter().find_map(|e| match e {
            DriverEvent::TurnStarted { turn_id } => Some(turn_id.clone()),
            _ => None,
        }),
        text: "codex received: after the hang".into()
    }));
    got.extend(next_turn);
    assert!(
        !got.iter().any(|e| matches!(
            e,
            DriverEvent::Unrecognized { .. } | DriverEvent::MessageFailed { .. }
        )),
        "{got:?}"
    );
    let starts = received(dir.path(), "turn/start");
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[1]["params"]["input"][0]["text"], "after the hang");
    s.stop(Duration::from_secs(2)).await.unwrap();
}

/// An interrupt right after `send` usually lands before the `turn/start` answer names the turn.
/// It is remembered and sent then, so the turn still ends interrupted (unit-tested exactly in
/// `codex::tests`; here end to end, whichever side of the answer it lands on).
#[tokio::test]
async fn an_interrupt_right_after_send_ends_that_turn() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "hang")).await.unwrap();
    s.interrupt().await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(
        matches!(
            got.last(),
            Some(DriverEvent::TurnEnded {
                status: TurnStatus::Interrupted,
                ..
            })
        ),
        "{got:?}"
    );
    assert_eq!(received(dir.path(), "turn/interrupt").len(), 1);
    s.stop(Duration::from_secs(2)).await.unwrap();
}

#[tokio::test]
async fn interrupt_ends_the_turn_and_the_next_one_runs() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "hang")).await.unwrap();
    until(&mut ev, |e| matches!(e, DriverEvent::ToolCall { .. })).await;
    s.interrupt().await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(
        got.iter()
            .any(|e| matches!(e, DriverEvent::ToolResult { is_error: true, .. })),
        "the open command is closed"
    );
    assert!(matches!(
        got.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Interrupted,
            ..
        })
    ));
    s.send(msg("m2", "after")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(matches!(
        got.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        })
    ));
    s.interrupt().await.unwrap();
}

/// Carry-forward: an interrupt does not stop the command Codex is running (codex-m2-interrupt: the
/// `sleep` outlived `turn/completed: interrupted`). Stop ends the whole process group, so nothing the
/// agent started outlives its session.
#[tokio::test]
async fn stop_ends_a_command_the_interrupt_left_running() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "hang-child")).await.unwrap();
    until(&mut ev, |e| matches!(e, DriverEvent::ToolCall { .. })).await;
    let log = std::fs::read_to_string(dir.path().join("codex.log")).unwrap();
    let pid: i32 = log
        .lines()
        .find_map(|l| l.strip_prefix("child: "))
        .expect("the child's pid")
        .parse()
        .unwrap();
    s.interrupt().await.unwrap();
    until(&mut ev, turn_ended).await;
    assert!(
        alive(pid),
        "the interrupt left the command running, as Codex does"
    );
    s.stop(Duration::from_secs(2)).await.unwrap();
    until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    let deadline = tokio::time::Instant::now() + EVENT_WAIT;
    while alive(pid) && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(pid), "Stop ended the command with the process group");
}

/// A process that exists and is not a zombie.
fn alive(pid: i32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // The state follows the parenthesised command name: `<pid> (<comm>) <state> ...`.
        Ok(stat) => stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_some_and(|st| st != "Z" && st != "X"),
        Err(_) => false,
    }
}

#[tokio::test]
async fn resume_counts_only_new_usage_and_fork_gets_a_new_thread() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, thread) = ready(&mut s).await;
    s.send(msg("m1", "hello")).await.unwrap();
    until(&mut ev, turn_ended).await;
    s.stop(Duration::from_secs(2)).await.unwrap();
    until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;

    let mut s = driver(false)
        .resume(spec(dir.path()), &thread)
        .await
        .unwrap();
    let (mut ev, resumed) = ready(&mut s).await;
    assert_eq!(resumed, thread);
    s.send(msg("m2", "again")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    let usage: Vec<&DriverEvent> = got
        .iter()
        .filter(|e| matches!(e, DriverEvent::Usage { .. }))
        .collect();
    assert_eq!(usage.len(), 1, "the replayed total is a baseline: {got:?}");
    assert!(matches!(
        usage[0],
        DriverEvent::Usage {
            input_tokens: 60,
            output_tokens: 10,
            cache_read_tokens: 40,
            ..
        }
    ));
    let resume = &received(dir.path(), "thread/resume")[0];
    assert_eq!(
        (
            resume["params"]["approvalPolicy"].as_str(),
            resume["params"]["sandbox"].as_str()
        ),
        (Some("never"), Some("danger-full-access"))
    );
    // Carry-forward: `excludeTurns` would change what the first `tokenUsage` replays, and the usage
    // baseline is only proven against full-history resumes (codex-m2-resume@232).
    assert!(resume["params"].get("excludeTurns").is_none(), "{resume}");
    s.stop(Duration::from_secs(2)).await.unwrap();

    let mut s = driver(false).fork(spec(dir.path()), &thread).await.unwrap();
    let (_ev, forked) = ready(&mut s).await;
    assert_ne!(forked, thread);
    let fork = &received(dir.path(), "thread/fork")[0];
    assert_eq!(fork["params"]["threadId"], thread.as_str());
    assert_eq!(
        (
            fork["params"]["approvalPolicy"].as_str(),
            fork["params"]["sandbox"].as_str()
        ),
        (Some("never"), Some("danger-full-access"))
    );
    s.stop(Duration::from_secs(2)).await.unwrap();
}

#[tokio::test]
async fn resuming_an_unknown_thread_fails_to_start_with_the_servers_words() {
    let dir = tempfile::tempdir().unwrap();
    let err = match driver(false).resume(spec(dir.path()), "thr-nope").await {
        Ok(_) => panic!("resume of an unknown thread started"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("thread not found: thr-nope"), "{err}");
}

#[tokio::test]
async fn a_restricted_edit_is_a_permission_question_answered_through_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut sp = spec(dir.path());
    sp.posture = PermissionPosture::Restricted;
    let mut s = driver(false).launch(sp).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "edit:notes.txt")).await.unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::QuestionRaised { .. })).await;
    let Some(DriverEvent::QuestionRaised {
        question_id,
        source,
        prompt,
        ..
    }) = got.last()
    else {
        unreachable!()
    };
    assert_eq!(*source, QuestionSource::NativePermission);
    assert_eq!(prompt.tool.as_deref(), Some("fileChange"));
    assert_eq!(prompt.text, "Allow Codex to change notes.txt?");
    s.answer(question_id, Answer::Deny { reason: None })
        .await
        .unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(got.contains(&DriverEvent::QuestionResolved {
        question_id: question_id.clone()
    }));
    assert!(got.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "codex was not allowed to change notes.txt")));
    assert!(matches!(
        got.last(),
        Some(DriverEvent::TurnEnded {
            status: TurnStatus::Completed,
            ..
        })
    ));
    let err = s.answer(question_id, Answer::Allow).await.unwrap_err();
    assert!(err.to_string().contains("answer"), "{err}");
    // D7 restricted: Codex asks before acting (`untrusted`); posture decides who is asked, not what
    // is reachable, so the sandbox stays `danger-full-access` (plan Task 13; refresh, codex-m2-approval).
    let start = &received(dir.path(), "thread/start")[0];
    assert_eq!(
        (
            start["params"]["approvalPolicy"].as_str(),
            start["params"]["sandbox"].as_str()
        ),
        (Some("untrusted"), Some("danger-full-access"))
    );
}

/// A restricted command (the shape the real model used, codex-m2-approval) asks with the whole command.
#[tokio::test]
async fn a_restricted_command_asks_and_runs_when_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let mut sp = spec(dir.path());
    sp.posture = PermissionPosture::Restricted;
    let mut s = driver(false).launch(sp).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "run:touch a.txt")).await.unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::QuestionRaised { .. })).await;
    let Some(DriverEvent::QuestionRaised {
        question_id,
        prompt,
        ..
    }) = got.last()
    else {
        unreachable!()
    };
    assert_eq!(
        prompt.text,
        "Allow Codex to run `/bin/bash -lc 'touch a.txt'`?"
    );
    s.answer(question_id, Answer::Allow).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(
        got.iter().any(
            |e| matches!(e, DriverEvent::Text { text, .. } if text == "codex ran touch a.txt")
        ),
        "{got:?}"
    );
    s.stop(Duration::from_secs(2)).await.unwrap();
}

#[tokio::test]
async fn native_questions_ask_through_request_user_input() {
    assert!(!driver(false).capabilities().questions);
    assert!(driver(true).capabilities().questions);
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(true).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "ask:Which colour?|Blue|Green"))
        .await
        .unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::QuestionRaised { .. })).await;
    let Some(DriverEvent::QuestionRaised {
        question_id,
        source,
        prompt,
        ..
    }) = got.last()
    else {
        unreachable!()
    };
    assert_eq!(*source, QuestionSource::NativeQuestion);
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["Blue", "Green"]
    );
    s.answer(
        question_id,
        Answer::Text {
            text: "Green".into(),
        },
    )
    .await
    .unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(
        got.iter().any(
            |e| matches!(e, DriverEvent::Text { text, .. } if text == "codex got answer: Green")
        )
    );
    let argv = std::fs::read_to_string(dir.path().join("codex.log")).unwrap();
    assert!(
        argv.starts_with("argv: app-server --enable default_mode_request_user_input"),
        "{argv}"
    );
}

/// Carry-forward (seat-tools config): Codex gets the `rig` server through the thread `config`
/// (dotted keys, confirmed by the 0.160.1 refresh), which travels over stdin. No config file is
/// written even when one is named, and the token is never on the command line.
#[tokio::test]
async fn seat_tools_reach_codex_as_thread_config() {
    let dir = tempfile::tempdir().unwrap();
    let mut sp = spec(dir.path());
    let file = dir.path().join("rig.json");
    sp.seat_tools = Some(SeatToolsLaunch {
        program: "/opt/sculptured".into(),
        args: vec!["seat-tools".into()],
        env: vec![("SCULPTURED_SEAT_TOKEN".into(), "tok-secret".into())].into(),
        approve: false,
        config_file: Some(file.clone()),
    });
    sp.model = Some("fake-model".into());
    assert!(
        !driver(false).seat_tools_config_file(),
        "hostd names no file for Codex"
    );
    let mut s = driver(false).launch(sp).await.unwrap();
    let (_ev, _) = ready(&mut s).await;
    let start = &received(dir.path(), "thread/start")[0]["params"];
    assert_eq!(start["model"], "fake-model");
    assert_eq!(
        start["config"]["mcp_servers.rig.command"],
        "/opt/sculptured"
    );
    assert_eq!(start["config"]["mcp_servers.rig.args"][0], "seat-tools");
    assert_eq!(
        start["config"]["mcp_servers.rig.env"]["SCULPTURED_SEAT_TOKEN"],
        "tok-secret"
    );
    assert_eq!(
        start["config"]["mcp_servers.rig.tool_timeout_sec"], 86_700,
        "longer than the longest question timeout"
    );
    assert_eq!(start["cwd"], dir.path().display().to_string());
    let argv = std::fs::read_to_string(dir.path().join("codex.log")).unwrap();
    let argv = argv.lines().next().unwrap();
    assert_eq!(
        argv, "argv: app-server",
        "nothing about the seat tools on the command line"
    );
    assert!(!file.exists(), "Codex needs no config file");
    s.stop(Duration::from_secs(2)).await.unwrap();
}

/// Server request ids restart at 0 in every app-server process and a resumed session keeps its
/// thread id: the first prompt of the resumed process must not reuse the old process's question id
/// (Task 3 stores questions with `INSERT OR IGNORE`, so a reused id would never reach Needs you).
#[tokio::test]
async fn question_ids_differ_after_resume() {
    let dir = tempfile::tempdir().unwrap();
    let restricted = || {
        let mut sp = spec(dir.path());
        sp.posture = PermissionPosture::Restricted;
        sp
    };
    let raised = |got: &[DriverEvent]| match got.last() {
        Some(DriverEvent::QuestionRaised { question_id, .. }) => question_id.clone(),
        other => panic!("{other:?}"),
    };
    let mut s = driver(false).launch(restricted()).await.unwrap();
    let (mut ev, thread) = ready(&mut s).await;
    s.send(msg("m1", "edit:a.txt")).await.unwrap();
    let first = raised(&until(&mut ev, |e| matches!(e, DriverEvent::QuestionRaised { .. })).await);
    s.answer(&first, Answer::Allow).await.unwrap();
    until(&mut ev, turn_ended).await;
    s.stop(Duration::from_secs(2)).await.unwrap();
    until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;

    let mut s = driver(false).resume(restricted(), &thread).await.unwrap();
    let (mut ev, resumed) = ready(&mut s).await;
    assert_eq!(resumed, thread, "the resumed session keeps its thread id");
    s.send(msg("m2", "edit:b.txt")).await.unwrap();
    let second = raised(&until(&mut ev, |e| matches!(e, DriverEvent::QuestionRaised { .. })).await);
    assert!(
        first.ends_with("-0") && second.ends_with("-0"),
        "both processes number from 0: {first} {second}"
    );
    assert_ne!(first, second);
    s.answer(&second, Answer::Allow).await.unwrap();
    until(&mut ev, turn_ended).await;
    s.stop(Duration::from_secs(2)).await.unwrap();
    let resume = &received(dir.path(), "thread/resume")[0]["params"];
    assert_eq!(
        (
            resume["approvalPolicy"].as_str(),
            resume["sandbox"].as_str()
        ),
        (Some("untrusted"), Some("danger-full-access")),
        "a resume keeps the posture"
    );
}

#[tokio::test]
async fn a_crash_ends_the_session_with_exited() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(false).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "crash")).await.unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    let Some(DriverEvent::Exited {
        exit_code,
        stderr_tail,
        ..
    }) = got.last()
    else {
        unreachable!()
    };
    assert_eq!(*exit_code, Some(3));
    assert!(
        stderr_tail.contains("fake-codex: crash requested"),
        "{stderr_tail}"
    );
    assert!(s.send(msg("m2", "too late")).await.is_err());
}

/// The ONE approved real-CLI probe (user decision 3, Step 12): run once, by hand, after Task 6. It
/// spends two tiny model turns on the user's Codex login and is never part of a routine run.
///
/// The real driver starts `codex app-server` through `codex-probe app-server`, which records every
/// line in the refresh's format to `$RIG_PROBE_OUT` (scrub before committing: `codex-probe scrub`).
/// The `rig` server is the real `sculptured seat-tools` shim, configured through the thread
/// `config`; its socket is a test socket here that records each call and answers like controld.
/// Turn 1 (`thread/start`): Codex calls `request_input`; the socket answers "Blue"; the answer must
/// reach Codex. Turn 2 (`thread/resume`, a fresh token, as on a relaunch): Codex calls `report`.
/// Afterwards it reads (never writes) `~/.codex` for files changed during the probe and says whether
/// they hold the seat token or the `rig` config, printing paths and yes/no only, never the token.
///
/// `RIG_PROBE_DRY=<fake-codex>` checks the plumbing without a model: fake-codex instead of codex,
/// one direct shim call, and no tool-call expectations.
#[tokio::test]
#[ignore = "real Codex CLI, run once: SCULPTURED_REAL_CODEX=1 SCULPTURED_BIN=<target>/debug/sculptured RIG_PROBE_OUT=<raw dir> cargo test -p fixtures --test codex_contract real_codex_probe -- --ignored --nocapture"]
async fn real_codex_probe() {
    use host_proto::seat_tools::{ToolReply, ToolRequest};
    use serde_json::json;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};

    if std::env::var("SCULPTURED_REAL_CODEX").as_deref() != Ok("1") {
        return;
    }
    let dry = std::env::var("RIG_PROBE_DRY").ok();
    let bin = PathBuf::from(
        std::env::var("SCULPTURED_BIN").expect("SCULPTURED_BIN: the built sculptured"),
    );
    let out = PathBuf::from(
        std::env::var("RIG_PROBE_OUT").expect("RIG_PROBE_OUT: the raw recording dir"),
    );
    std::fs::create_dir_all(&out).unwrap();
    // A short /tmp root: a Unix socket path must stay under 108 bytes.
    let root = tempfile::Builder::new()
        .prefix("rcp")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(
        out.join("probe-root.txt"),
        root.path().display().to_string(),
    )
    .unwrap();
    let cwd = root.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let sock = root.path().join("s.sock");
    let began = std::time::SystemTime::now();
    let nonce = began
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let tokens = [format!("probe-a-{nonce:x}"), format!("probe-b-{nonce:x}")];

    // The test socket: one JSON line in, one out, like hostd; every call recorded as the refresh's stub logs.
    let calls: Arc<Mutex<Vec<(String, Value, usize)>>> = Arc::default();
    let listener = UnixListener::bind(&sock).unwrap();
    let socket_log = Arc::new(Mutex::new(
        std::fs::File::create(out.join("codex-m2-seat-shim.seat-socket.jsonl")).unwrap(),
    ));
    {
        let (calls, tokens, log) = (calls.clone(), tokens.clone(), socket_log.clone());
        let t0 = std::time::Instant::now();
        std::thread::spawn(move || {
            let matching_listener = listener.incoming().map_while(Result::ok);
            for conn in matching_listener {
                let mut line = String::new();
                if BufReader::new(&conn).read_line(&mut line).is_err() {
                    continue;
                }
                let rec = |dir: &str, v: Value| {
                    let mut f = log.lock().unwrap();
                    let _ = writeln!(
                        f,
                        "{}",
                        json!({ "t_ms": t0.elapsed().as_millis() as u64, "dir": dir, "line": v })
                    );
                };
                rec(
                    "in",
                    serde_json::from_str(line.trim()).unwrap_or(Value::String(line.clone())),
                );
                let reply = match serde_json::from_str::<ToolRequest>(line.trim()) {
                    Ok(req) => match tokens.iter().position(|t| *t == req.token) {
                        None => ToolReply {
                            ok: false,
                            result: Value::Null,
                            error: Some("this agent's tool token is not valid".into()),
                        },
                        Some(which) => {
                            calls
                                .lock()
                                .unwrap()
                                .push((req.tool.clone(), req.args.clone(), which));
                            let result = match req.tool.as_str() {
                                // controld's shape for an answered question (crates/controld/src/seat_tools.rs).
                                "request_input" => {
                                    json!({ "text": "Blue", "question_id": "probe-q1", "timed_out": false })
                                }
                                "report" => json!({ "text": "Recorded your report." }),
                                _ => json!({ "text": "ok" }),
                            };
                            ToolReply {
                                ok: true,
                                result,
                                error: None,
                            }
                        }
                    },
                    Err(e) => ToolReply {
                        ok: false,
                        result: Value::Null,
                        error: Some(e.to_string()),
                    },
                };
                let reply = serde_json::to_value(&reply).unwrap();
                rec("out", reply.clone());
                let _ = writeln!(&conn, "{reply}");
            }
        });
    }

    let probe = PathBuf::from(env!("CARGO_BIN_EXE_codex-probe"));
    let launch_spec = |token: &str, rec: &str| {
        let mut sp = LaunchSpec::new("dev.probe@r".parse().unwrap(), cwd.clone());
        let env = host_proto::seat_tools::shim_env(&sock, token, false);
        sp.seat_tools = Some(SeatToolsLaunch {
            program: bin.clone(),
            args: vec!["seat-tools".into()],
            env: env.into(),
            approve: false,
            config_file: None,
        });
        sp.program = Some(probe.clone());
        sp.env = vec![(
            "CODEX_PROBE_REC".into(),
            out.join(rec).display().to_string(),
        )]
        .into();
        if let Some(fake) = &dry {
            sp.env.push(("CODEX_PROBE_PROGRAM".into(), fake.clone()));
            sp.env.push((
                "FAKE_CODEX_STATE".into(),
                root.path().join("state").display().to_string(),
            ));
        }
        sp
    };
    async fn turn(ev: &mut Events) -> Vec<DriverEvent> {
        let mut got = Vec::new();
        loop {
            let e = tokio::time::timeout(Duration::from_secs(240), ev.next())
                .await
                .expect("in time")
                .expect("open");
            let end = turn_ended(&e);
            got.push(e);
            if end {
                return got;
            }
        }
    }
    let tool_calls = |got: &[DriverEvent], name: &str| {
        got.iter()
            .filter(|e| matches!(e, DriverEvent::ToolCall { name: n, .. } if n == name))
            .count()
    };
    let results = |got: &[DriverEvent]| -> Vec<(String, bool)> {
        got.iter()
            .filter_map(|e| match e {
                DriverEvent::ToolResult {
                    output, is_error, ..
                } => Some((output.clone(), *is_error)),
                _ => None,
            })
            .collect()
    };
    let texts = |got: &[DriverEvent]| -> Vec<String> {
        got.iter()
            .filter_map(|e| match e {
                DriverEvent::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };

    if dry.is_some() {
        // The shim and the test socket without Codex: initialize, then one call.
        let mut shim = std::process::Command::new(&bin)
            .arg("seat-tools")
            .envs(host_proto::seat_tools::shim_env(&sock, &tokens[0], false))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut sin = shim.stdin.take().unwrap();
        writeln!(sin, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"request_input","arguments":{{"question":"dry?"}}}}}}"#).unwrap();
        let mut reply = String::new();
        BufReader::new(shim.stdout.take().unwrap())
            .read_line(&mut reply)
            .unwrap();
        drop(sin);
        let _ = shim.wait();
        println!("DRY SHIM REPLY: {reply}");
        assert!(reply.contains("Blue"), "{reply}");
    }

    // Turn 1: thread/start with the seat tools; Codex asks through request_input.
    let mut s = CodexAppServerDriver::default()
        .launch(launch_spec(&tokens[0], "codex-m2-seat-shim.jsonl"))
        .await
        .expect("codex app-server starts");
    let mut ev = s.take_events().unwrap();
    let thread = match ev.next().await {
        Some(DriverEvent::SessionReady { native_session_id }) => native_session_id,
        other => panic!("{other:?}"),
    };
    s.send(msg("m1", "Call the tool `request_input` of the MCP server `rig` once, with question \"Which colour?\" and options [\"Blue\", \"Green\"]. Then reply with exactly the answer the tool returned, nothing else.")).await.unwrap();
    let first = turn(&mut ev).await;
    println!("PROBE TURN 1 EVENTS: {first:#?}");
    s.stop(Duration::from_secs(10)).await.unwrap();

    // Turn 2: thread/resume with a fresh token (a relaunch mints one); Codex reports.
    let mut s = CodexAppServerDriver::default()
        .resume(
            launch_spec(&tokens[1], "codex-m2-seat-shim-resume.jsonl"),
            &thread,
        )
        .await
        .expect("resume starts");
    let mut ev = s.take_events().unwrap();
    ev.next().await;
    s.send(msg("m2", "Call the tool `report` of the MCP server `rig` once with summary \"probe\", then reply with exactly the word: ready")).await.unwrap();
    let second = turn(&mut ev).await;
    println!("PROBE TURN 2 EVENTS: {second:#?}");
    s.stop(Duration::from_secs(10)).await.unwrap();

    let calls = calls.lock().unwrap().clone();
    println!("SOCKET CALLS (tool, args, which token): {calls:?}");
    println!(
        "TURN 1: request_input calls {}, results {:?}, texts {:?}",
        tool_calls(&first, "mcp__rig__request_input"),
        results(&first),
        texts(&first)
    );
    println!(
        "TURN 2: report calls {}, results {:?}, texts {:?}",
        tool_calls(&second, "mcp__rig__report"),
        results(&second),
        texts(&second)
    );

    // ~/.codex: read only, files changed since the probe began; paths and yes/no, never the token.
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let codex_home = home.join(".codex");
    let mut found = Vec::new();
    let mut stack = vec![codex_home.clone()];
    while let Some(dir) = stack.pop() {
        let flattened_items = std::fs::read_dir(&dir).into_iter().flatten().flatten();
        for e in flattened_items {
            let (p, Ok(meta)) = (e.path(), e.metadata()) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(p);
            } else if meta.is_file()
                && meta.modified().is_ok_and(|m| m >= began)
                && meta.len() < 1 << 30
            {
                let Ok(bytes) = std::fs::read(&p) else {
                    continue;
                };
                let has =
                    |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
                found.push(json!({
                    "path": p.strip_prefix(&home).map(|r| format!("~/{}", r.display())).unwrap_or_default(),
                    "bytes": meta.len(),
                    "seat_token_a": has(&tokens[0]),
                    "seat_token_b": has(&tokens[1]),
                    "token_env_name": has("SCULPTURED_SEAT_TOKEN"),
                    "rig_config_key": has("mcp_servers.rig"),
                    "thread_id": has(&thread),
                }));
            }
        }
    }
    let persisted = json!({ "files_changed_under_codex_home": found });
    println!("CODEX HOME CHECK: {persisted:#}");
    std::fs::write(out.join("codex-home-check.json"), format!("{persisted:#}")).unwrap();

    if dry.is_none() {
        assert!(
            !first
                .iter()
                .any(|e| matches!(e, DriverEvent::Unrecognized { raw } if raw["name"] == "rig")),
            "rig failed to start"
        );
        assert!(
            tool_calls(&first, "mcp__rig__request_input") >= 1,
            "Codex called request_input"
        );
        assert!(
            calls
                .iter()
                .any(|(t, _, w)| t == "request_input" && *w == 0),
            "the call reached the socket with the first token"
        );
        assert!(
            results(&first)
                .iter()
                .any(|(o, e)| o.contains("Blue") && !e),
            "the answer reached Codex"
        );
        assert!(
            tool_calls(&second, "mcp__rig__report") >= 1,
            "after resume Codex called report"
        );
        assert!(
            calls.iter().any(|(t, _, w)| t == "report" && *w == 1),
            "the resumed thread used the fresh token"
        );
    }
}
