use drivers::claude::ClaudeStreamDriver;
use drivers::*;
use futures::{stream::BoxStream, StreamExt};
use model::{LimitKind, MessageIntent, TurnStatus};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn driver() -> ClaudeStreamDriver {
    ClaudeStreamDriver { program: PathBuf::from(env!("CARGO_BIN_EXE_fake-claude")) }
}
fn spec(dir: &Path, env: Vec<(String, String)>) -> LaunchSpec {
    let mut s = LaunchSpec::new("dev.impl@r".parse().unwrap(), dir.to_path_buf());
    s.model = Some("fake-model".into());
    s.env = env.into();
    s.native_session_id = Some("sess-1".into());
    s
}
fn msg(id: &str, text: &str) -> OutboundMessage {
    OutboundMessage { message_id: id.into(), intent: MessageIntent::Inform, text: text.into() }
}
async fn until(ev: &mut BoxStream<'static, DriverEvent>, pred: impl Fn(&DriverEvent) -> bool) -> Vec<DriverEvent> {
    let mut seen = Vec::new();
    loop {
        let e = tokio::time::timeout(Duration::from_secs(5), ev.next()).await.expect("timely").expect("open");
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
fn consumed(events: &[DriverEvent]) -> Vec<(&str, &str)> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::MessageConsumed { message_id, turn_id } => Some((message_id.as_str(), turn_id.as_str())),
            _ => None,
        })
        .collect()
}
/// The usage these events report, summed (the in-turn reports and the turn's last one); empty when none.
fn usage(events: &[DriverEvent]) -> Vec<[u64; 4]> {
    let all: Vec<[u64; 4]> = events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::Usage { input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, .. } => {
                Some([*input_tokens, *output_tokens, *cache_read_tokens, *cache_write_tokens])
            }
            _ => None,
        })
        .collect();
    if all.is_empty() {
        return all;
    }
    vec![all.iter().fold([0; 4], |a, u| [a[0] + u[0], a[1] + u[1], a[2] + u[2], a[3] + u[3]])]
}

#[tokio::test]
async fn basic_turn_and_args() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv.txt");
    let env = vec![("FAKE_CLAUDE_ARGV_LOG".into(), argv.display().to_string())];
    let mut s = driver().launch(spec(dir.path(), env)).await.unwrap();
    let mut ev = s.take_events().unwrap();
    // Ready before any message, although the CLI emits no init until it reads one (claude-init).
    assert_eq!(until(&mut ev, |_| true).await[0], DriverEvent::SessionReady { native_session_id: "sess-1".into() });
    s.send(msg("m1", "hello")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(!got.iter().any(|e| matches!(e, DriverEvent::Unrecognized { .. })), "hook, init, thinking and rate-limit lines are ignored: {got:?}");
    assert_eq!(consumed(&got), [("m1", "turn-1")]);
    assert!(got.contains(&DriverEvent::Text { turn_id: Some("turn-1".into()), text: "fake: hello".into() }));
    // Two assistant lines repeat the message usage; only the result's modelUsage delta counts.
    assert_eq!(usage(&got), [[12, 7, 3, 2]]);
    assert_eq!(got.last().unwrap(), &DriverEvent::TurnEnded { turn_id: "turn-1".into(), status: TurnStatus::Completed });
    let args = std::fs::read_to_string(argv).unwrap();
    for a in ["--input-format", "stream-json", "--replay-user-messages", "--dangerously-skip-permissions", "--session-id", "sess-1", "--model", "fake-model"] {
        assert!(args.lines().any(|l| l == a), "missing arg {a} in {args}");
    }
}

#[tokio::test]
async fn tool_call_and_result() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "tool:ls")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(got.iter().any(|e| matches!(e, DriverEvent::ToolCall { name, call_id, .. } if name == "Bash" && call_id == "toolu_1")));
    assert!(got.iter().any(|e| matches!(e, DriverEvent::ToolResult { call_id, output, is_error: false, .. } if call_id == "toolu_1" && output == "ran ls")));
}

