//! Fake `codex app-server` for tests. Behaviour is chosen by the message text (the first
//! line that is a script; see Task 11 of the M2 plan). Every line mirrors a shape in
//! `crates/fixtures/recordings/codex-*.jsonl` (Codex 0.160.0) and `codex-m2-*.jsonl`
//! (Codex 0.160.1, same schema), trimmed to the keys Sculptured reads plus the
//! identifying ones.
//!
//! One deliberate divergence: a `turn/start` while a turn is running gets JSON-RPC error
//! `-32600 a turn is already in progress`. The real server steers instead: it answers with
//! the active turn's id and no new turn (codex-m2-interrupt@6654-6656). The driver never
//! sends one mid-turn, so the error makes a driver bug visible in tests. `turn/steer` is not
//! implemented either (-32601); the driver does not use it.
//!
//! Thread status: `thread/status/changed` goes `active` (`activeFlags: []`) right after the
//! `turn/start` response and `idle` just before every `turn/completed` (codex-m2-interrupt@18,67);
//! an approval sets `waitingOnApproval` and clears it after `serverRequest/resolved`.
use serde_json::{Value, json};
use std::io::{BufRead, Lines, StdinLock, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

type Stdin = Lines<StdinLock<'static>>;

/// One model call's usage: input (of which cached), output.
const CALL: [u64; 3] = [100, 40, 10];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

enum Script {
    Default,
    Tool(String),
    Hang,
    /// As `hang`, but the command is a real `sleep 600` child process that an interrupt leaves
    /// running (codex-m2-interrupt: the interrupted command stayed alive). Its pid is logged as
    /// `child: <pid>`, so a test can check that the driver's Stop ends it.
    HangChild,
    /// As `hang`, but the interrupt makes fake-codex crash (exit 3) instead of ending the turn: a
    /// way for a test to crash the app-server at a moment it chooses, while a turn runs.
    CrashOnInterrupt,
    Fail,
    Crash,
    CrashOnce,
    Ask(String, Vec<String>),
    Edit(String),
    /// `run:<cmd>` (false) or `run-quiet:<cmd>` (true: an accepted run has no output).
    Run(String, bool),
    Mystery,
    Unknown,
}

fn script(text: &str) -> Script {
    let mapped_text = text.lines().map(str::trim);
    for line in mapped_text {
        let s = match line {
            "hang" => Script::Hang,
            "hang-child" => Script::HangChild,
            "crash-on-interrupt" => Script::CrashOnInterrupt,
            "fail" => Script::Fail,
            "crash" => Script::Crash,
            "crash-once" => Script::CrashOnce,
            "mystery" => Script::Mystery,
            "unknown" => Script::Unknown,
            l if l.starts_with("tool:") => Script::Tool(l["tool:".len()..].trim().to_string()),
            l if l.starts_with("edit:") => Script::Edit(l["edit:".len()..].trim().to_string()),
            l if l.starts_with("run:") => Script::Run(l["run:".len()..].trim().to_string(), false),
            l if l.starts_with("run-quiet:") => {
                Script::Run(l["run-quiet:".len()..].trim().to_string(), true)
            }
            l if l.starts_with("ask:") => {
                let mut parts = l["ask:".len()..].split('|').map(str::trim);
                let q = parts.next().unwrap_or_default().to_string();
                Script::Ask(
                    q,
                    parts
                        .filter(|p| !p.is_empty())
                        .map(str::to_string)
                        .collect(),
                )
            }
            _ => continue,
        };
        return s;
    }
    Script::Default
}

struct Fake {
    native_questions: bool,
    state: PathBuf,
    log: Option<PathBuf>,
    thread: Option<String>,
    approval: String,
    total: [u64; 3],
    last_turn: Option<String>,
    turns: u64,
    items: u64,
    server_ids: u64,
}

impl Fake {
    fn log(&self, line: &str) {
        if let Some(path) = &self.log
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
        {
            let _ = writeln!(f, "{line}");
        }
    }

    fn out(&self, v: Value) {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{v}").unwrap();
        out.flush().unwrap();
    }

    fn notify(&self, method: &str, params: Value) {
        self.out(json!({ "emittedAtMs": now_ms(), "method": method, "params": params }));
    }

    fn result(&self, id: &Value, result: Value) {
        self.out(json!({ "id": id, "result": result }));
    }

    fn error(&self, id: &Value, code: i64, message: &str) {
        self.out(json!({ "id": id, "error": { "code": code, "message": message } }));
    }

    fn thread_id(&self) -> String {
        self.thread.clone().unwrap_or_default()
    }

    fn new_thread_id() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("thr-{:08x}{:08x}", std::process::id(), nanos)
    }

    fn state_file(&self, thread: &str) -> PathBuf {
        self.state.join(format!("{thread}.json"))
    }

    fn save(&self) {
        if let Some(t) = &self.thread {
            let _ = std::fs::create_dir_all(&self.state);
            let body = json!({ "total": self.total, "last_turn": self.last_turn });
            let _ = std::fs::write(self.state_file(t), body.to_string());
        }
    }

    /// The stored total and last turn of `thread`; None when it was never started here.
    fn load(&self, thread: &str) -> Option<([u64; 3], Option<String>)> {
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(self.state_file(thread)).ok()?).ok()?;
        let t = &v["total"];
        let n = |i: usize| t[i].as_u64().unwrap_or(0);
        Some((
            [n(0), n(1), n(2)],
            v["last_turn"].as_str().map(str::to_string),
        ))
    }

    fn thread_json(&self, id: &str, forked_from: Option<&str>) -> Value {
        json!({ "id": id, "forkedFromId": forked_from, "status": { "type": "idle" }, "turns": [], "cliVersion": "0.160.1" })
    }

    fn sandbox_json(params: &Value) -> Value {
        let kind = match params["sandbox"].as_str() {
            Some("danger-full-access") => "dangerFullAccess",
            Some("read-only") => "readOnly",
            _ => "workspaceWrite",
        };
        json!({ "type": kind })
    }

    fn usage_json(n: [u64; 3]) -> Value {
        json!({ "inputTokens": n[0], "cachedInputTokens": n[1], "cacheWriteInputTokens": 0, "outputTokens": n[2],
                "reasoningOutputTokens": 0, "totalTokens": n[0] + n[2] })
    }

    /// The first `tokenUsage` after resume or fork replays the old total with the old turn id.
    fn replay_usage(&self, turn: Option<String>) {
        if self.total == [0, 0, 0] {
            return;
        }
        self.notify(
            "thread/tokenUsage/updated",
            json!({ "threadId": self.thread_id(), "turnId": turn.unwrap_or_else(|| "turn-replayed".into()),
                    "tokenUsage": { "last": Self::usage_json(self.total), "total": Self::usage_json(self.total), "modelContextWindow": 258400 } }),
        );
    }

    fn charge(&mut self, turn: &str) {
        let paired_total = self.total.iter_mut().zip(CALL);
        for (total, call) in paired_total {
            *total += call;
        }
        self.notify(
            "thread/tokenUsage/updated",
            json!({ "threadId": self.thread_id(), "turnId": turn,
                    "tokenUsage": { "last": Self::usage_json(CALL), "total": Self::usage_json(self.total), "modelContextWindow": 258400 } }),
        );
    }

    fn item_id(&mut self, prefix: &str) -> String {
        self.items += 1;
        format!("{prefix}-{}", self.items)
    }

    /// `item/started` carries `startedAtMs`, `item/completed` carries `completedAtMs` (codex-m2-basic@2423-2426).
    fn item(&self, method: &str, turn: &str, item: Value) {
        let at = if method == "item/completed" {
            "completedAtMs"
        } else {
            "startedAtMs"
        };
        self.notify(
            method,
            json!({ "item": item, "threadId": self.thread_id(), "turnId": turn, at: now_ms() }),
        );
    }

    fn agent_message(&mut self, turn: &str, text: &str) {
        let id = self.item_id("msg");
        let started = json!({ "type": "agentMessage", "id": id, "text": "", "phase": "final_answer", "delivery": null, "memoryCitation": null, "questions": null });
        self.item("item/started", turn, started);
        self.notify(
            "item/agentMessage/delta",
            json!({ "delta": text, "itemId": id, "threadId": self.thread_id(), "turnId": turn }),
        );
        let done = json!({ "type": "agentMessage", "id": id, "text": text, "phase": "final_answer", "delivery": null, "memoryCitation": null, "questions": null });
        self.item("item/completed", turn, done);
    }

    fn command_item(
        id: &str,
        cmd: &str,
        status: &str,
        output: Option<&str>,
        exit: Option<i64>,
    ) -> Value {
        json!({ "type": "commandExecution", "id": id, "command": format!("/bin/bash -lc '{cmd}'"),
                "commandActions": [{ "command": cmd, "type": "unknown" }], "cwd": ".", "aggregatedOutput": output,
                "exitCode": exit, "status": status, "durationMs": null, "processId": "1", "source": "unifiedExecStartup" })
    }

    /// A command that has not started a process: proposed and waiting for approval, or declined.
    /// `source` is `agent`, `processId` and `aggregatedOutput` are null (codex-m2-approval@4646, @4648).
    fn proposed_command_item(id: &str, cmd: &str, status: &str) -> Value {
        let mut item = Self::command_item(id, cmd, status, None, None);
        item["processId"] = Value::Null;
        item["source"] = json!("agent");
        item
    }

    /// `thread/status/changed` to `active` with `flags` (codex-m2-interrupt@18: `[]` at turn start;
    /// codex-m2-approval@4646: `["waitingOnApproval"]` while an approval is open).
    fn active(&self, flags: Value) {
        self.notify("thread/status/changed", json!({ "status": { "activeFlags": flags, "type": "active" }, "threadId": self.thread_id() }));
    }

    /// `thread/status/changed` to `idle`, sent just before every `turn/completed` (codex-m2-interrupt@67).
    fn idle(&self) {
        self.notify(
            "thread/status/changed",
            json!({ "status": { "type": "idle" }, "threadId": self.thread_id() }),
        );
    }

    /// Sends approval request `method` and waits. On an answer: `serverRequest/resolved`, then the
    /// `waitingOnApproval` flag (set by the caller, see `active`) cleared to `[]`
    /// (codex-approval@4805-4806, codex-m2-approval@4647-4648). None when the turn was interrupted.
    ///
    /// Unobserved: the real server's behaviour on an interrupt during a pending approval. fake-codex
    /// leaves `waitingOnApproval` set and sends no `serverRequest/resolved`; the turn's closing
    /// `idle` status line follows directly (pinned by `an_interrupt_during_a_pending_approval_resolves_nothing`).
    fn ask_approval(&mut self, method: &str, params: Value, stdin: &mut Stdin) -> Option<Value> {
        let rid = self.server_request(method, params);
        let resp = self.wait_response(stdin, Some(rid))?;
        self.notify(
            "serverRequest/resolved",
            json!({ "requestId": rid, "threadId": self.thread_id() }),
        );
        self.active(json!([]));
        Some(resp)
    }

    fn file_item(id: &str, path: &str, status: &str) -> Value {
        json!({ "type": "fileChange", "id": id, "status": status, "changes": [{ "path": path, "kind": { "type": "add" }, "diff": "hi\n" }] })
    }

    fn complete(&mut self, turn: &str, status: &str, error: Value) {
        self.last_turn = Some(turn.to_string());
        self.save();
        self.idle();
        self.notify(
            "turn/completed",
            json!({ "threadId": self.thread_id(),
                    "turn": { "id": turn, "status": status, "error": error, "items": [], "itemsView": "summary", "durationMs": 1, "completedAt": now_ms() / 1000 } }),
        );
    }

    fn server_request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.server_ids;
        self.server_ids += 1;
        self.out(json!({ "id": id, "method": method, "params": params }));
        id
    }

    /// Reads input until the response to server request `id` (Some), or until `turn` is
    /// interrupted (None, after answering the interrupt). Other requests are refused.
    fn wait_response(&mut self, stdin: &mut Stdin, id: Option<u64>) -> Option<Value> {
        loop {
            let Some(Ok(line)) = stdin.next() else {
                std::process::exit(0)
            };
            self.log(&format!("in: {line}"));
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match (v["method"].as_str(), v.get("id")) {
                (None, Some(rid)) if id.is_some_and(|want| rid.as_u64() == Some(want)) => {
                    return Some(v);
                }
                (Some("turn/interrupt"), Some(rid)) => {
                    self.result(rid, json!({}));
                    return None;
                }
                // The real server steers here (module doc); the error is a deliberate bug detector.
                (Some("turn/start"), Some(rid)) => {
                    self.error(rid, -32600, "a turn is already in progress")
                }
                (Some(m), Some(rid)) => {
                    self.error(rid, -32601, &format!("fake-codex cannot {m} during a turn"))
                }
                _ => {}
            }
        }
    }

    fn interrupted(&mut self, turn: &str) {
        self.charge(turn);
        self.complete(turn, "interrupted", Value::Null);
    }

    fn turn(&mut self, id: &Value, text: &str, stdin: &mut Stdin) {
        self.turns += 1;
        let suffix: String = self.thread_id().chars().rev().take(6).collect();
        let turn = format!("turn-{}-{suffix}", self.turns);
        let turn_json = json!({ "id": turn, "status": "inProgress", "items": [], "itemsView": "notLoaded", "error": null, "startedAt": null, "completedAt": null, "durationMs": null });
        self.result(id, json!({ "turn": turn_json.clone() }));
        self.active(json!([]));
        self.notify(
            "turn/started",
            json!({ "threadId": self.thread_id(), "turn": turn_json }),
        );
        let user = self.item_id("user");
        let user_item = json!({ "type": "userMessage", "id": user, "clientId": null, "content": [{ "type": "text", "text": text, "text_elements": [] }] });
        self.item("item/started", &turn, user_item.clone());
        self.item("item/completed", &turn, user_item);
        let payload = fixtures::work_message(text);
        let text = payload.as_str();
        let reply = match script(text) {
            Script::Crash => crash(),
            Script::CrashOnce => {
                let marker = self.state.join("crashed-once");
                if marker.exists() {
                    format!("codex received: {text}")
                } else {
                    let _ = std::fs::create_dir_all(&self.state);
                    let _ = std::fs::write(&marker, "1");
                    crash()
                }
            }
            Script::Fail => {
                self.charge(&turn);
                self.complete(&turn, "failed", json!({ "message": "fake-codex: the model request failed", "additionalDetails": null }));
                return;
            }
            Script::HangChild => {
                // Same process group as fake-codex (the default), stdio detached so it never holds
                // fake-codex's stdout open. Never waited for: it outlives fake-codex unless killed.
                #[expect(
                    clippy::zombie_processes,
                    reason = "the command must outlive fake-codex unless the driver kills its group"
                )]
                let child = std::process::Command::new("sleep")
                    .arg("600")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .expect("spawn sleep");
                self.log(&format!("child: {}", child.id()));
                let item = self.item_id("exec");
                self.item(
                    "item/started",
                    &turn,
                    Self::command_item(&item, "sleep 600", "inProgress", None, None),
                );
                self.wait_response(stdin, None);
                return self.interrupted(&turn);
            }
            Script::CrashOnInterrupt => {
                let item = self.item_id("exec");
                self.item(
                    "item/started",
                    &turn,
                    Self::command_item(&item, "sleep 600", "inProgress", None, None),
                );
                self.wait_response(stdin, None);
                crash()
            }
            Script::Hang => {
                let item = self.item_id("exec");
                self.item(
                    "item/started",
                    &turn,
                    Self::command_item(&item, "sleep 600", "inProgress", None, None),
                );
                self.wait_response(stdin, None);
                return self.interrupted(&turn);
            }
            Script::Tool(cmd) => {
                let item = self.item_id("exec");
                self.item(
                    "item/started",
                    &turn,
                    Self::command_item(&item, &cmd, "inProgress", None, None),
                );
                let output = format!("ran {cmd}\n");
                self.item(
                    "item/completed",
                    &turn,
                    Self::command_item(&item, &cmd, "completed", Some(&output), Some(0)),
                );
                format!("codex ran {cmd}")
            }
            Script::Ask(question, options) if self.native_questions => {
                let opts: Vec<Value> = options
                    .iter()
                    .map(|o| json!({ "label": o, "description": null }))
                    .collect();
                let params = json!({ "questions": [{ "id": "q1", "header": "Question", "question": question, "options": opts, "isOther": true, "isSecret": false }],
                                     "itemId": self.item_id("call"), "threadId": self.thread_id(), "turnId": turn, "isBlocking": false, "autoResolutionMs": null });
                let rid = self.server_request("item/tool/requestUserInput", params);
                let Some(resp) = self.wait_response(stdin, Some(rid)) else {
                    return self.interrupted(&turn);
                };
                self.notify(
                    "serverRequest/resolved",
                    json!({ "requestId": rid, "threadId": self.thread_id() }),
                );
                let answer = resp["result"]["answers"]["q1"]["answers"][0]
                    .as_str()
                    .unwrap_or("(no answer)")
                    .to_string();
                format!("codex got answer: {answer}")
            }
            Script::Ask(question, _) => {
                // Without the flag Codex asks in plain text and then waits in `sleep` calls; its turn never
                // ends by itself (codex-user-input-noflag: 15 sleeps of 60 s, 480 s, never completed).
                self.agent_message(&turn, &question);
                let first = self.item_id("sleep");
                self.item(
                    "item/started",
                    &turn,
                    json!({ "type": "sleep", "id": first, "durationMs": 60000 }),
                );
                self.item(
                    "item/completed",
                    &turn,
                    json!({ "type": "sleep", "id": first, "durationMs": 60000 }),
                );
                self.charge(&turn);
                let second = self.item_id("sleep");
                self.item(
                    "item/started",
                    &turn,
                    json!({ "type": "sleep", "id": second, "durationMs": 60000 }),
                );
                self.wait_response(stdin, None);
                return self.interrupted(&turn);
            }
            Script::Edit(path) => {
                let item = self.item_id("exec");
                self.item(
                    "item/started",
                    &turn,
                    Self::file_item(&item, &path, "inProgress"),
                );
                let allowed = if self.approval == "never" {
                    true
                } else {
                    // M0 order: the fileChange item starts, then the flag, then the request (codex-approval@55-59).
                    self.active(json!(["waitingOnApproval"]));
                    let params = json!({ "itemId": item, "threadId": self.thread_id(), "turnId": turn, "grantRoot": null, "reason": null, "startedAtMs": now_ms() });
                    let Some(resp) =
                        self.ask_approval("item/fileChange/requestApproval", params, stdin)
                    else {
                        return self.interrupted(&turn);
                    };
                    allows(&resp)
                };
                let status = if allowed { "completed" } else { "declined" };
                self.item(
                    "item/completed",
                    &turn,
                    Self::file_item(&item, &path, status),
                );
                if allowed {
                    format!("codex changed {path}")
                } else {
                    format!("codex was not allowed to change {path}")
                }
            }
            Script::Run(cmd, quiet) => {
                // The command approval path (codex-m2-approval@4646-4648 decline, @8562-8596 accept). The
                // request has no `reason`, and `availableDecisions` omits `decline`, which is still honoured.
                let item = self.item_id("exec");
                let ask = self.approval != "never";
                if ask {
                    // M2 order: the flag, then the command item starts, then the request (codex-m2-approval@62-64).
                    self.active(json!(["waitingOnApproval"]));
                }
                self.item(
                    "item/started",
                    &turn,
                    Self::proposed_command_item(&item, &cmd, "inProgress"),
                );
                let allowed = if ask {
                    let argv = execpolicy_argv(&cmd);
                    let params = json!({ "availableDecisions": ["accept", { "acceptWithExecpolicyAmendment": { "execpolicy_amendment": argv } }, "cancel"],
                                         "command": format!("/bin/bash -lc '{cmd}'"), "commandActions": [{ "command": cmd, "type": "unknown" }],
                                         "cwd": ".", "environmentId": "local", "itemId": item, "kind": "command",
                                         "proposedExecpolicyAmendment": argv, "startedAtMs": now_ms(), "threadId": self.thread_id(), "turnId": turn });
                    let Some(resp) =
                        self.ask_approval("item/commandExecution/requestApproval", params, stdin)
                    else {
                        return self.interrupted(&turn);
                    };
                    allows(&resp)
                } else {
                    true
                };
                if allowed {
                    let done = if quiet {
                        // An accepted command with no output: `aggregatedOutput` null, `durationMs` 0 (codex-m2-approval@8596).
                        let mut done = Self::command_item(&item, &cmd, "completed", None, Some(0));
                        done["durationMs"] = json!(0);
                        done
                    } else {
                        Self::command_item(
                            &item,
                            &cmd,
                            "completed",
                            Some(&format!("ran {cmd}\n")),
                            Some(0),
                        )
                    };
                    self.item("item/completed", &turn, done);
                    format!("codex ran {cmd}")
                } else {
                    self.item(
                        "item/completed",
                        &turn,
                        Self::proposed_command_item(&item, &cmd, "declined"),
                    );
                    format!("codex was not allowed to run {cmd}")
                }
            }
            Script::Mystery => {
                let params = json!({ "itemId": self.item_id("call"), "threadId": self.thread_id(), "turnId": turn, "cwd": ".",
                                     "permissions": {}, "reason": null, "startedAtMs": now_ms(), "environmentId": null });
                let rid = self.server_request("item/permissions/requestApproval", params);
                let Some(resp) = self.wait_response(stdin, Some(rid)) else {
                    return self.interrupted(&turn);
                };
                match resp.get("error") {
                    Some(e) => format!(
                        "codex got back error: {}",
                        e["message"].as_str().unwrap_or("")
                    ),
                    None => format!("codex got back result: {}", resp["result"]),
                }
            }
            Script::Unknown => {
                self.notify("fake/unknownThing", json!({ "threadId": self.thread_id() }));
                format!("codex received: {text}")
            }
            Script::Default => format!("codex received: {text}"),
        };
        self.agent_message(&turn, &reply);
        self.charge(&turn);
        self.complete(&turn, "completed", Value::Null);
    }

    fn handle(&mut self, v: Value, stdin: &mut Stdin) {
        let Some(id) = v.get("id").cloned() else {
            return;
        };
        let Some(method) = v["method"].as_str() else {
            return;
        };
        let params = &v["params"];
        match method {
            "initialize" => self.result(&id, json!({ "codexHome": "/home/user/.codex", "platformFamily": "unix", "platformOs": "linux", "userAgent": "fake-codex/0.160.1" })),
            "thread/start" => {
                let thread = Self::new_thread_id();
                self.thread = Some(thread.clone());
                self.approval = params["approvalPolicy"].as_str().unwrap_or("never").to_string();
                self.total = [0, 0, 0];
                self.last_turn = None;
                self.save();
                let t = self.thread_json(&thread, None);
                self.result(&id, json!({ "thread": t, "approvalPolicy": self.approval, "sandbox": Self::sandbox_json(params), "cwd": params["cwd"], "model": params["model"].as_str().unwrap_or("fake-model") }));
                self.notify("thread/started", json!({ "thread": t }));
            }
            "thread/resume" | "thread/fork" => {
                let source = params["threadId"].as_str().unwrap_or_default().to_string();
                let Some((total, last_turn)) = self.load(&source) else {
                    return self.error(&id, -32600, &format!("thread not found: {source}"));
                };
                let fork = method == "thread/fork";
                let thread = if fork { Self::new_thread_id() } else { source.clone() };
                self.thread = Some(thread.clone());
                self.approval = params["approvalPolicy"].as_str().unwrap_or("never").to_string();
                self.total = total;
                self.last_turn = last_turn.clone();
                self.save();
                let t = self.thread_json(&thread, fork.then_some(source.as_str()));
                self.result(&id, json!({ "thread": t, "approvalPolicy": self.approval, "sandbox": Self::sandbox_json(params), "cwd": params["cwd"], "model": params["model"].as_str().unwrap_or("fake-model") }));
                self.replay_usage(last_turn);
                if fork {
                    self.notify("thread/started", json!({ "thread": t }));
                }
            }
            "turn/start" if self.thread.is_none() => self.error(&id, -32600, "no thread: call thread/start first"),
            "turn/start" => {
                let text = params["input"][0]["text"].as_str().unwrap_or_default().to_string();
                self.turn(&id, &text, stdin);
            }
            // Nothing is running between turns.
            "turn/interrupt" => self.result(&id, json!({})),
            m => self.error(&id, -32601, &format!("fake-codex does not handle {m}")),
        }
    }
}

