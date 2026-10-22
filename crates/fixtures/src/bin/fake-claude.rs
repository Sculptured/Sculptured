//! Fake `claude -p --input-format stream-json --output-format stream-json` for tests.
//! Behaviour is selected by the user message text; see M1 plan Task 4.
//! Every line mirrors a shape in `crates/fixtures/recordings/claude-*.jsonl`
//! (Claude Code 2.1.289), trimmed to the keys Sculptured reads plus the
//! identifying keys `session_id`, `uuid` and `timestamp`.
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, Lines, StdinLock, Write};

/// Usage of one main-thread API message: input, output, cache read, cache write.
const MAIN: [u64; 4] = [12, 7, 3, 2];
/// Usage of the fake subagent's API message.
const SUB: [u64; 4] = [5, 4, 1, 1];
const TIMESTAMP: &str = "2026-10-05T00:00:00.000Z";
/// The synthetic tool_result text after an interrupt (claude-interrupt@2431), shortened.
const REJECTED: &str = "The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). STOP what you are doing and wait for the user to tell you how to proceed.";

type Stdin = Lines<StdinLock<'static>>;

fn reap_in_background(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn user_text(v: &Value) -> String {
    match &v["message"]["content"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn usage_json(u: [u64; 4]) -> Value {
    json!({ "input_tokens": u[0], "output_tokens": u[1],
            "cache_read_input_tokens": u[2], "cache_creation_input_tokens": u[3] })
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({ "type": "tool_use", "id": id, "name": name, "input": input, "caller": { "type": "direct" } })
}

fn thinking() -> Value {
    json!({ "type": "thinking", "thinking": "", "signature": "ZmFrZQ==" })
}

struct Fake {
    sid: String,
    permission_mode: &'static str,
    lines_out: u64,
    api_messages: u64,
    results: u64,
    /// Session-cumulative usage per model, reported as `result.modelUsage`
    /// (it grows across turns: claude-queue@2136, then @3522).
    model_usage: BTreeMap<&'static str, [u64; 4]>,
    /// Operator messages that arrived during a turn. Claude runs each as its own
    /// turn after the current `result` (claude-queue@3166).
    queued: VecDeque<String>,
    /// The next `result` reports `is_error: true` (the `limit` script).
    next_is_error: bool,
    /// This turn's main-thread API messages and their usage: `result.modelUsage` grows by their sum,
    /// as the real CLI's does (a turn with none is charged `MAIN`).
    turn_messages: BTreeMap<String, [u64; 4]>,
}

impl Fake {
    fn emit_raw(&self, v: &Value) {
        let mut out = std::io::stdout().lock();
        writeln!(out, "{v}").unwrap();
        out.flush().unwrap();
    }

    /// Emits a line with `session_id` and `uuid`, which every real stdout line
    /// carries except `control_response`.
    fn out(&mut self, mut v: Value) {
        self.lines_out += 1;
        v["session_id"] = json!(self.sid);
        v["uuid"] = json!(format!("00000000-0000-4000-8000-{:012}", self.lines_out));
        self.emit_raw(&v);
        if v["type"] == "assistant" || v["type"] == "user" {
            self.log(v);
        }
    }

    /// With `FAKE_CLAUDE_PROJECTS` set, keeps the session's messages in `<that folder>/<session>.jsonl`,
    /// timestamped now, as the real CLI keeps them under `~/.claude/projects/<folder>/`.
    fn log(&self, mut v: Value) {
        let Ok(dir) = std::env::var("FAKE_CLAUDE_PROJECTS") else {
            return;
        };
        v["timestamp"] = json!(fixtures::now_rfc3339());
        let path = std::path::Path::new(&dir).join(format!("{}.jsonl", self.sid));
        let _ = std::fs::create_dir_all(&dir);
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("session log");
        writeln!(f, "{v}").unwrap();
    }

    fn message_id(&mut self) -> String {
        self.api_messages += 1;
        format!("msg_fake_{}", self.api_messages)
    }

    /// SessionStart hooks run at process start, before any user line (claude-init@473).
    fn hooks(&mut self) {
        self.out(
            json!({ "type": "system", "subtype": "hook_started", "hook_id": "fake-hook-1",
                         "hook_name": "SessionStart:startup", "hook_event": "SessionStart" }),
        );
        self.out(json!({ "type": "system", "subtype": "hook_response", "hook_id": "fake-hook-1",
                         "hook_name": "SessionStart:startup", "hook_event": "SessionStart",
                         "output": "", "stdout": "", "stderr": "", "exit_code": 0, "outcome": "success" }));
    }

    /// Real init arrives only after a user line, then again at the start of every
    /// turn (claude-basic@531; claude-queue@927 and @2143). Its tool list says
    /// `Task`, although the wire tool name is `Agent`.
    fn init(&mut self) {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.out(
            json!({ "type": "system", "subtype": "init", "cwd": cwd, "model": "fake",
                         "permissionMode": self.permission_mode, "tools": ["Task", "Bash"],
                         "claude_code_version": "9.9.9" }),
        );
    }

    /// The only user line with `isReplay: true` (claude-basic@1579).
    fn replay(&mut self, text: &str) {
        self.out(json!({ "type": "user", "isReplay": true, "parent_tool_use_id": null, "timestamp": TIMESTAMP,
                         "message": { "role": "user", "content": [{ "type": "text", "text": text }] } }));
    }

    /// One content block per line. Lines of one API message share `message.id` and
    /// repeat its usage (claude-subagent@1834 and @3265).
    fn assistant(
        &mut self,
        message_id: &str,
        block: Value,
        parent: Option<&str>,
        model: &str,
        usage: [u64; 4],
    ) {
        let mut v = json!({ "type": "assistant", "parent_tool_use_id": parent,
                            "message": { "id": message_id, "type": "message", "role": "assistant", "model": model,
                                         "content": [block], "stop_reason": null, "usage": usage_json(usage) } });
        if parent.is_none() {
            self.turn_messages.insert(message_id.to_string(), usage);
        }
        if parent.is_some() {
            // Every real child line carries these (claude-subagent), as does the prompt line.
            v["subagent_type"] = json!("general-purpose");
            v["task_description"] = json!("list files");
            v["timestamp"] = json!(TIMESTAMP);
        }
        self.out(v);
    }

    /// A real child tool_result has no `tool_use_result` key (claude-subagent).
    fn tool_result(
        &mut self,
        id: &str,
        content: Value,
        parent: Option<&str>,
        tool_use_result: Value,
    ) {
        let mut v = json!({ "type": "user", "parent_tool_use_id": parent, "timestamp": TIMESTAMP,
                            "message": { "role": "user", "content": [
                                { "type": "tool_result", "tool_use_id": id, "content": content, "is_error": false } ] } });
        if parent.is_some() {
            v["subagent_type"] = json!("general-purpose");
            v["task_description"] = json!("list files");
        } else {
            v["tool_use_result"] = tool_use_result;
        }
        self.out(v);
    }

    /// `tool_use_result.stdout` duplicates `content` (claude-tool@1945).
    fn bash_result(&mut self, id: &str, stdout: &str, parent: Option<&str>) {
        self.tool_result(id, json!(stdout), parent, json!({ "stdout": stdout, "stderr": "", "interrupted": false,
                                                             "isImage": false, "noOutputExpected": false }));
    }

    fn rate_limit(&mut self) {
        self.out(json!({ "type": "rate_limit_event", "rate_limit_info": {
            "status": "allowed", "rateLimitType": "five_hour", "resetsAt": 1791250200, "isUsingOverage": false } }));
    }

    /// Adds `step` to the shared 5-hour use in `file` and reports it.
    fn plan_step(&mut self, file: &str, step: f64) {
        let used = plan_add(file, step);
        self.out(json!({ "type": "rate_limit_event", "rate_limit_info": {
            "status": "allowed", "rateLimitType": "five_hour", "resetsAt": 4102444800u64, "isUsingOverage": false,
            "unifiedWindows": { "five_hour": { "resetsAt": 4102444800u64, "utilization": used / 100.0 } } } }));
    }

    fn thinking_tokens(&mut self) {
        self.out(json!({ "type": "system", "subtype": "thinking_tokens",
                         "estimated_tokens": 5, "estimated_tokens_delta": 5 }));
    }

    /// Nested `response.response`, and no `session_id` (claude-interrupt@2355).
    fn control_response(&self, request: &Value) {
        self.emit_raw(&json!({ "type": "control_response", "response": {
            "subtype": "success", "request_id": request["request_id"], "response": { "still_queued": [] } } }));
    }

    fn charge(&mut self, model: &'static str, u: [u64; 4]) {
        let total = self.model_usage.entry(model).or_default();
        let paired_total = total.iter_mut().zip(u);
        for (t, n) in paired_total {
            *t += n;
        }
    }

    /// Ends the turn. `usage` is the main thread's share only, like the real
    /// `result.usage` (claude-subagent@10782). `modelUsage` is session-cumulative
    /// and includes subagents.
    fn result(&mut self, interrupted: bool, text: &str) {
        let turn = std::mem::take(&mut self.turn_messages);
        let used = if turn.is_empty() {
            MAIN
        } else {
            turn.values().fold([0; 4], |a, u| {
                [a[0] + u[0], a[1] + u[1], a[2] + u[2], a[3] + u[3]]
            })
        };
        self.charge("fake", used);
        let model_usage: Map<String, Value> = self
            .model_usage
            .iter()
            .map(|(model, u)| {
                let entry = json!({ "inputTokens": u[0], "outputTokens": u[1], "cacheReadInputTokens": u[2],
                                    "cacheCreationInputTokens": u[3], "costUSD": 0.0 });
                (model.to_string(), entry)
            })
            .collect();
        let mut v = json!({ "type": "result", "num_turns": 1, "result_index": self.results, "total_cost_usd": 0.0,
                            "usage": usage_json(MAIN), "modelUsage": Value::Object(model_usage),
                            "permission_denials": [] });
        if interrupted {
            // claude-interrupt@2434: no `result` text. The abort signal is `terminal_reason`.
            v["subtype"] = json!("error_during_execution");
            v["is_error"] = json!(true);
            v["stop_reason"] = json!("tool_use");
            v["terminal_reason"] = json!("aborted_tools");
            v["errors"] = json!([
                "[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=tool_use"
            ]);
        } else {
            v["subtype"] = json!("success");
            v["is_error"] = json!(std::mem::take(&mut self.next_is_error));
            v["stop_reason"] = json!("end_turn");
            v["terminal_reason"] = json!("completed");
            v["result"] = json!(text);
        }
        self.results += 1;
        self.out(v);
    }

    fn turn(&mut self, text: &str, stdin: &mut Stdin) {
        self.log(json!({ "type": "user", "session_id": self.sid, "message": { "role": "user", "content": text } }));
        // Interpret fixture commands inside the team's captured briefing envelope.
        let payload = fixtures::work_message(text);
        let text = payload.as_str();
        match text.trim() {
            "crash" => {
                eprintln!("fake crash");
                std::process::exit(3);
            }
            "hang" => self.hang(stdin),
            // Hard-stop tests: ignores Ctrl+C's SIGINT, starts a `sleep` in its own process group
            // (which inherits the ignored SIGINT), says the sleep's pid, then never ends the turn.
            #[cfg(unix)]
            "stubborn" => {
                // SAFETY: setting a signal disposition before any other thread could rely on it.
                unsafe { libc::signal(libc::SIGINT, libc::SIG_IGN) };
                let child = std::process::Command::new("sleep")
                    .arg("600")
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap();
                let child_pid = child.id();
                reap_in_background(child);
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": format!("child {child_pid}") }),
                    None,
                    "fake",
                    MAIN,
                );
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
            "subagent" => self.subagent(),
            "unknown" => {
                self.emit_raw(&json!({ "type": "mystery", "detail": 1 }));
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": "after mystery" }),
                    None,
                    "fake",
                    MAIN,
                );
                self.result(false, "after mystery");
            }
            "big" => {
                let m = self.message_id();
                self.assistant(
                    &m,
                    tool_use("toolu_big", "Bash", json!({ "command": "cat big" })),
                    None,
                    "fake",
                    MAIN,
                );
                self.bash_result("toolu_big", &"x".repeat(2_000_000), None);
                self.result(false, "");
            }
            // The user's real run (M2): the turn ends with an `is_error` result carrying the CLI's words.
            "limit" => {
                self.next_is_error = true;
                self.result(
                    false,
                    "You've hit your session limit · resets 4:30am (America/New_York)",
                );
            }
            // Test-only: replies with an environment variable's value ("unset" when absent), so a
            // test can see what the driver put in the CLI's environment.
            t if t.starts_with("env:") => {
                let name = t.trim_start_matches("env:").trim();
                let value = std::env::var(name).unwrap_or_else(|_| "unset".into());
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": value }),
                    None,
                    "fake",
                    MAIN,
                );
                self.result(false, &value);
            }
            // Load testing the web app: `busy:<n>` runs n Bash calls about 20 ms apart, like a busy agent.
            t if t.starts_with("busy:") => {
                let n: usize = t.trim_start_matches("busy:").trim().parse().unwrap_or(100);
                for i in 0..n {
                    let id = format!("toolu_busy_{i}");
                    let m = self.message_id();
                    self.assistant(
                        &m,
                        tool_use(&id, "Bash", json!({ "command": format!("step {i}") })),
                        None,
                        "fake",
                        MAIN,
                    );
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    self.bash_result(&id, &format!("step {i} ok"), None);
                }
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": "busy done" }),
                    None,
                    "fake",
                    MAIN,
                );
                self.result(false, "busy done");
            }
            // Budget tests: `spend:<calls>:<tokens>[:<pause ms>[:<plan file>[:<step>]]]` makes `calls` model
            // calls, each reading `tokens` from the cache, `pause` ms apart (default 20). With a plan
            // file, each call also adds `step` percent (default 0.5) to the 5-hour use kept in that
            // file, shared by every fake like one account, and reports it as a rate_limit_event.
            t if t.starts_with("spend:") => {
                let parts: Vec<&str> = t
                    .trim_start_matches("spend:")
                    .split(':')
                    .map(str::trim)
                    .collect();
                let num = |i: usize, d: u64| parts.get(i).and_then(|n| n.parse().ok()).unwrap_or(d);
                let (calls, tokens, pause) = (num(0, 10), num(1, 1000), num(2, 20));
                let plan = parts.get(3).filter(|f| !f.is_empty()).map(|f| {
                    (
                        f.to_string(),
                        parts.get(4).and_then(|n| n.parse().ok()).unwrap_or(0.5),
                    )
                });
                for i in 0..calls {
                    let (id, m) = (format!("toolu_spend_{i}"), self.message_id());
                    self.assistant(
                        &m,
                        tool_use(&id, "Bash", json!({ "command": format!("read {i}") })),
                        None,
                        "fake",
                        [0, 10, tokens, 0],
                    );
                    if let Some((file, step)) = &plan {
                        self.plan_step(file, *step);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(pause));
                    self.bash_result(&id, &format!("read {i} ok"), None);
                }
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": "spent" }),
                    None,
                    "fake",
                    [0, 5, tokens, 0],
                );
                self.result(false, "spent");
            }
            // Load tests: `chatty:<calls>:<bytes>[:<pause ms>]` makes `calls` tool calls whose output is
            // `bytes` long, `pause` ms apart (default 50), like an agent reading files.
            t if t.starts_with("chatty:") => {
                let parts: Vec<u64> = t
                    .trim_start_matches("chatty:")
                    .split(':')
                    .filter_map(|n| n.trim().parse().ok())
                    .collect();
                let (calls, bytes, pause) = (
                    parts.first().copied().unwrap_or(10),
                    parts.get(1).copied().unwrap_or(4000),
                    parts.get(2).copied().unwrap_or(50),
                );
                for i in 0..calls {
                    let (id, m) = (format!("toolu_chatty_{i}"), self.message_id());
                    self.assistant(
                        &m,
                        tool_use(
                            &id,
                            "Read",
                            json!({ "file_path": format!("/work/file{i}.txt") }),
                        ),
                        None,
                        "fake",
                        MAIN,
                    );
                    std::thread::sleep(std::time::Duration::from_millis(pause));
                    self.bash_result(&id, &"x".repeat(bytes as usize), None);
                }
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": "chatty done" }),
                    None,
                    "fake",
                    MAIN,
                );
                self.result(false, "chatty done");
            }
            // Context ceiling tests: `grow:<calls>:<start>:<step>` makes `calls` tool calls whose context
            // grows from `start` tokens by `step` per call, like an agent reading file after file.
            t if t.starts_with("grow:") => {
                let n: Vec<u64> = t
                    .trim_start_matches("grow:")
                    .split(':')
                    .filter_map(|x| x.trim().parse().ok())
                    .collect();
                let (calls, start, step) = (
                    n.first().copied().unwrap_or(3),
                    n.get(1).copied().unwrap_or(20_000),
                    n.get(2).copied().unwrap_or(20_000),
                );
                for i in 0..calls {
                    let (id, m) = (format!("toolu_grow_{i}"), self.message_id());
                    self.assistant(
                        &m,
                        tool_use(&id, "Read", json!({ "file_path": format!("part{i}.txt") })),
                        None,
                        "fake",
                        [0, 10, start + i * step, 0],
                    );
                    self.bash_result(&id, &format!("part {i}"), None);
                }
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": "grown" }),
                    None,
                    "fake",
                    [0, 5, start + calls * step, 0],
                );
                self.result(false, "grown");
            }
            t if t.starts_with("tool:") => {
                let cmd = t.trim_start_matches("tool:").trim();
                let m = self.message_id();
                self.assistant(
                    &m,
                    tool_use("toolu_1", "Bash", json!({ "command": cmd })),
                    None,
                    "fake",
                    MAIN,
                );
                self.rate_limit();
                self.bash_result("toolu_1", &format!("ran {cmd}"), None);
                let done = format!("done: {cmd}");
                let m = self.message_id();
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": done }),
                    None,
                    "fake",
                    MAIN,
                );
                self.result(false, &done);
            }
            t => {
                // claude-queue@3166-3522: a thinking line and a text line sharing one message.id.
                let reply = format!("fake: {t}");
                self.thinking_tokens();
                let m = self.message_id();
                self.assistant(&m, thinking(), None, "fake", MAIN);
                self.assistant(
                    &m,
                    json!({ "type": "text", "text": reply }),
                    None,
                    "fake",
                    MAIN,
                );
                self.rate_limit();
                self.result(false, &reply);
            }
        }
    }

    /// Stops after a Bash tool_use and waits for an interrupt, as in claude-interrupt,
    /// where the interrupt arrived after the tool_use and before the tool ran (@2342).
    /// The fake does not model an interrupt during thinking or text; M0 did not observe one.
    fn hang(&mut self, stdin: &mut Stdin) {
        let m = self.message_id();
        self.assistant(
            &m,
            tool_use("toolu_hang", "Bash", json!({ "command": "sleep 600" })),
            None,
            "fake",
            MAIN,
        );
        // UNOBSERVED: stdin EOF while hanging. The fake emits no `result` and falls
        // through to exit 0; M0 never closed stdin mid-turn.
        let matching_stdin = stdin.by_ref();
        for line in matching_stdin {
            let Ok(line) = line else { return };
            let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
            match v["type"].as_str() {
                // Claude queues a user line sent mid-turn and runs it as a later turn (claude-queue).
                Some("user") => self.queued.push_back(user_text(&v)),
                Some("control_request") => {
                    // claude-interrupt@2355-2434, in this order.
                    // UNOBSERVED: an interrupt while another message is queued. claude-interrupt
                    // had none queued and answered `still_queued: []`. The fake answers the same
                    // and keeps the queued message for the next turn.
                    self.control_response(&v);
                    self.out(json!({ "type": "user", "parent_tool_use_id": null, "timestamp": TIMESTAMP,
                                     "message": { "role": "user", "content": [{ "type": "tool_result",
                                         "tool_use_id": "toolu_hang", "content": REJECTED, "is_error": true }] },
                                     "tool_result_meta": [{ "id": "toolu_hang", "non_execution_kind": "user-rejected" }],
                                     "tool_use_result": "User rejected tool use" }));
                    self.out(json!({ "type": "user", "parent_tool_use_id": null, "timestamp": TIMESTAMP,
                                     "message": { "role": "user", "content": [
                                         { "type": "text", "text": "[Request interrupted by user for tool use]" }] } }));
                    self.result(true, "");
                    return;
                }
                _ => {}
            }
        }
    }

    /// claude-subagent@1834-10782: an `Agent` call whose child lines share the session
    /// id and carry `parent_tool_use_id`, framed by `system/task_*` lines.
    fn subagent(&mut self) {
        let parent = Some("toolu_task");
        let m = self.message_id();
        self.assistant(&m, thinking(), None, "fake", MAIN);
        let input = json!({ "description": "list files", "prompt": "ls",
                            "subagent_type": "general-purpose", "run_in_background": false });
        self.assistant(
            &m,
            tool_use("toolu_task", "Agent", input),
            None,
            "fake",
            MAIN,
        );
        self.out(json!({ "type": "system", "subtype": "task_started", "task_id": "task_fake_1",
                         "tool_use_id": "toolu_task", "description": "list files", "prompt": "ls",
                         "subagent_type": "general-purpose", "task_type": "local_agent", "spawn_depth": 1 }));
        // The subagent's prompt: a user text line with `parent_tool_use_id` and no `isReplay` (@3277).
        self.out(json!({ "type": "user", "parent_tool_use_id": "toolu_task", "timestamp": TIMESTAMP,
                         "subagent_type": "general-purpose", "task_description": "list files",
                         "message": { "role": "user", "content": [{ "type": "text", "text": "ls" }] } }));
        self.out(json!({ "type": "system", "subtype": "task_progress", "task_id": "task_fake_1",
                         "tool_use_id": "toolu_task", "description": "Running ls", "last_tool_name": "Bash",
                         "usage": { "total_tokens": 10, "tool_uses": 1, "duration_ms": 5 } }));
        // The recording has one model. A second `modelUsage` key exercises the sum over models.
        let child = self.message_id();
        self.assistant(
            &child,
            tool_use("toolu_sub_ls", "Bash", json!({ "command": "ls" })),
            parent,
            "fake-sub",
            SUB,
        );
        self.bash_result("toolu_sub_ls", "a.txt", parent);
        self.charge("fake-sub", SUB);
        self.out(
            json!({ "type": "system", "subtype": "task_updated", "task_id": "task_fake_1",
                         "patch": { "status": "completed" } }),
        );
        self.out(
            json!({ "type": "system", "subtype": "task_notification", "task_id": "task_fake_1",
                         "tool_use_id": "toolu_task", "status": "completed", "summary": "one file",
                         "usage": { "total_tokens": 11, "tool_uses": 1, "duration_ms": 5 } }),
        );
        // The hand-back is a parent-level tool_result whose content is a list of text parts (@8264).
        self.tool_result("toolu_task", json!([{ "type": "text", "text": "one file" }]), None,
                         json!({ "agentId": "task_fake_1", "agentType": "general-purpose", "status": "completed" }));
        let m = self.message_id();
        self.assistant(
            &m,
            json!({ "type": "text", "text": "subagent found one file" }),
            None,
            "fake",
            MAIN,
        );
        self.result(false, "subagent found one file");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Ok(path) = std::env::var("FAKE_CLAUDE_ARGV_LOG") {
        std::fs::write(path, args.join("\n")).unwrap();
    }
    if args.iter().any(|a| a == "--version") {
        println!("9.9.9 (Fake Claude)");
        return;
    }
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let resume = flag("--resume");
    let sid = resume
        .clone()
        .or_else(|| flag("--session-id"))
        .unwrap_or_else(|| "fake-session".into());
    if resume.is_some() && std::env::var("FAKE_CLAUDE_FAIL_RESUME").is_ok() {
        eprintln!("No conversation found with session ID: {sid}");
        std::process::exit(1);
    }
    let unrestricted = args.iter().any(|a| a == "--dangerously-skip-permissions");

    let mut fake = Fake {
        sid,
        permission_mode: if unrestricted {
            "bypassPermissions"
        } else {
            "default"
        },
        lines_out: 0,
        api_messages: 0,
        results: 0,
        model_usage: BTreeMap::new(),
        queued: VecDeque::new(),
        next_is_error: false,
        turn_messages: BTreeMap::new(),
    };
    fake.hooks();
    // FAKE_CLAUDE_IGNORE_EOF: ignore stdin EOF and keep running, with a detached
    // grandchild (new session, so killpg misses it) holding stderr open.
    // FAKE_CLAUDE_BAD_UTF8: emit a non-UTF-8 stdout line first.
    let ignore_eof = std::env::var("FAKE_CLAUDE_IGNORE_EOF").is_ok();
    if ignore_eof {
        let child = std::process::Command::new("sh")
            .args(["-c", "setsid sleep 30"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn grandchild");
        reap_in_background(child);
    }
    if std::env::var("FAKE_CLAUDE_BAD_UTF8").is_ok() {
        let mut out = std::io::stdout().lock();
        out.write_all(b"\xff\xfe\n").unwrap();
        out.flush().unwrap();
    }

    let mut stdin: Stdin = std::io::stdin().lock().lines();
    loop {
        let text = match fake.queued.pop_front() {
            Some(text) => text,
            None => {
                let Some(Ok(line)) = stdin.next() else {
                    if ignore_eof {
                        loop {
                            std::thread::sleep(std::time::Duration::from_secs(3600));
                        }
                    }
                    break;
                };
                let Ok(v) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                match v["type"].as_str() {
                    Some("user") => user_text(&v),
                    // UNOBSERVED: an interrupt with no turn running. The fake only acknowledges it.
                    Some("control_request") => {
                        fake.control_response(&v);
                        continue;
                    }
                    _ => continue,
                }
            }
        };
        fake.init();
        fake.replay(&text);
        fake.turn(&text, &mut stdin);
    }
}

/// Adds `step` to the percent in `file` under an exclusive lock (several fakes share it) and returns the new value.
fn plan_add(file: &str, step: f64) -> f64 {
    use std::io::{Read, Seek};
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(file)
        .unwrap();
    f.lock().expect("f mutex must not be poisoned");
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    let used = s.trim().parse::<f64>().unwrap_or(0.0) + step;
    f.set_len(0).unwrap();
    f.rewind().unwrap();
    write!(f, "{used}").unwrap();
    used
}
