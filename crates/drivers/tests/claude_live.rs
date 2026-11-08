//! The Claude driver against the real `claude` CLI on haiku: what the hostd supervisor relies on.
//! Live tests (see `common`): run with `cargo test -p drivers --test claude_live -- --ignored live_`.
mod common;

use common::{
    CLAUDE_MODEL, Events, cmdline, consumed, descendants_with, ended, environ, exited, gone_within,
    kill9, long_sleep, marker, msg, next, require, texts, tokens, tool_call, turn_ended, until,
    wait_descendants,
};
use drivers::claude::ClaudeStreamDriver;
use drivers::{Driver, DriverEvent, LaunchSpec, MCP_TOOL_TIMEOUT_MS, SeatToolsLaunch, Session};
use model::{PermissionPosture, TurnStatus};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn driver(program: &Path) -> ClaudeStreamDriver {
    ClaudeStreamDriver {
        program: program.to_path_buf(),
    }
}

fn spec(dir: &Path) -> LaunchSpec {
    let mut s = LaunchSpec::new("dev.impl@r".parse().unwrap(), dir.to_path_buf());
    s.model = Some(CLAUDE_MODEL.into());
    s
}

async fn ready(s: &mut Box<dyn Session>) -> (Events, String) {
    let mut ev = s.take_events().unwrap();
    match next(&mut ev).await {
        DriverEvent::SessionReady { native_session_id } => (ev, native_session_id),
        other => panic!("{other:?}"),
    }
}

/// A Bash command the agent runs in the foreground until the test ends the turn.
fn hang_prompt(sleep: &str) -> String {
    format!(
        "Use your Bash tool to run exactly this command in the foreground (not in the background) and wait for it: {sleep}"
    )
}

/// The `sleep` the agent started, once its process exists.
async fn sleeping(sleep: &str) -> Vec<u32> {
    let pids = wait_descendants(sleep, Duration::from_secs(30)).await;
    assert!(!pids.is_empty(), "the agent started `{sleep}`");
    pids
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_turn_answers_reports_usage_and_stop_exits_cleanly() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let mut sp = spec(dir.path());
    sp.native_session_id = Some(id.clone());
    let mut s = driver(&claude).launch(sp).await.unwrap();
    // Ready before any message, although the CLI emits no init until it reads one.
    let (mut ev, native) = ready(&mut s).await;
    assert_eq!(native, id);
    let pid = s.process_id().expect("a process");
    let args = cmdline(pid);
    for a in [
        "--input-format",
        "stream-json",
        "--replay-user-messages",
        "--dangerously-skip-permissions",
        "--session-id",
        id.as_str(),
        "--model",
        CLAUDE_MODEL,
    ] {
        assert!(args.iter().any(|l| l == a), "missing arg {a} in {args:?}");
    }
    if std::env::var_os("MCP_TOOL_TIMEOUT").is_none() {
        assert!(
            !environ(pid).iter().any(|(k, _)| k == "MCP_TOOL_TIMEOUT"),
            "only seat tools raise the MCP tool timeout"
        );
    }
    s.send(msg("m1", "Reply with the single word OK and nothing else."))
        .await
        .unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(got.contains(&DriverEvent::TurnStarted {
        turn_id: "turn-1".into()
    }));
    assert_eq!(consumed(&got), [("m1".to_string(), "turn-1".to_string())]);
    assert!(!texts(&got).is_empty(), "the agent answered: {got:?}");
    assert!(tokens(&got) > 0, "usage was reported: {got:?}");
    assert_eq!(
        got.last().unwrap(),
        &DriverEvent::TurnEnded {
            turn_id: "turn-1".into(),
            status: TurnStatus::Completed
        }
    );
    s.stop(Duration::from_secs(10)).await.unwrap();
    let end = until(&mut ev, exited).await;
    assert!(
        matches!(
            end.last(),
            Some(DriverEvent::Exited {
                exit_code: Some(0),
                ..
            })
        ),
        "closing stdin ends `claude -p`: {end:?}"
    );
    assert!(
        futures::StreamExt::next(&mut ev).await.is_none(),
        "Exited is the last event"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_bash_command_is_a_tool_call_and_its_result() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let word = marker("sculptured-live");
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg(
        "m1",
        &format!("Use your Bash tool to run exactly: echo {word}\nThen reply with the word done."),
    ))
    .await
    .unwrap();
    let got = until(&mut ev, turn_ended).await;
    let call = got
        .iter()
        .find_map(|e| match e {
            DriverEvent::ToolCall {
                name,
                call_id,
                input,
                ..
            } if name == "Bash" && input.to_string().contains(&word) => Some(call_id.clone()),
            _ => None,
        })
        .expect("a Bash call");
    assert!(
        got.iter().any(|e| matches!(e, DriverEvent::ToolResult { call_id, output, is_error: false, .. } if *call_id == call && output.contains(&word))),
        "{got:?}"
    );
    assert_eq!(ended(&got), Some(TurnStatus::Completed));
    s.stop(Duration::from_secs(10)).await.unwrap();
}