/// The argv the server proposes for an exec-policy amendment. Recorded: a plain command is split
/// into words (`touch approval-a.txt` → `["touch","approval-a.txt"]`, codex-m2-approval@4647); one
/// with shell syntax stays `["/bin/bash","-lc",<cmd>]` (`printf hi > approval-b.txt`, @8562). The
/// real server's exact rule is unknown; fake-codex treats any shell metacharacter as shell syntax.
fn execpolicy_argv(cmd: &str) -> Vec<String> {
    if cmd.contains(|c: char| "|&;<>()$`\\\"'*?[]{}~#=\n".contains(c)) {
        vec!["/bin/bash".into(), "-lc".into(), cmd.into()]
    } else {
        cmd.split_whitespace().map(str::to_string).collect()
    }
}

/// An approval answer that lets the action run (`accept`, `acceptForSession`); anything else declines.
fn allows(resp: &Value) -> bool {
    matches!(
        resp["result"]["decision"].as_str(),
        Some("accept" | "acceptForSession")
    )
}

fn crash() -> ! {
    eprintln!("fake-codex: crash requested");
    std::process::exit(3)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let state = std::env::var_os("FAKE_CODEX_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".fake-codex")
        });
    let mut fake = Fake {
        native_questions: args
            .windows(2)
            .any(|w| w[0] == "--enable" && w[1] == "default_mode_request_user_input"),
        state,
        log: std::env::var_os("FAKE_CODEX_LOG").map(PathBuf::from),
        thread: None,
        approval: "never".into(),
        total: [0, 0, 0],
        last_turn: None,
        turns: 0,
        items: 0,
        server_ids: 0,
    };
    fake.log(&format!("argv: {}", args.join(" ")));
    let mut stdin: Stdin = std::io::stdin().lock().lines();
    while let Some(Ok(line)) = stdin.next() {
        fake.log(&format!("in: {line}"));
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            fake.handle(v, &mut stdin);
        }
    }
}