#[tokio::test]
async fn interrupt_then_continue() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "hang")).await.unwrap();
    let before = until(&mut ev, |e| matches!(e, DriverEvent::ToolCall { call_id, .. } if call_id == "toolu_hang")).await;
    assert_eq!(usage(&before), [[12, 7, 3, 2]], "the model call is counted as it happens, before the interrupt");
    s.interrupt().await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    // control_response and the interrupt marker yield nothing: ToolResult, then the turn's last report
    // (no tokens left to add, but the turn's cost), then TurnEnded.
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(matches!(&got[0], DriverEvent::ToolResult { turn_id: Some(t), call_id, is_error: true, .. } if t == "turn-1" && call_id == "toolu_hang"));
    assert!(matches!(&got[1], DriverEvent::Usage { input_tokens: 0, output_tokens: 0, cache_read_tokens: 0, cache_write_tokens: 0, cost: Some(_), in_turn: false, .. }), "{:?}", got[1]);
    assert_eq!(got[2], DriverEvent::TurnEnded { turn_id: "turn-1".into(), status: TurnStatus::Interrupted });
    s.send(msg("m2", "hello")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert_eq!(consumed(&got), [("m2", "turn-2")]);
    // A delta, not the session-cumulative 24/14/6/4.
    assert_eq!(usage(&got), [[12, 7, 3, 2]]);
    assert_eq!(got.last().unwrap(), &DriverEvent::TurnEnded { turn_id: "turn-2".into(), status: TurnStatus::Completed });
}

#[tokio::test]
async fn queued_message_waits_for_the_next_turn_after_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "hang")).await.unwrap();
    until(&mut ev, |e| matches!(e, DriverEvent::ToolCall { call_id, .. } if call_id == "toolu_hang")).await;
    s.send(msg("m2", "after")).await.unwrap(); // queued while turn-1 runs
    s.interrupt().await.unwrap();
    let first = until(&mut ev, turn_ended).await;
    assert!(consumed(&first).is_empty(), "the interrupt marker line consumes nothing: {first:?}");
    assert_eq!(first.last().unwrap(), &DriverEvent::TurnEnded { turn_id: "turn-1".into(), status: TurnStatus::Interrupted });
    let second = until(&mut ev, turn_ended).await;
    assert_eq!(second[0], DriverEvent::TurnStarted { turn_id: "turn-2".into() });
    assert_eq!(second[1], DriverEvent::MessageConsumed { message_id: "m2".into(), turn_id: "turn-2".into() });
    assert!(second.contains(&DriverEvent::Text { turn_id: Some("turn-2".into()), text: "fake: after".into() }));
    assert_eq!(second.last().unwrap(), &DriverEvent::TurnEnded { turn_id: "turn-2".into(), status: TurnStatus::Completed });
}

#[tokio::test]
async fn crash_reports_exit_code_and_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "crash")).await.unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    match got.last().unwrap() {
        DriverEvent::Exited { exit_code, stderr_tail, .. } => {
            assert_eq!(*exit_code, Some(3));
            assert!(stderr_tail.contains("fake crash"), "{stderr_tail}");
        }
        other => panic!("{other:?}"),
    }
    assert!(ev.next().await.is_none());
}

#[tokio::test]
async fn unknown_lines_are_kept_not_fatal() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "unknown")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(got.iter().any(|e| matches!(e, DriverEvent::Unrecognized { raw } if raw["type"] == "mystery")));
    assert!(matches!(got.last().unwrap(), DriverEvent::TurnEnded { status: TurnStatus::Completed, .. }));
}

#[tokio::test]
async fn subagent_calls_carry_parent() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "subagent")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(!got.iter().any(|e| matches!(e, DriverEvent::Unrecognized { .. })), "task_* lines are ignored: {got:?}");
    assert!(got.iter().any(|e| matches!(e, DriverEvent::ToolCall { name, call_id, parent_call_id: None, .. } if name == "Agent" && call_id == "toolu_task")));
    assert!(got.iter().any(|e| matches!(e, DriverEvent::ToolCall { name, parent_call_id: Some(p), .. } if name == "Bash" && p == "toolu_task")));
    assert!(got.iter().any(|e| matches!(e, DriverEvent::ToolResult { call_id, output, .. } if call_id == "toolu_task" && output == "one file")));
    assert_eq!(consumed(&got), [("m1", "turn-1")]);
    // Two main-thread API messages of 12/7/3/2, counted as they arrive, plus the subagent's 5/4/1/1,
    // which only modelUsage (summed over models) carries: the turn's last report adds it.
    assert_eq!(usage(&got), [[29, 18, 7, 5]]);
}

