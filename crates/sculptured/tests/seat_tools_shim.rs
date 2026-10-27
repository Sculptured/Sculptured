//! `sculptured seat-tools`, the MCP server a CLI starts for its agent: the real binary over stdio,
//! relaying a tool call through a fake hostd socket. No model and no CLI are involved.
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// A fake hostd socket: records each request line and answers it with a fixed report reply.
fn fake_socket(path: &std::path::Path) -> mpsc::Receiver<Value> {
    let listener = UnixListener::bind(path).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let matching_listener = listener.incoming();
        for conn in matching_listener {
            let Ok(mut conn) = conn else { return };
            let mut line = String::new();
            BufReader::new(conn.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let _ = tx.send(serde_json::from_str::<Value>(line.trim()).unwrap());
            let reply =
                json!({ "ok": true, "result": { "text": "Recorded your report." }, "error": null });
            let _ = writeln!(conn, "{reply}");
        }
    });
    rx
}

#[test]
fn the_shim_speaks_mcp_on_stdio_and_relays_calls_to_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("seat-tools.sock");
    let requests = fake_socket(&socket);
    let mut child = Command::new(env!("CARGO_BIN_EXE_sculptured"))
        .arg("seat-tools")
        .env("SCULPTURED_SEAT_SOCKET", &socket)
        .env("SCULPTURED_SEAT_TOKEN", "tok-1")
        .env_remove("SCULPTURED_SEAT_APPROVE")
        .env_remove("RIG_CONTROL_SEAT_APPROVE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for line in [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "report", "arguments": { "summary": "ok" } } }),
    ] {
        writeln!(stdin, "{line}").unwrap();
    }
    stdin.flush().unwrap();

    // Each request runs on its own thread, so replies may come in any order: collect them by id.
    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        let matching_items = BufReader::new(stdout).lines();
        for line in matching_items {
            let Ok(line) = line else { return };
            let _ = tx.send(
                serde_json::from_str::<Value>(&line).expect("stdout carries only the protocol"),
            );
        }
    });
    let mut replies: HashMap<i64, Value> = HashMap::new();
    while replies.len() < 3 {
        let r = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("a reply in time");
        replies.insert(r["id"].as_i64().unwrap(), r);
    }
    assert_eq!(replies[&1]["result"]["serverInfo"]["name"], "rig");
    let names: Vec<&str> = replies[&2]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "request_input",
            "record_decision",
            "report",
            "dispatch_work",
            "complete_work",
            "workflow_status",
            "read_decisions",
            "search_history",
            "record_lesson",
            "spawn_agent",
            "send_agent_message",
            "team_status",
            "propose_change",
            "write_status",
            "read_pdf"
        ],
        "no approve without SCULPTURED_SEAT_APPROVE"
    );
    assert_eq!(
        replies[&3]["result"]["content"][0]["text"],
        "Recorded your report."
    );
    assert_eq!(replies[&3]["result"]["isError"], false);
    let sent = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        sent,
        json!({ "token": "tok-1", "tool": "report", "args": { "summary": "ok" } })
    );

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "the shim ends when the agent closes stdin: {status}"
    );
}

#[test]
fn the_shim_says_plainly_when_it_was_started_without_its_socket() {
    let out = Command::new(env!("CARGO_BIN_EXE_sculptured"))
        .arg("seat-tools")
        .env_remove("SCULPTURED_SEAT_SOCKET")
        .env_remove("RIG_CONTROL_SEAT_SOCKET")
        .env_remove("SCULPTURED_SEAT_TOKEN")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty(), "nothing on the protocol stream");
    assert!(String::from_utf8_lossy(&out.stderr).contains("SCULPTURED_SEAT_SOCKET is not set"));
}
