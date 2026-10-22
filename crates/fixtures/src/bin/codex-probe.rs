//! codex-probe: drives the REAL `codex app-server` through one named scenario per run and
//! records every line in and out (`{t_ms, dir: meta|in|out|stderr, line}`, as the M0
//! recordings). Scenarios with a turn spend real model calls on the user's Codex login, so this
//! is run by hand only, with the user's approval, and is never part of a routine test.
//!
//! Usage:
//! ```text
//!   codex-probe run <scenario> --out <raw dir> --cwd <throwaway dir> [--thread <id>] [--form dotted|nested]
//!   codex-probe scrub <raw dir> <dest dir> <cwd root>
//!   codex-probe app-server [args…]   (as a driver's program: records to $CODEX_PROBE_REC; see `tee`)
//! ```
//!
//! Scenarios (model turns in brackets):
//! ```text
//!   basic [1], tool [1], approval [1], user-input [1], interrupt [1-2], resume [1, needs --thread],
//!   mcp-config-dotted [0], mcp-config-nested [0], mcp-cli [0], seat-config [1, --form], seat-cli [1],
//!   reasoning-summary [1],
//!   limits [0].
//! ```
//!
//! The raw recordings hold account data (home paths, plan, credits, rate-limit numbers, host
//! name). Run `scrub` and grep the result before anything is committed; see
//! docs/spikes/m0-driver-capabilities.md, "Codex 0.160.1 refresh (M2)".
use serde_json::{Map, Value, json};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const TURN: Duration = Duration::from_secs(240);
const RPC: Duration = Duration::from_secs(60);
/// The MCP server name the seat-tools scenarios configure (the plan's `drivers::MCP_SERVER`).
const SERVER: &str = "rig";
/// Sent to the stub through the server's `env`; the stub logs only whether it arrived.
const TOKEN_ENV: (&str, &str) = ("RIG_PROBE_TOKEN", "tok");
/// The plan's `tool_timeout_sec` (MCP_TOOL_TIMEOUT_MS 86 700 000 / 1000).
const TOOL_TIMEOUT_SEC: u64 = 86_700;

type Policy = Box<dyn FnMut(&Value) -> Option<Value>>;

struct Probe {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<Value>,
    rec: Arc<Mutex<File>>,
    start: Instant,
    buf: VecDeque<Value>,
    next_id: u64,
    /// Answers server-initiated requests as they arrive: returns `{"result": …}` or `{"error": …}`.
    policy: Policy,
}

fn record(rec: &Arc<Mutex<File>>, start: Instant, dir: &str, line: Value) {
    let entry = json!({ "t_ms": start.elapsed().as_millis() as u64, "dir": dir, "line": line });
    let mut f = rec.lock().expect("rec mutex must not be poisoned");
    writeln!(f, "{entry}").unwrap();
    f.flush().unwrap();
}

fn method(v: &Value) -> &str {
    v["method"].as_str().unwrap_or("")
}
fn is_server_request(v: &Value) -> bool {
    v.get("id").is_some() && v.get("method").is_some()
}
fn is_notification(v: &Value, m: &str) -> bool {
    v.get("id").is_none() && method(v) == m
}

/// The default answers: accept approvals and elicitations, answer "green" to questions, refuse
/// anything else with -32601 (an unanswered request would hang the turn).
fn default_answer(v: &Value) -> Value {
    let m = method(v);
    if m == "item/tool/requestUserInput" {
        let mut answers = Map::new();
        let flattened_items = v["params"]["questions"].as_array().into_iter().flatten();
        for q in flattened_items {
            answers.insert(
                q["id"].as_str().unwrap_or("q").to_string(),
                json!({ "answers": ["green"] }),
            );
        }
        json!({ "result": { "answers": answers } })
    } else if m.ends_with("requestApproval") {
        json!({ "result": { "decision": "accept" } })
    } else if m == "mcpServer/elicitation/request" {
        json!({ "result": { "action": "accept", "content": {} } })
    } else {
        json!({ "error": { "code": -32601, "message": format!("codex-probe does not handle {m}") } })
    }
}