#[tokio::test]
async fn queued_message_is_not_consumed_by_the_subagent_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "subagent")).await.unwrap();
    // Queued: the fake reads it only after turn-1's result. Both ids are registered as soon as
    // `send` returns, so in practice before the subagent prompt line is parsed. Task 5's golden
    // test `queued_message_survives_marker_and_subagent_prompt` pins the rule deterministically.
    s.send(msg("m2", "hello")).await.unwrap();
    let first = until(&mut ev, turn_ended).await;
    assert_eq!(consumed(&first), [("m1", "turn-1")], "{first:?}");
    assert_eq!(first.last().unwrap(), &DriverEvent::TurnEnded { turn_id: "turn-1".into(), status: TurnStatus::Completed });
    let second = until(&mut ev, turn_ended).await;
    assert_eq!(second[0], DriverEvent::TurnStarted { turn_id: "turn-2".into() });
    assert_eq!(second[1], DriverEvent::MessageConsumed { message_id: "m2".into(), turn_id: "turn-2".into() });
    assert_eq!(second.last().unwrap(), &DriverEvent::TurnEnded { turn_id: "turn-2".into(), status: TurnStatus::Completed });
}

#[tokio::test]
async fn big_tool_result_passes_through_intact() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "big")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(got.iter().any(|e| matches!(e, DriverEvent::ToolResult { output, .. } if output.len() == 2_000_000)));
}

#[tokio::test]
async fn missing_program_is_a_spawn_error() {
    let dir = tempfile::tempdir().unwrap();
    let d = ClaudeStreamDriver { program: PathBuf::from("/nonexistent/claude") };
    let err = d.launch(spec(dir.path(), vec![])).await.err().expect("must fail");
    assert!(matches!(err, DriverError::Spawn { .. }), "{err}");
}

#[tokio::test]
async fn resume_passes_id_and_reports_failure_as_early_exit() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv.txt");
    let env = vec![
        ("FAKE_CLAUDE_ARGV_LOG".into(), argv.display().to_string()),
        ("FAKE_CLAUDE_FAIL_RESUME".into(), "1".into()),
    ];
    let mut s = driver().resume(spec(dir.path(), env), "old-sess").await.unwrap();
    let mut ev = s.take_events().unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    assert!(std::fs::read_to_string(argv).unwrap().lines().any(|l| l == "old-sess"));
    match got.last().unwrap() {
        DriverEvent::Exited { exit_code: Some(1), stderr_tail, .. } => assert!(stderr_tail.contains("No conversation found")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn stop_closes_stdin_and_exits_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.stop(Duration::from_secs(2)).await.unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    assert!(matches!(got.last().unwrap(), DriverEvent::Exited { exit_code: Some(0), .. }));
}

fn stuck_env(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![("FAKE_CLAUDE_IGNORE_EOF".to_string(), "1".to_string())];
    env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    env
}

#[tokio::test]
async fn stop_kills_a_child_that_ignores_eof_and_a_grandchild_holding_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), stuck_env(&[]))).await.unwrap();
    let mut ev = s.take_events().unwrap();
    let started = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(3), s.stop(Duration::from_millis(500))).await.expect("stop is bounded").unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert!(matches!(got.last().unwrap(), DriverEvent::Exited { signal: Some(9), .. }), "{got:?}");
    assert!(ev.next().await.is_none(), "Exited is the last event");
}

#[tokio::test]
async fn dropping_the_session_kills_the_process() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), stuck_env(&[]))).await.unwrap();
    let mut ev = s.take_events().unwrap();
    drop(s);
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    assert!(matches!(got.last().unwrap(), DriverEvent::Exited { signal: Some(9), .. }), "{got:?}");
}

