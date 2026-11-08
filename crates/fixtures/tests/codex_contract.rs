//! The one approved real-CLI probe of Codex with Sculptured's seat tools, which records the
//! `codex-m2-seat-shim*` recordings. The driver's live contract tests are in
//! `crates/drivers/tests/codex_live.rs`.
use drivers::codex::CodexAppServerDriver;

use drivers::{Driver, DriverEvent, LaunchSpec, OutboundMessage, SeatToolsLaunch};
use futures::StreamExt;
use futures::stream::BoxStream;
use model::MessageIntent;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

type Events = BoxStream<'static, DriverEvent>;

fn msg(id: &str, text: &str) -> OutboundMessage {
    OutboundMessage {
        message_id: id.into(),
        intent: MessageIntent::Inform,
        text: text.into(),
    }
}
fn turn_ended(e: &DriverEvent) -> bool {
    matches!(e, DriverEvent::TurnEnded { .. })
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