/// A message sent while a turn runs waits for the next turn, also when the running turn is interrupted.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_interrupt_ends_the_turn_and_a_queued_message_runs_next() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sleep = long_sleep();
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", &hang_prompt(&sleep))).await.unwrap();
    until(&mut ev, tool_call).await;
    sleeping(&sleep).await;
    s.send(msg("m2", "Reply with the single word OK and nothing else."))
        .await
        .unwrap();
    s.interrupt().await.unwrap();
    let first = until(&mut ev, turn_ended).await;
    assert_eq!(
        first.last().unwrap(),
        &DriverEvent::TurnEnded {
            turn_id: "turn-1".into(),
            status: TurnStatus::Interrupted
        }
    );
    assert!(
        !consumed(&first).iter().any(|(m, _)| m == "m2"),
        "the queued message is not part of the interrupted turn: {first:?}"
    );
    let mut second = until(
        &mut ev,
        |e| matches!(e, DriverEvent::MessageConsumed { message_id, .. } if message_id == "m2"),
    )
    .await;
    second.extend(until(&mut ev, turn_ended).await);
    let (_, turn) = consumed(&second)
        .into_iter()
        .find(|(m, _)| m == "m2")
        .unwrap();
    assert_ne!(turn, "turn-1");
    assert_eq!(
        second.last().unwrap(),
        &DriverEvent::TurnEnded {
            turn_id: turn,
            status: TurnStatus::Completed
        }
    );
    s.stop(Duration::from_secs(10)).await.unwrap();
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_resume_keeps_the_native_session_id() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, id) = ready(&mut s).await;
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
    s.stop(Duration::from_secs(10)).await.unwrap();
    until(&mut ev, exited).await;

    let mut s = driver(&claude).resume(spec(dir.path()), &id).await.unwrap();
    let (mut ev, resumed) = ready(&mut s).await;
    assert_eq!(resumed, id);
    let args = cmdline(s.process_id().unwrap());
    let at = args.iter().position(|a| a == "--resume").expect("--resume");
    assert_eq!(args[at + 1], id);
    s.send(msg(
        "m2",
        "Which word did I ask you to remember? Reply with that word only.",
    ))
    .await
    .unwrap();
    let got = until(&mut ev, turn_ended).await;
    eprintln!("resumed answer: {:?}", texts(&got));
    assert_eq!(consumed(&got), [("m2".to_string(), "turn-1".to_string())]);
    assert!(tokens(&got) > 0, "usage was reported: {got:?}");
    assert_eq!(ended(&got), Some(TurnStatus::Completed));
    s.stop(Duration::from_secs(10)).await.unwrap();
}