#[tokio::test]
async fn stop_is_bounded_after_invalid_utf8_on_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), stuck_env(&[("FAKE_CLAUDE_BAD_UTF8", "1")]))).await.unwrap();
    let mut ev = s.take_events().unwrap();
    // Let the read error end the stdout loop while the child stays alive.
    tokio::time::sleep(Duration::from_millis(300)).await;
    tokio::time::timeout(Duration::from_secs(3), s.stop(Duration::from_millis(500))).await.expect("stop is bounded").unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Exited { .. })).await;
    assert!(matches!(got.last().unwrap(), DriverEvent::Exited { signal: Some(9), .. }), "{got:?}");
    assert!(ev.next().await.is_none());
}

#[tokio::test]
async fn a_usage_limit_ends_the_turn_failed_with_the_clis_words() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "limit")).await.unwrap();
    let got = until(&mut ev, turn_ended).await;
    assert!(
        got.iter().any(|e| matches!(e, DriverEvent::LimitReached { kind: LimitKind::Usage, resets: Some(r), text, .. }
            if r == "4:30am" && text == "You've hit your session limit · resets 4:30am (America/New_York)")),
        "{got:?}"
    );
    assert!(matches!(got.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Failed, .. })));
    // Nothing retries it: the session stays open and idle until the operator sends something.
    assert!(tokio::time::timeout(Duration::from_millis(500), ev.next()).await.is_err(), "no retry");
    s.send(msg("m2", "continue")).await.unwrap();
    let next = until(&mut ev, turn_ended).await;
    assert!(matches!(next.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Completed, .. })));
    s.stop(Duration::from_secs(5)).await.unwrap();
}

fn text_of(events: &[DriverEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            DriverEvent::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_launch_with_a_usage_baseline_counts_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut sp = spec(dir.path(), vec![]);
    // fake-claude's one API message uses [12, 7, 3, 2]; the baseline stands for `modelUsage` already
    // counted before the resume, so the result adds nothing past what the message reported. (The
    // baseline arithmetic on `modelUsage` is unit-tested in drivers/tests/claude_usage.rs.)
    sp.usage_baseline = Some([5, 5, 1, 1]);
    let mut s = driver().launch(sp).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "hello")).await.unwrap();
    assert_eq!(usage(&until(&mut ev, turn_ended).await), [[12, 7, 3, 2]]);
    s.stop(Duration::from_secs(5)).await.unwrap();
}

#[tokio::test]
async fn seat_tools_raise_the_clis_mcp_tool_timeout() {
    let dir = tempfile::tempdir().unwrap();
    for (tools, want) in [(true, MCP_TOOL_TIMEOUT_MS), (false, "unset")] {
        let mut sp = spec(dir.path(), vec![]);
        if tools {
            sp.seat_tools = Some(SeatToolsLaunch { program: "/opt/sculptured".into(), args: vec!["seat-tools".into()], env: Default::default(), approve: false, config_file: None });
        }
        let mut s = driver().launch(sp).await.unwrap();
        let mut ev = s.take_events().unwrap();
        s.send(msg("m1", "env:MCP_TOOL_TIMEOUT")).await.unwrap();
        assert_eq!(text_of(&until(&mut ev, turn_ended).await), [want], "seat tools: {tools}");
        s.stop(Duration::from_secs(5)).await.unwrap();
    }
}

#[tokio::test]
async fn the_seat_tools_config_is_a_private_file_and_the_token_stays_off_argv() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv.txt");
    let file = dir.path().join("dev.impl@r.json");
    let mut sp = spec(dir.path(), vec![("FAKE_CLAUDE_ARGV_LOG".into(), argv.display().to_string())]);
    sp.seat_tools = Some(SeatToolsLaunch {
        program: "/opt/sculptured".into(),
        args: vec!["seat-tools".into()],
        env: vec![("SCULPTURED_SEAT_TOKEN".into(), "secret-token-1".into())].into(),
        approve: false,
        config_file: Some(file.clone()),
    });
    let mut s = driver().launch(sp).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "hello")).await.unwrap();
    until(&mut ev, turn_ended).await;
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(config["mcpServers"]["rig"]["env"]["SCULPTURED_SEAT_TOKEN"], "secret-token-1");
    let args = std::fs::read_to_string(&argv).unwrap();
    assert!(args.lines().any(|l| l == file.display().to_string()), "the path is passed: {args}");
    assert!(!args.contains("secret-token-1"), "the token is not on the command line: {args}");
    s.stop(Duration::from_secs(5)).await.unwrap();
}

