//! A tiny stdio MCP server with one tool, `ping`, for the Codex seat-tools probe
//! (`codex-probe`, M2 Codex refresh). Never used by a routine test.
//!
//! Newline-delimited JSON-RPC 2.0 on stdin/stdout. It answers `initialize`, `ping`,
//! `tools/list` and `tools/call` (`ping` returns `pong: <message>`); other requests get
//! error -32601; notifications get nothing.
//!
//! The file named by `--log <path>` (else by `MCP_STUB_LOG`), when given, receives one JSON line per event in the recording format
//! (`{t_ms, dir: meta|in|out, line}`): a first `meta` line with the argv and whether the
//! `RIG_PROBE_TOKEN` env var arrived (its value is never logged), then every line in and out.
use serde_json::{Value, json};
use std::fs::File;
use std::io::{BufRead, Write};
use std::time::Instant;

struct Log {
    file: Option<File>,
    start: Instant,
}

impl Log {
    fn write(&mut self, dir: &str, line: Value) {
        if let Some(f) = self.file.as_mut() {
            let entry = json!({ "t_ms": self.start.elapsed().as_millis() as u64, "dir": dir, "line": line });
            let _ = writeln!(f, "{entry}");
            let _ = f.flush();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let log_path = args
        .iter()
        .position(|a| a == "--log")
        .and_then(|i| args.get(i + 1))
        .map(std::ffi::OsString::from);
    let file = log_path
        .or_else(|| std::env::var_os("MCP_STUB_LOG"))
        .and_then(|p| File::create(p).ok());
    let mut log = Log {
        file,
        start: Instant::now(),
    };
    log.write(
        "meta",
        json!({
            "program": "mcp-ping-stub",
            "args": args,
            "env_RIG_PROBE_TOKEN_present": std::env::var_os("RIG_PROBE_TOKEN").is_some(),
        }),
    );
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let matching_stdin = stdin.lock().lines().map_while(Result::ok);
    for line in matching_stdin {
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                log.write("in", Value::String(line));
                continue;
            }
        };
        log.write("in", req.clone());
        let Some(id) = req.get("id").cloned() else {
            continue;
        };
        if req.get("method").is_none() {
            continue; // a response to something we never send
        }
        let method = req["method"].as_str().unwrap_or("");
        let reply = match method {
            "initialize" => json!({ "jsonrpc": "2.0", "id": id, "result": {
                "protocolVersion": req["params"]["protocolVersion"].clone(),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mcp-ping-stub", "version": "0.0.0" }
            }}),
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            "tools/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": [{
                "name": "ping",
                "description": "Answers pong with the given message. A harmless probe tool.",
                "inputSchema": { "type": "object", "properties": {
                    "message": { "type": "string", "description": "Any short text." }
                }, "required": ["message"] }
            }]}}),
            "tools/call" if req["params"]["name"] == "ping" => {
                let msg = req["params"]["arguments"]["message"].as_str().unwrap_or("");
                json!({ "jsonrpc": "2.0", "id": id, "result": {
                    "content": [{ "type": "text", "text": format!("pong: {msg}") }],
                    "isError": false
                }})
            }
            "tools/call" => json!({ "jsonrpc": "2.0", "id": id, "result": {
                "content": [{ "type": "text", "text": "unknown tool" }], "isError": true
            }}),
            _ => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": format!("method not found: {method}") } })
            }
        };
        log.write("out", reply.clone());
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
    log.write("meta", json!({ "stdin_closed": true }));
}
