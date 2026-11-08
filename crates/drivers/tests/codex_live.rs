//! The Codex driver against the real `codex app-server` on the cheapest model: what the hostd
//! supervisor and the handoff dispatcher rely on. Live tests (see `common`): run with
//! `cargo test -p drivers --test codex_live -- --ignored live_`.
mod common;

use common::{
    CODEX_MODEL, Events, cmdline, consumed, ended, exited, gone_within, kill9, long_sleep, marker,
    msg, next, question, require, texts, tokens, tool_call, turn_ended, until, wait_descendants,
};
use drivers::codex::CodexAppServerDriver;
use drivers::{Driver, DriverEvent, LaunchSpec, SeatToolsLaunch, Session};
use model::{Answer, PermissionPosture, QuestionSource, TurnStatus};
use std::path::Path;
use std::time::Duration;

fn driver(program: &Path, native_questions: bool) -> CodexAppServerDriver {
    CodexAppServerDriver {
        program: program.to_path_buf(),
        native_questions,
    }
}

fn spec(dir: &Path) -> LaunchSpec {
    let mut s = LaunchSpec::new("dev.lead@r".parse().unwrap(), dir.to_path_buf());
    s.model = Some(CODEX_MODEL.into());
    s
}

fn restricted(dir: &Path) -> LaunchSpec {
    let mut s = spec(dir);
    s.posture = PermissionPosture::Restricted;
    s
}

async fn ready(s: &mut Box<dyn Session>) -> (Events, String) {
    let mut ev = s.take_events().unwrap();
    match next(&mut ev).await {
        DriverEvent::SessionReady { native_session_id } => (ev, native_session_id),
        other => panic!("{other:?}"),
    }
}

fn hang_prompt(sleep: &str) -> String {
    format!("Run exactly this shell command and wait for it to finish: {sleep}")
}

/// Answers every question with `answer` until the turn ends; returns the events and the question ids.
async fn answer_all(
    s: &mut Box<dyn Session>,
    ev: &mut Events,
    answer: Answer,
) -> (Vec<DriverEvent>, Vec<String>) {
    let mut got = Vec::new();
    let mut asked = Vec::new();
    loop {
        let batch = until(ev, |e| question(e) || turn_ended(e)).await;
        let last = batch.last().cloned();
        got.extend(batch);
        match last {
            Some(DriverEvent::QuestionRaised {
                question_id,
                source,
                ..
            }) => {
                assert_eq!(source, QuestionSource::NativePermission);
                s.answer(&question_id, answer.clone()).await.unwrap();
                asked.push(question_id);
            }
            _ => return (got, asked),
        }
    }
}

#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_a_turn_reads_one_message_and_completes() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&codex, false)
        .launch(spec(dir.path()))
        .await
        .unwrap();
    let (mut ev, thread) = ready(&mut s).await;
    assert!(!thread.is_empty());
    s.send(msg("m1", "Reply with the single word OK and nothing else."))
        .await
        .unwrap();
    let got = until(&mut ev, turn_ended).await;
    let turn = got
        .iter()
        .find_map(|e| match e {
            DriverEvent::TurnStarted { turn_id } => Some(turn_id.clone()),
            _ => None,
        })
        .expect("a turn started");
    assert_eq!(consumed(&got), [("m1".to_string(), turn.clone())]);
    assert!(!texts(&got).is_empty(), "the agent answered: {got:?}");
    assert!(tokens(&got) > 0, "usage was reported: {got:?}");
    assert_eq!(
        got.last(),
        Some(&DriverEvent::TurnEnded {
            turn_id: turn,
            status: TurnStatus::Completed
        })
    );
    s.stop(Duration::from_secs(5)).await.unwrap();
    until(&mut ev, exited).await;
}