impl Probe {
    fn spawn(extra: &[String], cwd: &Path, rec_path: &Path) -> Probe {
        let rec = Arc::new(Mutex::new(
            File::create(rec_path).expect("create recording"),
        ));
        let start = Instant::now();
        let mut args = vec!["app-server".to_string()];
        args.extend(extra.iter().cloned());
        let version = Command::new("codex")
            .arg("--version")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        record(
            &rec,
            start,
            "meta",
            json!({ "program": "codex", "args": args, "version": version.unwrap_or_default() }),
        );
        let mut child = Command::new("codex")
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn codex app-server");
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let r = rec.clone();
        thread::spawn(move || {
            let matching_items = BufReader::new(stdout).lines().map_while(Result::ok);
            for line in matching_items {
                let v = serde_json::from_str::<Value>(&line).unwrap_or(Value::String(line));
                record(&r, start, "out", v.clone());
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        let r = rec.clone();
        thread::spawn(move || {
            let matching_items = BufReader::new(stderr).lines().map_while(Result::ok);
            for line in matching_items {
                record(&r, start, "stderr", Value::String(line));
            }
        });
        let stdin = child.stdin.take();
        let mut p = Probe {
            child,
            stdin,
            rx,
            rec,
            start,
            buf: VecDeque::new(),
            next_id: 1,
            policy: Box::new(|v| Some(default_answer(v))),
        };
        let init = p.call(
            "initialize",
            json!({ "clientInfo": { "name": "sculptured-probe", "version": "0.0.0" } }),
        );
        println!("initialize -> ok: {}", init.get("result").is_some());
        p.send(json!({ "jsonrpc": "2.0", "method": "initialized" }));
        p
    }

    fn send(&mut self, v: Value) {
        record(&self.rec, self.start, "in", v.clone());
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{v}").unwrap();
        stdin.flush().unwrap();
    }

    fn meta(&self, v: Value) {
        println!("meta: {v}");
        record(&self.rec, self.start, "meta", v);
    }

    /// Sends a request; returns its id.
    fn request(&mut self, m: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": m, "params": params }));
        id
    }

    fn response(&mut self, id: u64, timeout: Duration) -> Value {
        let r = self.wait(timeout, |v| {
            v["id"] == json!(id) && v.get("method").is_none()
        });
        let r = r.unwrap_or_else(|| json!({ "timeout": id }));
        if r.get("error").is_some() {
            println!("JSON-RPC error: {r}");
        }
        r
    }

    fn call(&mut self, m: &str, params: Value) -> Value {
        let id = self.request(m, params);
        self.response(id, RPC)
    }

    /// One line from the server; server requests are answered by the policy (and still returned).
    fn recv(&mut self, deadline: Instant) -> Result<Value, RecvTimeoutError> {
        let v = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))?;
        if is_server_request(&v) {
            println!("server request: {} (id {})", method(&v), v["id"]);
            if let Some(mut answer) = (self.policy)(&v) {
                let o = answer.as_object_mut().expect("policy answers an object");
                o.insert("jsonrpc".into(), json!("2.0"));
                o.insert("id".into(), v["id"].clone());
                self.send(answer);
            }
        }
        Ok(v)
    }

    /// The first line (buffered first, then new) that `pred` accepts. Others are kept for later.
    fn wait(&mut self, timeout: Duration, pred: impl Fn(&Value) -> bool) -> Option<Value> {
        if let Some(i) = self.buf.iter().position(&pred) {
            return self.buf.remove(i);
        }
        let deadline = Instant::now() + timeout;
        loop {
            match self.recv(deadline) {
                Ok(v) if pred(&v) => return Some(v),
                Ok(v) => self.buf.push_back(v),
                Err(RecvTimeoutError::Timeout) => {
                    self.meta(json!({ "timeout_ms": timeout.as_millis() as u64 }));
                    return None;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.meta(json!({ "stdout_closed": true }));
                    return None;
                }
            }
        }
    }

    /// Lets lines arrive for `d` (answering server requests), keeping them buffered.
    fn idle(&mut self, d: Duration) {
        let deadline = Instant::now() + d;
        while let Ok(v) = self.recv(deadline) {
            self.buf.push_back(v);
        }
    }

    fn thread_start(&mut self, params: Value) -> String {
        let r = self.call("thread/start", params);
        let id = r
            .pointer("/result/thread/id")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no thread id: {r}"))
            .to_string();
        println!("thread {id}");
        id
    }

    /// Starts a turn; returns the turn id.
    fn turn(&mut self, thread: &str, text: &str) -> String {
        let id = self.request(
            "turn/start",
            json!({ "threadId": thread, "input": [{ "type": "text", "text": text }] }),
        );
        let r = self.response(id, RPC);
        r.pointer("/result/turn/id")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("no turn id: {r}"))
            .to_string()
    }