#[tokio::test]
async fn an_existing_config_file_is_narrowed_to_owner_only_and_replaced() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dev.impl@r.json");
    std::fs::write(&file, "stale").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut sp = spec(dir.path(), vec![]);
    sp.seat_tools = Some(SeatToolsLaunch {
        program: "/opt/sculptured".into(),
        args: vec!["seat-tools".into()],
        env: vec![("SCULPTURED_SEAT_TOKEN".into(), "secret-token-2".into())].into(),
        approve: false,
        config_file: Some(file.clone()),
    });
    let mut s = driver().launch(sp).await.unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(std::fs::read_to_string(&file).unwrap().contains("secret-token-2"));
    s.stop(Duration::from_secs(5)).await.unwrap();
}

/// Alive and not a zombie (Linux `/proc`).
#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| s.rsplit(')').next().is_some_and(|rest| !rest.trim_start().starts_with('Z')))
}

#[cfg(target_os = "linux")]
async fn gone_within(pid: u32, d: Duration) -> bool {
    let t = std::time::Instant::now();
    while t.elapsed() < d {
        if !alive(pid) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_hard_stop_ends_a_stubborn_agent_and_its_commands_within_2_5_s() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "stubborn")).await.unwrap();
    let got = until(&mut ev, |e| matches!(e, DriverEvent::Text { text, .. } if text.starts_with("child "))).await;
    let DriverEvent::Text { text, .. } = got.last().unwrap() else { unreachable!() };
    let sleep_pid: u32 = text.trim_start_matches("child ").parse().unwrap();
    assert!(alive(sleep_pid));
    let t = std::time::Instant::now();
    s.hard_stop().await.unwrap();
    assert!(t.elapsed() < Duration::from_millis(2500), "took {:?}", t.elapsed());
    assert!(t.elapsed() >= Duration::from_millis(1900), "it ignored the interrupt, so the force came after 2 s: {:?}", t.elapsed());
    assert!(gone_within(sleep_pid, Duration::from_millis(500)).await, "the command it started is gone too");
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_well_behaved_agent_exits_on_the_interrupt_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = driver().launch(spec(dir.path(), vec![])).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "hang")).await.unwrap();
    until(&mut ev, |e| matches!(e, DriverEvent::ToolCall { call_id, .. } if call_id == "toolu_hang")).await;
    let t = std::time::Instant::now();
    s.hard_stop().await.unwrap();
    assert!(t.elapsed() < Duration::from_millis(1000), "Ctrl+C alone ended it: {:?}", t.elapsed());
}

#[tokio::test]
async fn a_claude_setup_reaches_the_cli_as_its_flags() {
    let dir = tempfile::tempdir().unwrap();
    let argv = dir.path().join("argv.txt");
    let mut sp = spec(dir.path(), vec![("FAKE_CLAUDE_ARGV_LOG".into(), argv.display().to_string())]);
    sp.claude_setup = Some(model::ClaudeSetup::default());
    let mut s = driver().launch(sp).await.unwrap();
    let mut ev = s.take_events().unwrap();
    s.send(msg("m1", "hello")).await.unwrap();
    until(&mut ev, turn_ended).await;
    let args = std::fs::read_to_string(&argv).unwrap();
    let args: Vec<&str> = args.lines().collect();
    let at = args.iter().position(|a| *a == "--setting-sources").expect("setting sources passed");
    assert_eq!(args[at + 1], "project,local");
    assert!(args.contains(&"--strict-mcp-config"), "{args:?}");
    s.stop(Duration::from_secs(5)).await.unwrap();
}