#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_a_command_is_a_tool_call_and_its_result() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let word = marker("sculptured-live");
    let mut s = driver(&codex, false)
        .launch(spec(dir.path()))
        .await
        .unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg(
        "m1",
        &format!("Run exactly this shell command: echo {word}\nThen reply with the word done."),
    ))
    .await
    .unwrap();
    let got = until(&mut ev, turn_ended).await;
    let call = got
        .iter()
        .find_map(|e| match e {
            DriverEvent::ToolCall { call_id, input, .. } if input.to_string().contains(&word) => {
                Some(call_id.clone())
            }
            _ => None,
        })
        .expect("the command was a tool call");
    assert!(
        got.iter().any(|e| matches!(e, DriverEvent::ToolResult { call_id, output, .. } if *call_id == call && output.contains(&word))),
        "{got:?}"
    );
    assert_eq!(ended(&got), Some(TurnStatus::Completed));
    s.stop(Duration::from_secs(5)).await.unwrap();
}

/// One message per turn: a message sent while a turn runs waits for `turn/completed`, here of an
/// interrupted turn, and then starts its own turn.
#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_a_message_sent_during_a_turn_waits_for_the_interrupted_turn() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sleep = long_sleep();
    let mut s = driver(&codex, false)
        .launch(spec(dir.path()))
        .await
        .unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", &hang_prompt(&sleep))).await.unwrap();
    let mut got = until(&mut ev, tool_call).await;
    s.send(msg("m2", "Reply with the single word OK and nothing else."))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    s.interrupt().await.unwrap();
    got.extend(until(&mut ev, turn_ended).await);
    assert_eq!(ended(&got), Some(TurnStatus::Interrupted), "{got:?}");
    assert!(
        got.iter()
            .any(|e| matches!(e, DriverEvent::ToolResult { is_error: true, .. })),
        "the open command is closed: {got:?}"
    );
    let first: Vec<(String, String)> = consumed(&got);
    assert_eq!(first.len(), 1, "m2 is not steered into the running turn");
    let next_turn = until(&mut ev, turn_ended).await;
    let mine = consumed(&next_turn);
    assert_eq!(mine.len(), 1, "{next_turn:?}");
    assert_eq!(mine[0].0, "m2");
    assert_ne!(mine[0].1, first[0].1, "one turn per message");
    assert_eq!(ended(&next_turn), Some(TurnStatus::Completed));
    got.extend(next_turn);
    assert!(
        !got.iter()
            .any(|e| matches!(e, DriverEvent::MessageFailed { .. })),
        "{got:?}"
    );
    s.stop(Duration::from_secs(5)).await.unwrap();
}

/// An interrupt right after `send` usually lands before the `turn/start` answer names the turn; it
/// is sent then, so the turn still ends interrupted.
#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_an_interrupt_right_after_send_ends_that_turn() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&codex, false)
        .launch(spec(dir.path()))
        .await
        .unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", &hang_prompt(&long_sleep())))
        .await
        .unwrap();
    s.interrupt().await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert_eq!(ended(&got), Some(TurnStatus::Interrupted), "{got:?}");
    s.stop(Duration::from_secs(5)).await.unwrap();
}

/// An interrupt may leave the command running (Codex does); Stop ends it with the session.
#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_stop_ends_a_command_the_interrupt_left_running() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sleep = long_sleep();
    let mut s = driver(&codex, false)
        .launch(spec(dir.path()))
        .await
        .unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", &hang_prompt(&sleep))).await.unwrap();
    until(&mut ev, tool_call).await;
    let pids = wait_descendants(&sleep, Duration::from_secs(30)).await;
    assert!(!pids.is_empty(), "the agent started `{sleep}`");
    s.interrupt().await.unwrap();
    until(&mut ev, turn_ended).await;
    eprintln!(
        "after the interrupt the command is {}",
        if pids.iter().any(|&p| common::alive(p)) {
            "still running"
        } else {
            "gone"
        }
    );
    s.stop(Duration::from_secs(2)).await.unwrap();
    until(&mut ev, exited).await;
    assert!(
        gone_within(&pids, Duration::from_secs(5)).await,
        "Stop ended the command with the process group"
    );
}