    fn completed(&mut self, turn: &str, timeout: Duration) -> Option<Value> {
        let t = turn.to_string();
        let done = self.wait(timeout, move |v| {
            is_notification(v, "turn/completed") && v["params"]["turn"]["id"] == json!(t)
        });
        println!(
            "turn {turn} completed: {:?}",
            done.as_ref().map(|v| v["params"]["turn"]["status"].clone())
        );
        done
    }

    /// Closes stdin and records how the server ended (exit within `grace`, else killed).
    fn finish(mut self, grace: Duration) {
        self.meta(json!({ "stdin_closed": true }));
        self.stdin.take();
        let deadline = Instant::now() + grace;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                // Let the reader threads record the last lines.
                thread::sleep(Duration::from_millis(200));
                record(
                    &self.rec,
                    self.start,
                    "meta",
                    json!({ "exit": status.code() }),
                );
                return;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                record(
                    &self.rec,
                    self.start,
                    "meta",
                    json!({ "killed_after_ms": grace.as_millis() as u64 }),
                );
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Pids whose argv is exactly `argv` (for "is the interrupted command still running?").
fn procs_with_argv(argv: &[&str]) -> Vec<u32> {
    let mut pids = Vec::new();
    let flattened_items = std::fs::read_dir("/proc").into_iter().flatten().flatten();
    for e in flattened_items {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(raw) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = raw
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        if args.len() == argv.len() && args.iter().zip(argv).all(|(a, b)| a == b) {
            pids.push(pid);
        }
    }
    pids
}

fn stub_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let p = exe.parent().unwrap().join("mcp-ping-stub");
    assert!(
        p.exists(),
        "build it first: cargo build -p fixtures --bin mcp-ping-stub ({})",
        p.display()
    );
    p
}

fn base_thread(cwd: &Path, approval: &str) -> Value {
    json!({ "cwd": cwd, "approvalPolicy": approval, "sandbox": "danger-full-access" })
}

/// The `rig` server as thread `config`: dotted keys (the plan's primary form) or nested tables.
fn mcp_config(form: &str, stub: &Path, log: &Path) -> Value {
    let args = json!(["--log", log]);
    let env = json!({ TOKEN_ENV.0: TOKEN_ENV.1 });
    match form {
        "dotted" => {
            let k = |s: &str| format!("mcp_servers.{SERVER}.{s}");
            let mut m = Map::new();
            m.insert(k("command"), json!(stub));
            m.insert(k("args"), args);
            m.insert(k("env"), env);
            m.insert(k("tool_timeout_sec"), json!(TOOL_TIMEOUT_SEC));
            Value::Object(m)
        }
        "nested" => {
            json!({ "mcp_servers": { SERVER: { "command": stub, "args": args, "env": env, "tool_timeout_sec": TOOL_TIMEOUT_SEC } } })
        }
        other => panic!("unknown --form {other}"),
    }
}

/// The `rig` server as `-c` overrides, in the plan's fallback-2 format (a JSON string is a TOML string).
fn mcp_overrides(stub: &Path, log: &Path) -> Vec<String> {
    let q = |s: &str| serde_json::to_string(s).unwrap();
    [
        format!("command={}", q(&stub.display().to_string())),
        format!("args=[{}, {}]", q("--log"), q(&log.display().to_string())),
        format!("env={{ {} = {} }}", TOKEN_ENV.0, q(TOKEN_ENV.1)),
        format!("tool_timeout_sec={TOOL_TIMEOUT_SEC}"),
    ]
    .into_iter()
    .flat_map(|kv| ["-c".to_string(), format!("mcp_servers.{SERVER}.{kv}")])
    .collect()
}

/// After a thread starts: waits for `rig`'s startup status to settle, then asks the server what
/// it sees of `rig` (no model call).
fn mcp_status(p: &mut Probe, thread: &str) {
    let settled = p.wait(Duration::from_secs(30), |v| {
        is_notification(v, "mcpServer/startupStatus/updated")
            && v["params"]["name"] == SERVER
            && v["params"]["status"] != "starting"
    });
    p.meta(json!({ "probe": "rig startup status", "settled": settled.as_ref().map(|v| v["params"].clone()) }));
    let list = p.call(
        "mcpServerStatus/list",
        json!({ "threadId": thread, "serverName": SERVER, "detail": "toolsAndAuthOnly" }),
    );
    let tools: Vec<String> = list["result"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["name"] == SERVER)
        .flat_map(|s| {
            s["tools"]
                .as_object()
                .map(|m| m.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    p.meta(json!({ "probe": "rig tools listed", "tools": tools }));
}

const SEAT_PROMPT: &str = "Call the tool `ping` of the MCP server `rig` once with message \"probe\", then reply with exactly the word: ready";

fn run(scenario: &str, out: &Path, cwd: &Path, thread_arg: Option<&str>, form: Option<&str>) {
    let rec = |name: &str| out.join(format!("codex-m2-{name}.jsonl"));
    match scenario {
        "basic" => {
            let mut p = Probe::spawn(&[], cwd, &rec("basic"));
            let t = p.thread_start(base_thread(cwd, "never"));
            let turn = p.turn(&t, "Reply with exactly the word: ready");
            p.completed(&turn, TURN);
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "tool" => {
            let mut p = Probe::spawn(&[], cwd, &rec("tool"));
            let t = p.thread_start(base_thread(cwd, "never"));
            let turn = p.turn(
                &t,
                "Run the shell command `echo probe-ok` and reply with exactly what it printed.",
            );
            p.completed(&turn, TURN);
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "approval" => {
            // The plan's restricted posture exactly: untrusted + danger-full-access. Decline the first
            // approval request, accept every later one.
            let mut p = Probe::spawn(&[], cwd, &rec("approval"));
            let mut n = 0;
            p.policy = Box::new(move |v| {
                if !method(v).ends_with("requestApproval") {
                    return Some(default_answer(v));
                }
                n += 1;
                Some(json!({ "result": { "decision": if n == 1 { "decline" } else { "accept" } } }))
            });
            let t = p.thread_start(base_thread(cwd, "untrusted"));
            let turn = p.turn(
                &t,
                "Run the shell command `touch approval-a.txt`. After that, as a separate step, create a file named approval-b.txt containing the text hi. If a step is not allowed, skip it and say so.",
            );
            p.completed(&turn, TURN);
            p.meta(json!({ "probe": "files", "approval-a.txt": cwd.join("approval-a.txt").exists(), "approval-b.txt": cwd.join("approval-b.txt").exists() }));
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "user-input" => {
            let mut p = Probe::spawn(
                &["--enable".into(), "default_mode_request_user_input".into()],
                cwd,
                &rec("user-input"),
            );
            let t = p.thread_start(base_thread(cwd, "never"));
            let turn = p.turn(&t, "Before doing anything, ask me (the user) which color I prefer using your request-user-input tool, then reply with my answer.");
            p.completed(&turn, TURN);
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "interrupt" => {
            let mut p = Probe::spawn(&[], cwd, &rec("interrupt"));
            let t = p.thread_start(base_thread(cwd, "never"));
            std::fs::write(out.join("interrupt-thread.txt"), &t).unwrap();
            let turn = p.turn(&t, "Remember the codeword KIWI-3. Then run the shell command `sleep 97` and, when it finishes, reply with the word: done");
            let started = p.wait(TURN, |v| {
                is_notification(v, "item/started")
                    && v["params"]["item"]["type"] == "commandExecution"
            });
            p.meta(json!({ "probe": "command item started", "seen": started.is_some() }));
            thread::sleep(Duration::from_millis(1500));
            p.meta(json!({ "probe": "sleep 97 processes before interrupt", "count": procs_with_argv(&["sleep", "97"]).len() }));
            // What does a turn/start do while a turn runs? (fake-codex answers -32600; the plan guesses the real server steers.)
            let busy = p.request("turn/start", json!({ "threadId": t, "input": [{ "type": "text", "text": "Also remember a second codeword: LIME-5." }] }));
            let busy = p.response(busy, Duration::from_secs(15));
            p.meta(json!({ "probe": "turn/start during a running turn", "same_turn_id": busy.pointer("/result/turn/id") == Some(&json!(turn)) }));
            let ack = p.call("turn/interrupt", json!({ "threadId": t, "turnId": turn }));
            println!("interrupt -> {ack}");
            p.completed(&turn, Duration::from_secs(60));
            thread::sleep(Duration::from_secs(2));
            p.meta(json!({ "probe": "sleep 97 processes 2 s after the interrupted turn completed", "count": procs_with_argv(&["sleep", "97"]).len() }));
            // A queued turn/start may now run as its own turn; let it finish instead of starting another.
            let t2 = t.clone();
            let queued = p.wait(Duration::from_secs(6), move |v| {
                is_notification(v, "turn/started") && v["params"]["threadId"] == json!(t2)
            });
            match queued.and_then(|v| v["params"]["turn"]["id"].as_str().map(str::to_string)) {
                Some(q) if q != turn => {
                    p.meta(json!({ "probe": "a new turn started by itself after the interrupt", "turn": q }));
                    p.completed(&q, TURN);
                }
                _ => {
                    let alive = p.turn(&t, "Reply with exactly the word: alive");
                    p.completed(&alive, TURN);
                }
            }
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
            thread::sleep(Duration::from_secs(1));
            println!(
                "sleep 97 processes after app-server exit: {}",
                procs_with_argv(&["sleep", "97"]).len()
            );
        }
        "resume" => {
            let thread = thread_arg
                .expect("--thread <id> (the interrupt scenario writes interrupt-thread.txt)");
            let mut p = Probe::spawn(&[], cwd, &rec("resume"));
            let mut params = base_thread(cwd, "never");
            params["threadId"] = json!(thread);
            let r = p.call("thread/resume", params);
            p.meta(json!({ "probe": "resume settings", "sandbox": r["result"]["sandbox"], "approvalPolicy": r["result"]["approvalPolicy"], "turns": r["result"]["thread"]["turns"].as_array().map(Vec::len) }));
            let turn = p.turn(thread, "What codeword or codewords did I ask you to remember? Reply with only the codewords.");
            p.completed(&turn, TURN);
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "mcp-config-dotted" | "mcp-config-nested" => {
            let form = scenario.trim_start_matches("mcp-config-");
            let stub = stub_path();
            let log = out.join(format!("codex-m2-{scenario}.mcp-stub.jsonl"));
            let mut p = Probe::spawn(&[], cwd, &rec(scenario));
            let mut params = base_thread(cwd, "never");
            params["config"] = mcp_config(form, &stub, &log);
            let t = p.thread_start(params);
            mcp_status(&mut p, &t);
            p.finish(Duration::from_secs(10));
        }
        "mcp-cli" => {
            let stub = stub_path();
            let log = out.join("codex-m2-mcp-cli.mcp-stub.jsonl");
            let mut p = Probe::spawn(&mcp_overrides(&stub, &log), cwd, &rec("mcp-cli"));
            let t = p.thread_start(base_thread(cwd, "never"));
            mcp_status(&mut p, &t);
            p.finish(Duration::from_secs(10));
        }
        "seat-config" => {
            let form = form.expect("--form dotted|nested");
            let stub = stub_path();
            let name = format!("seat-config-{form}");
            let log = out.join(format!("codex-m2-{name}.mcp-stub.jsonl"));
            let mut p = Probe::spawn(&[], cwd, &rec(&name));
            let mut params = base_thread(cwd, "never");
            params["config"] = mcp_config(form, &stub, &log);
            let t = p.thread_start(params);
            mcp_status(&mut p, &t);
            let turn = p.turn(&t, SEAT_PROMPT);
            p.completed(&turn, TURN);
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "seat-cli" => {
            let stub = stub_path();
            let log = out.join("codex-m2-seat-cli.mcp-stub.jsonl");
            let mut p = Probe::spawn(&mcp_overrides(&stub, &log), cwd, &rec("seat-cli"));
            let t = p.thread_start(base_thread(cwd, "never"));
            mcp_status(&mut p, &t);
            let turn = p.turn(&t, SEAT_PROMPT);
            p.completed(&turn, TURN);
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "reasoning-summary" => {
            // Thinking capture: `model_reasoning_summary = "auto"` through the thread `config` (a
            // config key; `TurnStartParams.summary` is the schema's other route). One tiny turn.
            let mut p = Probe::spawn(&[], cwd, &rec("reasoning-summary"));
            let mut params = base_thread(cwd, "never");
            params["config"] = json!({ "model_reasoning_summary": "auto" });
            let t = p.thread_start(params);
            let turn = p.turn(
                &t,
                "Think it through briefly, then reply with only the number: what is 17 times 23?",
            );
            p.completed(&turn, TURN);
            p.idle(Duration::from_secs(2));
            p.finish(Duration::from_secs(10));
        }
        "limits" => {
            // No thread, no turn: only what the protocol exposes about limits.
            let mut p = Probe::spawn(&[], cwd, &rec("limits"));
            p.call("account/rateLimits/read", Value::Null);
            p.idle(Duration::from_secs(3));
            p.finish(Duration::from_secs(10));
        }
        other => panic!("unknown scenario {other}"),
    }
}

// ---------------------------------------------------------------- scrub

/// Keys whose values are account data: replaced by `<redacted>` wherever they appear.
const REDACT_KEYS: [&str; 20] = [
    "accountId",
    "authMode",
    "balance",
    "email",
    "hasCredits",
    "installationId",
    "individualLimit",
    "messaging_socket_path",
    "ordinaryUsageAllowed",
    "planType",
    "rateLimitResetCredits",
    "rateLimitUpsell",
    "resetsAt",
    "spendControlReached",
    "unlimited",
    "usedPercent",
    "userId",
    "user_id",
    "windowDurationMins",
    "account_id",
];

struct Scrub {
    /// (needle, replacement), applied in order; needles matched case-insensitively for names.
    paths: Vec<(String, String)>,
    names: Vec<String>,
    count: std::collections::BTreeMap<String, usize>,
}

impl Scrub {
    fn bump(&mut self, what: &str) {
        *self.count.entry(what.to_string()).or_default() += 1;
    }

    fn string(&mut self, s: &str) -> String {
        let mut s = s.to_string();
        let matching_paths = self.paths.clone();
        for (needle, with) in matching_paths {
            if s.contains(&needle) {
                s = s.replace(&needle, &with);
                self.bump(&format!("path {with}"));
            }
        }
        let matching_names = self.names.clone();
        for name in matching_names {
            let lower = s.to_lowercase();
            if lower.contains(&name) {
                let mut outp = String::new();
                let mut i = 0;
                while let Some(j) = lower[i..].find(&name) {
                    outp.push_str(&s[i..i + j]);
                    outp.push_str("user");
                    i += j + name.len();
                }
                outp.push_str(&s[i..]);
                s = outp;
                self.bump("user or host name");
            }
        }
        self.emails(&s)
    }

    /// Replaces `local@domain.tld` with `<redacted email>` (a domain needs a dot, so plugin ids
    /// such as `browser@openai-bundled` stay).
    fn emails(&mut self, s: &str) -> String {
        let b = s.as_bytes();
        let local = |c: u8| c.is_ascii_alphanumeric() || b"._%+-".contains(&c);
        let dom = |c: u8| c.is_ascii_alphanumeric() || b".-".contains(&c);
        let mut out = String::new();
        let mut last = 0;
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'@' {
                let mut l = i;
                while l > 0 && local(b[l - 1]) {
                    l -= 1;
                }
                let mut r = i + 1;
                while r < b.len() && dom(b[r]) {
                    r += 1;
                }
                let domain = &s[i + 1..r];
                if l < i && domain.contains('.') && !domain.ends_with('.') && l >= last {
                    out.push_str(&s[last..l]);
                    out.push_str("<redacted email>");
                    last = r;
                    self.bump("email");
                    i = r;
                    continue;
                }
            }
            i += 1;
        }
        out.push_str(&s[last..]);
        out
    }

    fn value(&mut self, key: Option<&str>, v: &Value) -> Value {
        if let Some(k) = key {
            if REDACT_KEYS.contains(&k) && !v.is_null() {
                self.bump(&format!("key {k}"));
                return json!("<redacted>");
            }
            if k.to_lowercase().contains("token") && v.is_string() {
                self.bump(&format!("key {k}"));
                return json!("<redacted>");
            }
            if let (true, Some(s)) = (k == "userAgent", v.as_str()) {
                self.bump("userAgent OS part");
                return json!(format!("{} <redacted>", s.split(' ').next().unwrap_or("")));
            }
            // Hook output written by the user's own hooks.
            if (k == "entries" && v.as_array().is_some_and(|a| !a.is_empty()))
                || (k == "statusMessage" && v.is_string())
            {
                self.bump("hook output");
                return json!("<redacted hook output>");
            }
        }
        match v {
            Value::String(s) => Value::String(self.string(s)),
            Value::Array(a) => Value::Array(a.iter().map(|x| self.value(None, x)).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, x)| (self.string(k), self.value(Some(k), x)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

fn scrub(raw: &Path, dest: &Path, cwd_root: &Path) {
    let home = std::env::var("HOME").expect("HOME");
    let user = std::env::var("USER").unwrap_or_default().to_lowercase();
    let host = std::fs::read_to_string("/etc/hostname")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let mut names: Vec<String> = [user, host].into_iter().filter(|n| n.len() >= 3).collect();
    names.dedup();
    let cwd_root = cwd_root.canonicalize().unwrap_or(cwd_root.to_path_buf());
    let mut s = Scrub {
        paths: vec![
            (cwd_root.display().to_string(), "/tmp/codex-probe".into()),
            (raw.display().to_string(), "/tmp/codex-probe/out".into()),
            (home, "/home/user".into()),
        ],
        names,
        count: Default::default(),
    };
    std::fs::create_dir_all(dest).unwrap();
    let mut entries: Vec<_> = std::fs::read_dir(raw)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    entries.sort();
    for path in entries {
        let text = std::fs::read_to_string(&path).unwrap();
        let mut out = String::new();
        let matching_text = text.lines();
        for line in matching_text {
            let v: Value =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            out.push_str(&s.value(None, &v).to_string());
            out.push('\n');
        }
        let target = dest.join(path.file_name().unwrap());
        std::fs::write(&target, out).unwrap();
        println!("scrubbed {}", target.display());
    }
    println!("replacements: {:#?}", s.count);
}

// ---------------------------------------------------------------- tee

/// `codex-probe app-server [args…]`: stands in for `codex app-server` under a driver (the driver's
/// `program`), passes every line through unchanged and records it in the format above to
/// `$CODEX_PROBE_REC`. It runs `$CODEX_PROBE_PROGRAM` (default `codex`) with the same arguments.
/// Used by the Task 13 seat-tools probe so the recording shows exactly what the real driver sent.
fn tee(args: &[String]) -> ! {
    let rec_path = PathBuf::from(
        std::env::var_os("CODEX_PROBE_REC").expect("CODEX_PROBE_REC: where to record"),
    );
    let program = std::env::var("CODEX_PROBE_PROGRAM").unwrap_or_else(|_| "codex".into());
    let rec = Arc::new(Mutex::new(
        File::create(&rec_path).expect("create recording"),
    ));
    let start = Instant::now();
    let version = Command::new(&program)
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    record(
        &rec,
        start,
        "meta",
        json!({ "program": program, "args": args, "version": version.unwrap_or_default(), "via": "codex-probe app-server (the driver's own lines)" }),
    );
    let mut child = Command::new(&program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("spawn {program}: {e}"));
    let mut child_in = child.stdin.take().unwrap();
    let (out, err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let r = rec.clone();
    thread::spawn(move || {
        let matching_items = std::io::stdin().lock().lines().map_while(Result::ok);
        for line in matching_items {
            record(
                &r,
                start,
                "in",
                serde_json::from_str::<Value>(&line).unwrap_or(Value::String(line.clone())),
            );
            if writeln!(child_in, "{line}")
                .and_then(|_| child_in.flush())
                .is_err()
            {
                break;
            }
        }
        record(&r, start, "meta", json!({ "stdin_closed": true }));
        // Dropping `child_in` closes the server's stdin, as the driver's Stop does.
    });
    let r = rec.clone();
    let errs = thread::spawn(move || {
        let matching_items = BufReader::new(err).lines().map_while(Result::ok);
        for line in matching_items {
            record(&r, start, "stderr", Value::String(line.clone()));
            eprintln!("{line}");
        }
    });
    let mut stdout = std::io::stdout();
    let matching_items = BufReader::new(out).lines().map_while(Result::ok);
    for line in matching_items {
        record(
            &rec,
            start,
            "out",
            serde_json::from_str::<Value>(&line).unwrap_or(Value::String(line.clone())),
        );
        if writeln!(stdout, "{line}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            break;
        }
    }
    let status = child.wait().expect("wait for the server");
    let _ = errs.join();
    record(&rec, start, "meta", json!({ "exit": status.code() }));
    std::process::exit(status.code().unwrap_or(1))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("app-server") {
        tee(&args);
    }
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };
    match args.first().map(String::as_str) {
        Some("run") => {
            let scenario = args.get(1).expect("scenario");
            let out = PathBuf::from(flag("--out").expect("--out <dir>"));
            let cwd = PathBuf::from(flag("--cwd").expect("--cwd <dir>"));
            std::fs::create_dir_all(&out).unwrap();
            std::fs::create_dir_all(&cwd).unwrap();
            run(scenario, &out, &cwd, flag("--thread"), flag("--form"));
        }
        Some("scrub") => scrub(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        ),
        _ => {
            eprintln!(
                "usage: codex-probe run <scenario> --out <dir> --cwd <dir> [--thread <id>] [--form dotted|nested] | scrub <raw> <dest> <cwd root>"
            );
            std::process::exit(2);
        }
    }
}