#[tokio::test]
#[ignore = "live: real claude CLI (no model turn); run with --ignored"]
async fn live_resuming_an_unknown_session_exits_with_the_clis_words() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let unknown = uuid::Uuid::new_v4().to_string();
    let mut s = driver(&claude)
        .resume(spec(dir.path()), &unknown)
        .await
        .unwrap();
    let (mut ev, _) = ready(&mut s).await;
    // The CLI may check the session before or after it reads a message; either way it exits.
    let _ = s.send(msg("m1", "Reply with OK.")).await;
    let got = until(&mut ev, exited).await;
    match got.last().unwrap() {
        DriverEvent::Exited {
            exit_code: Some(code),
            stderr_tail,
            ..
        } => {
            assert_ne!(*code, 0, "{got:?}");
            assert!(!stderr_tail.trim().is_empty(), "the CLI said why: {got:?}");
        }
        other => panic!("{other:?}"),
    }
}

/// A restricted agent without seat tools has no one to ask: the CLI refuses what needs permission.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_restricted_agent_without_a_prompt_tool_is_refused() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut sp = spec(dir.path());
    sp.posture = PermissionPosture::Restricted;
    let mut s = driver(&claude).launch(sp).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    assert!(
        !cmdline(s.process_id().unwrap())
            .iter()
            .any(|a| a == "--dangerously-skip-permissions")
    );
    s.send(msg(
        "m1",
        "Use your Bash tool to run exactly: touch made.txt\nIf you cannot, reply with the word refused.",
    ))
    .await
    .unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(ended(&got).is_some());
    assert!(
        !dir.path().join("made.txt").exists(),
        "nothing ran without permission: {got:?}"
    );
    s.stop(Duration::from_secs(10)).await.unwrap();
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_subagents_calls_carry_their_parent() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let word = marker("sub");
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", &format!("Use your Agent tool to start one subagent that runs the Bash command `echo {word}` and reports its output. Do not run it yourself. Then reply with the word done."))).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    let parent = got
        .iter()
        .find_map(|e| match e {
            DriverEvent::ToolCall {
                name,
                call_id,
                parent_call_id: None,
                ..
            } if name == "Agent" || name == "Task" => Some(call_id.clone()),
            _ => None,
        })
        .expect("an Agent call");
    assert!(
        got.iter().any(
            |e| matches!(e, DriverEvent::ToolCall { parent_call_id: Some(p), .. } if *p == parent)
        ),
        "the subagent's calls name the Agent call: {got:?}"
    );
    assert!(
        got.iter()
            .any(|e| matches!(e, DriverEvent::ToolResult { call_id, .. } if *call_id == parent))
    );
    assert_eq!(consumed(&got), [("m1".to_string(), "turn-1".to_string())]);
    s.stop(Duration::from_secs(10)).await.unwrap();
}

/// The seat-tools config reaches the CLI as a private file (the token never on its command line),
/// the CLI starts the `rig` server from it, and the long MCP tool timeout is in the CLI's environment.
/// The `rig` server here is a probe that records the token it was given and exits.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_seat_tools_reach_the_cli_as_a_private_config_file() {
    use std::os::unix::fs::PermissionsExt;
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dev.impl@r.json");
    let probe = dir.path().join("rig-env.txt");
    let token = marker("secret-token");
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
    let mut s = driver(&claude).launch(sp).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    let pid = s.process_id().unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let args = cmdline(pid);
    assert!(
        args.iter().any(|a| *a == file.display().to_string()),
        "the path is passed: {args:?}"
    );
    assert!(
        !args.iter().any(|a| a.contains(&token)),
        "the token is not on the command line: {args:?}"
    );
    assert!(
        environ(pid)
            .iter()
            .any(|(k, v)| k == "MCP_TOOL_TIMEOUT" && v == MCP_TOOL_TIMEOUT_MS),
        "a question may hold a tool call for the whole question timeout"
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
            .expect("the CLI started the rig server")
            .trim(),
        token
    );
    s.stop(Duration::from_secs(10)).await.unwrap();
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_claude_setup_reaches_the_cli_as_its_flags() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut sp = spec(dir.path());
    sp.claude_setup = Some(model::ClaudeSetup::default());
    let mut s = driver(&claude).launch(sp).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    let args = cmdline(s.process_id().unwrap());
    let at = args
        .iter()
        .position(|a| a == "--setting-sources")
        .expect("setting sources passed");
    assert_eq!(args[at + 1], "project,local");
    assert!(args.iter().any(|a| a == "--strict-mcp-config"), "{args:?}");
    s.send(msg("m1", "Reply with the single word OK and nothing else."))
        .await
        .unwrap();
    assert_eq!(
        ended(&until(&mut ev, turn_ended).await),
        Some(TurnStatus::Completed),
        "the CLI accepted the flags"
    );
    s.stop(Duration::from_secs(10)).await.unwrap();
}