#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_resume_keeps_the_thread_and_fork_gets_a_new_one() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let d = driver(&codex, false);
    let mut s = d.launch(spec(dir.path())).await.unwrap();
    let (mut ev, thread) = ready(&mut s).await;
    s.send(msg(
        "m1",
        "Remember the word lantern. Reply with the single word OK and nothing else.",
    ))
    .await
    .unwrap();
    assert_eq!(
        ended(&until(&mut ev, turn_ended).await),
        Some(TurnStatus::Completed)
    );
    s.stop(Duration::from_secs(5)).await.unwrap();
    until(&mut ev, exited).await;

    let mut s = d.resume(spec(dir.path()), &thread).await.unwrap();
    let (mut ev, resumed) = ready(&mut s).await;
    assert_eq!(resumed, thread);
    s.send(msg(
        "m2",
        "Which word did I ask you to remember? Reply with that word only.",
    ))
    .await
    .unwrap();
    let got = until(&mut ev, turn_ended).await;
    eprintln!("resumed answer: {:?}", texts(&got));
    assert_eq!(ended(&got), Some(TurnStatus::Completed));
    assert!(tokens(&got) > 0, "usage was reported: {got:?}");
    s.stop(Duration::from_secs(5)).await.unwrap();
    until(&mut ev, exited).await;

    let mut s = d.fork(spec(dir.path()), &thread).await.unwrap();
    let (_ev, forked) = ready(&mut s).await;
    assert_ne!(forked, thread);
    s.stop(Duration::from_secs(5)).await.unwrap();
}

#[tokio::test]
#[ignore = "live: real codex CLI (no model turn); run with --ignored"]
async fn live_resuming_an_unknown_thread_fails_to_start_with_the_servers_words() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let unknown = uuid::Uuid::new_v4().to_string();
    let err = match driver(&codex, false)
        .resume(spec(dir.path()), &unknown)
        .await
    {
        Ok(_) => panic!("resume of an unknown thread started"),
        Err(e) => e.to_string(),
    };
    eprintln!("{err}");
    assert!(err.contains("did not start"), "{err}");
}

/// Restricted (`untrusted`): Codex asks before a file change; a denial is passed back and the
/// question resolves.
#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_a_restricted_change_is_a_permission_question_and_deny_holds() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&codex, false)
        .launch(restricted(dir.path()))
        .await
        .unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg(
        "m1",
        "Create the file notes.txt in the current folder containing the word hello. If you are not allowed, reply with the word refused.",
    ))
    .await
    .unwrap();
    let (got, asked) = answer_all(&mut s, &mut ev, Answer::Deny { reason: None }).await;
    assert!(!asked.is_empty(), "Codex asked first: {got:?}");
    assert!(got.contains(&DriverEvent::QuestionResolved {
        question_id: asked[0].clone()
    }));
    assert!(ended(&got).is_some());
    assert!(
        !dir.path().join("notes.txt").exists(),
        "a denied change is not made"
    );
    let err = s.answer(&asked[0], Answer::Allow).await.unwrap_err();
    assert!(err.to_string().contains("answer"), "{err}");
    s.stop(Duration::from_secs(5)).await.unwrap();
}

/// A restricted command asks, runs when allowed, and question ids differ after a resume (server
/// request ids restart at 0 in every app-server process).
#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_a_restricted_command_asks_runs_when_allowed_and_ids_differ_after_resume() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let d = driver(&codex, false);
    let mut s = d.launch(restricted(dir.path())).await.unwrap();
    let (mut ev, thread) = ready(&mut s).await;
    s.send(msg("m1", "Run exactly this shell command: touch a.txt"))
        .await
        .unwrap();
    let (got, first) = answer_all(&mut s, &mut ev, Answer::Allow).await;
    assert!(!first.is_empty(), "Codex asked first: {got:?}");
    assert!(
        got.iter().any(|e| matches!(e, DriverEvent::QuestionRaised { prompt, .. } if prompt.text.contains("touch") || prompt.text.contains("a.txt"))),
        "the question names what it allows: {got:?}"
    );
    assert_eq!(ended(&got), Some(TurnStatus::Completed));
    assert!(dir.path().join("a.txt").exists(), "an allowed command runs");
    s.stop(Duration::from_secs(5)).await.unwrap();
    until(&mut ev, exited).await;

    let mut s = d.resume(restricted(dir.path()), &thread).await.unwrap();
    let (mut ev, resumed) = ready(&mut s).await;
    assert_eq!(resumed, thread, "the resumed session keeps its thread id");
    s.send(msg("m2", "Run exactly this shell command: touch b.txt"))
        .await
        .unwrap();
    let (got, second) = answer_all(&mut s, &mut ev, Answer::Allow).await;
    assert!(!second.is_empty(), "a resume keeps the posture: {got:?}");
    assert!(
        second.iter().all(|q| !first.contains(q)),
        "{first:?} {second:?}"
    );
    s.stop(Duration::from_secs(5)).await.unwrap();
}

#[tokio::test]
async fn native_questions_are_a_capability_of_the_flag() {
    assert!(!CodexAppServerDriver::default().capabilities().questions);
    let d = CodexAppServerDriver {
        native_questions: true,
        ..Default::default()
    };
    assert!(d.capabilities().questions);
    assert!(
        d.args()
            .windows(2)
            .any(|w| w == ["--enable", "default_mode_request_user_input"])
    );
}

#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_native_questions_ask_through_request_user_input() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&codex, true).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", "Before anything else, ask me which colour I prefer using your request_user_input tool, with the two options Blue and Green. Then reply with the colour I chose, nothing else.")).await.unwrap();
    let got = until(&mut ev, |e| question(e) || turn_ended(e)).await;
    let Some(DriverEvent::QuestionRaised {
        question_id,
        source,
        ..
    }) = got.last()
    else {
        panic!("Codex asked: {got:?}")
    };
    assert_eq!(*source, QuestionSource::NativeQuestion);
    s.answer(
        question_id,
        Answer::Text {
            text: "Green".into(),
        },
    )
    .await
    .unwrap();
    let got = until(&mut ev, turn_ended).await;
    eprintln!("answer: {:?}", texts(&got));
    assert_eq!(ended(&got), Some(TurnStatus::Completed));
    s.stop(Duration::from_secs(5)).await.unwrap();
}

/// Codex gets the `rig` server through the thread `config`, over stdin: no config file is written
/// even when one is named, and the token is never on the command line. The `rig` server here is a
/// probe that records the token it was given and exits.
#[tokio::test]
#[ignore = "live: real codex CLI on its cheapest model; run with --ignored"]
async fn live_seat_tools_reach_codex_as_thread_config() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rig.json");
    let probe = dir.path().join("rig-env.txt");
    let token = marker("tok-secret");
    let mut sp = spec(dir.path());
    sp.seat_tools = Some(SeatToolsLaunch {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!("printenv SCULPTURED_SEAT_TOKEN > '{}'", probe.display()),
        ],
        env: vec![("SCULPTURED_SEAT_TOKEN".into(), token.clone())].into(),
        approve: false,
        config_file: Some(file.clone()),
    });
    let d = driver(&codex, false);
    assert!(!d.seat_tools_config_file(), "hostd names no file for Codex");
    let mut s = d.launch(sp).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    let args = cmdline(s.process_id().unwrap());
    assert!(
        !args
            .iter()
            .any(|a| a.contains(&token) || a.contains("mcp_servers")),
        "nothing about the seat tools on the command line: {args:?}"
    );
    s.send(msg("m1", "Reply with the single word OK and nothing else."))
        .await
        .unwrap();
    until(&mut ev, turn_ended).await;
    let t = std::time::Instant::now();
    while !probe.exists() && t.elapsed() < Duration::from_secs(30) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        std::fs::read_to_string(&probe)
            .expect("Codex started the rig server")
            .trim(),
        token
    );
    assert!(!file.exists(), "Codex needs no config file");
    s.stop(Duration::from_secs(5)).await.unwrap();
}

/// A crash, as the session sees it: the app-server is killed and the session ends with `Exited`.
#[tokio::test]
#[ignore = "live: real codex CLI (no model turn); run with --ignored"]
async fn live_a_killed_app_server_ends_the_session_with_exited() {
    let Some(codex) = require("codex") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&codex, false)
        .launch(spec(dir.path()))
        .await
        .unwrap();
    let (mut ev, _) = ready(&mut s).await;
    kill9(s.process_id().unwrap());
    let got = until(&mut ev, exited).await;
    assert!(
        matches!(
            got.last(),
            Some(DriverEvent::Exited {
                signal: Some(9),
                ..
            })
        ),
        "{got:?}"
    );
    assert!(s.send(msg("m2", "too late")).await.is_err());
}