/// A crash, as the session sees it: the CLI is killed and the session ends with `Exited`.
#[tokio::test]
#[ignore = "live: real claude CLI (no model turn); run with --ignored"]
async fn live_a_killed_cli_ends_the_session_with_exited() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
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
    assert!(futures::StreamExt::next(&mut ev).await.is_none());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(s.send(msg("m2", "too late")).await.is_err());
}

#[tokio::test]
#[ignore = "live: real claude CLI (no model turn); run with --ignored"]
async fn live_dropping_the_session_ends_the_cli() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    let pid = s.process_id().unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    drop(s);
    until(&mut ev, exited).await;
    assert!(gone_within(&[pid], Duration::from_secs(2)).await);
}

/// Stop is bounded while the agent runs a command, and ends that command with the CLI.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stop_during_a_command_ends_the_cli_and_the_command() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sleep = long_sleep();
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", &hang_prompt(&sleep))).await.unwrap();
    until(&mut ev, tool_call).await;
    let pids = sleeping(&sleep).await;
    let started = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(5), s.stop(Duration::from_millis(500)))
        .await
        .expect("stop is bounded")
        .unwrap();
    until(&mut ev, exited).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(
        futures::StreamExt::next(&mut ev).await.is_none(),
        "Exited is the last event"
    );
    assert!(
        gone_within(&pids, Duration::from_secs(2)).await,
        "the command the agent started is gone too: {:?}",
        descendants_with(&sleep)
    );
}

/// Like Ctrl+C in a terminal: the CLI and the command it runs end within 2.5 s.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_hard_stop_ends_the_cli_and_its_command_within_2_5_s() {
    let Some(claude) = require("claude") else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let sleep = long_sleep();
    let mut s = driver(&claude).launch(spec(dir.path())).await.unwrap();
    let (mut ev, _) = ready(&mut s).await;
    s.send(msg("m1", &hang_prompt(&sleep))).await.unwrap();
    until(&mut ev, tool_call).await;
    let pids = sleeping(&sleep).await;
    let t = std::time::Instant::now();
    s.hard_stop().await.unwrap();
    assert!(
        t.elapsed() < Duration::from_millis(2500),
        "took {:?}",
        t.elapsed()
    );
    until(&mut ev, exited).await;
    assert!(
        gone_within(&pids, Duration::from_millis(500)).await,
        "the command it started is gone too"
    );
}

/// Needs no CLI.
#[tokio::test]
async fn a_missing_program_is_a_spawn_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = driver(&PathBuf::from("/nonexistent/claude"))
        .launch(spec(dir.path()))
        .await
        .err()
        .expect("must fail");
    assert!(matches!(err, drivers::DriverError::Spawn { .. }), "{err}");
}

/// Needs no CLI: the seat-tools file is written before the CLI is started.
#[tokio::test]
async fn an_existing_config_file_is_narrowed_to_owner_only_and_replaced() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dev.impl@r.json");
    std::fs::write(&file, "stale").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut sp = spec(dir.path());
    sp.seat_tools = Some(SeatToolsLaunch {
        program: "/opt/sculptured".into(),
        args: vec!["seat-tools".into()],
        env: vec![("SCULPTURED_SEAT_TOKEN".into(), "secret-token-2".into())].into(),
        approve: false,
        config_file: Some(file.clone()),
    });
    let _ = driver(&PathBuf::from("/nonexistent/claude"))
        .launch(sp)
        .await;
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        config["mcpServers"]["rig"]["env"]["SCULPTURED_SEAT_TOKEN"],
        "secret-token-2"
    );
}
