//! fake-codex speaks the recorded app-server shapes. These tests drive the binary over stdio.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::Duration;

struct Fake {
    child: Child,
    stdin: ChildStdin,
    out: Receiver<Value>,
    next_id: u64,
}

impl Fake {
    fn start(args: &[&str], state: &Path, log: Option<&Path>) -> Fake {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake-codex"));
        cmd.args(args)
            .env("FAKE_CODEX_STATE", state)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(l) = log {
            cmd.env("FAKE_CODEX_LOG", l);
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, out) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let matching_items = BufReader::new(stdout).lines().map_while(Result::ok);
            for line in matching_items {
                if tx
                    .send(
                        serde_json::from_str::<Value>(&line).expect("fake-codex writes JSON lines"),
                    )
                    .is_err()
                {
                    return;
                }
            }
        });
        Fake {
            child,
            stdin,
            out,
            next_id: 1,
        }
    }

    fn write(&mut self, v: Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Sends a request and returns its id.
    fn request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.write(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    fn next(&self) -> Value {
        self.out
            .recv_timeout(Duration::from_secs(5))
            .expect("fake-codex answered in time")
    }

    /// Every line up to and including the first one `pred` accepts.
    fn until(&self, pred: impl Fn(&Value) -> bool) -> Vec<Value> {
        let mut seen = Vec::new();
        loop {
            let v = self.next();
            let hit = pred(&v);
            seen.push(v);
            if hit {
                return seen;
            }
        }
    }

    fn response(&self, id: u64) -> Value {
        self.until(|v| v["id"] == id && v.get("method").is_none())
            .pop()
            .unwrap()
    }

    /// initialize, initialized, then thread/start; returns the thread id.
    fn open(&mut self, approval: &str) -> String {
        let id = self.request(
            "initialize",
            json!({ "clientInfo": { "name": "test", "version": "0" } }),
        );
        assert!(
            self.response(id)["result"]["userAgent"]
                .as_str()
                .unwrap()
                .starts_with("fake-codex/")
        );
        self.write(json!({ "jsonrpc": "2.0", "method": "initialized" }));
        let id = self.request(
            "thread/start",
            json!({ "cwd": "/tmp", "approvalPolicy": approval, "sandbox": "danger-full-access" }),
        );
        self.response(id)["result"]["thread"]["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn turn(&mut self, thread: &str, text: &str) -> u64 {
        self.request(
            "turn/start",
            json!({ "threadId": thread, "input": [{ "type": "text", "text": text }] }),
        )
    }
}

fn method(v: &Value) -> &str {
    v["method"].as_str().unwrap_or("")
}
fn completed(v: &Value) -> bool {
    method(v) == "turn/completed"
}
fn agent_texts(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .filter(|v| method(v) == "item/completed" && v["params"]["item"]["type"] == "agentMessage")
        .map(|v| v["params"]["item"]["text"].as_str().unwrap().to_string())
        .collect()
}

/// The approval-relevant lines of a turn, in order, as labels: `active`/`waiting`/`idle` for
/// `thread/status/changed`, `started`/`completed` for the `item_type` item, `request` for a
/// `*/requestApproval` server request, `resolved`, and `turn/started`/`turn/completed`.
fn trace(lines: &[Value], item_type: &str) -> Vec<&'static str> {
    lines
        .iter()
        .filter_map(|v| match method(v) {
            "thread/status/changed" => Some(match &v["params"]["status"] {
                s if s["type"] == "idle" => "idle",
                s if s["activeFlags"] == json!(["waitingOnApproval"]) => "waiting",
                s if s["type"] == "active" && s["activeFlags"] == json!([]) => "active",
                _ => "other-status",
            }),
            "turn/started" => Some("turn/started"),
            "turn/completed" => Some("turn/completed"),
            "item/started" if v["params"]["item"]["type"] == item_type => Some("started"),
            "item/completed" if v["params"]["item"]["type"] == item_type => Some("completed"),
            "serverRequest/resolved" => Some("resolved"),
            m if m.ends_with("/requestApproval") => Some("request"),
            _ => None,
        })
        .collect()
}

#[test]
fn a_turn_follows_the_recorded_shapes() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("never");
    assert!(thread.starts_with("thr-"), "{thread}");
    let id = f.turn(&thread, "hello");
    let lines = f.until(completed);
    let resp = lines.iter().find(|v| v["id"] == id).unwrap();
    assert_eq!(resp["result"]["turn"]["status"], "inProgress");
    let turn = resp["result"]["turn"]["id"].as_str().unwrap();
    assert!(
        lines
            .iter()
            .any(|v| method(v) == "turn/started" && v["params"]["turn"]["id"] == turn)
    );
    assert!(
        lines
            .iter()
            .any(|v| method(v) == "item/completed" && v["params"]["item"]["type"] == "userMessage")
    );
    assert_eq!(agent_texts(&lines), ["codex received: hello"]);
    let usage = lines
        .iter()
        .find(|v| method(v) == "thread/tokenUsage/updated")
        .unwrap();
    assert_eq!(usage["params"]["turnId"], turn);
    assert_eq!(usage["params"]["tokenUsage"]["total"]["totalTokens"], 110);
    assert_eq!(
        usage["params"]["tokenUsage"]["total"]["cachedInputTokens"],
        40
    );
    assert_eq!(
        lines.last().unwrap()["params"]["turn"]["status"],
        "completed"
    );
}

#[test]
fn tool_hang_fail_and_unknown_behave_as_scripted() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("never");

    f.turn(&thread, "tool:ls -1");
    let lines = f.until(completed);
    let cmd = lines
        .iter()
        .find(|v| {
            method(v) == "item/completed" && v["params"]["item"]["type"] == "commandExecution"
        })
        .unwrap();
    assert_eq!(
        cmd["params"]["item"]["commandActions"][0]["command"],
        "ls -1"
    );
    assert_eq!(cmd["params"]["item"]["exitCode"], 0);
    assert_eq!(cmd["params"]["item"]["aggregatedOutput"], "ran ls -1\n");

    f.turn(&thread, "hang");
    let started = f
        .until(|v| method(v) == "item/started" && v["params"]["item"]["type"] == "commandExecution")
        .pop()
        .unwrap();
    let turn = started["params"]["turnId"].as_str().unwrap().to_string();
    let refused = f.turn(&thread, "too early");
    assert_eq!(
        f.response(refused)["error"]["message"],
        "a turn is already in progress"
    );
    let interrupt = f.request(
        "turn/interrupt",
        json!({ "threadId": thread, "turnId": turn }),
    );
    let lines = f.until(completed);
    assert!(
        lines
            .iter()
            .any(|v| v["id"] == interrupt && v["result"] == json!({}))
    );
    assert_eq!(
        lines.last().unwrap()["params"]["turn"]["status"],
        "interrupted"
    );

    f.turn(&thread, "Lead's notes\nfail");
    let lines = f.until(completed);
    let turn = &lines.last().unwrap()["params"]["turn"];
    assert_eq!(
        (turn["status"].as_str(), turn["error"]["message"].as_str()),
        (Some("failed"), Some("fake-codex: the model request failed"))
    );
    assert!(agent_texts(&lines).is_empty());

    f.turn(&thread, "unknown");
    let lines = f.until(completed);
    assert!(lines.iter().any(|v| method(v) == "fake/unknownThing"));
}

#[test]
fn ask_uses_request_user_input_only_with_the_flag() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(
        &["app-server", "--enable", "default_mode_request_user_input"],
        state.path(),
        None,
    );
    let thread = f.open("never");
    f.turn(&thread, "ask:Which colour?|Blue|Green");
    let req = f
        .until(|v| method(v) == "item/tool/requestUserInput")
        .pop()
        .unwrap();
    assert_eq!(req["id"], 0, "server request ids start at 0");
    let q = &req["params"]["questions"][0];
    assert_eq!(
        (q["id"].as_str(), q["question"].as_str()),
        (Some("q1"), Some("Which colour?"))
    );
    assert_eq!(q["options"][1]["label"], "Green");
    f.write(json!({ "id": 0, "jsonrpc": "2.0", "result": { "answers": { "q1": { "answers": ["Green"] } } } }));
    let lines = f.until(completed);
    assert!(
        lines
            .iter()
            .any(|v| method(v) == "serverRequest/resolved" && v["params"]["requestId"] == 0)
    );
    assert_eq!(agent_texts(&lines), ["codex got answer: Green"]);

    // Without the flag it asks in plain text, then waits in `sleep` items; the turn never ends by itself.
    let mut plain = Fake::start(&["app-server"], state.path(), None);
    let thread = plain.open("never");
    plain.turn(&thread, "ask:Which colour?|Blue|Green");
    let is_sleep = |v: &Value| v["params"]["item"]["type"] == "sleep";
    let sleeps = std::cell::Cell::new(0);
    let lines = plain.until(|v| {
        if method(v) == "item/started" && is_sleep(v) {
            sleeps.set(sleeps.get() + 1);
        }
        sleeps.get() == 2
    });
    assert!(
        !lines
            .iter()
            .any(|v| method(v) == "item/tool/requestUserInput" || completed(v))
    );
    assert_eq!(agent_texts(&lines), ["Which colour?"]);
    let slept = lines
        .iter()
        .find(|v| method(v) == "item/completed" && is_sleep(v))
        .unwrap();
    assert_eq!(slept["params"]["item"]["durationMs"], 60000);
    let turn = lines.last().unwrap()["params"]["turnId"]
        .as_str()
        .unwrap()
        .to_string();
    plain.request(
        "turn/interrupt",
        json!({ "threadId": thread, "turnId": turn }),
    );
    assert_eq!(
        plain.until(completed).last().unwrap()["params"]["turn"]["status"],
        "interrupted"
    );
}

#[test]
fn edit_asks_for_approval_unless_approvals_are_off() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("untrusted");
    f.turn(&thread, "edit:notes.txt");
    let mut all = f.until(|v| method(v) == "item/fileChange/requestApproval");
    let req = all.last().unwrap().clone();
    let id = req["id"].clone();
    f.write(json!({ "id": id, "jsonrpc": "2.0", "result": { "decision": "decline" } }));
    let lines = f.until(completed);
    all.extend(lines.iter().cloned());
    // codex-approval@4805-4806 (M0): the fileChange item starts before the waiting flag.
    assert_eq!(
        trace(&all, "fileChange"),
        [
            "active",
            "turn/started",
            "started",
            "waiting",
            "request",
            "resolved",
            "active",
            "completed",
            "idle",
            "turn/completed"
        ]
    );
    assert_eq!(
        agent_texts(&lines),
        ["codex was not allowed to change notes.txt"]
    );
    let item = lines
        .iter()
        .find(|v| method(v) == "item/completed" && v["params"]["item"]["type"] == "fileChange")
        .unwrap();
    assert_eq!(item["params"]["item"]["status"], "declined");

    let mut open = Fake::start(&["app-server"], state.path(), None);
    let thread = open.open("never");
    open.turn(&thread, "edit:notes.txt");
    let lines = open.until(completed);
    assert!(
        !lines
            .iter()
            .any(|v| method(v) == "item/fileChange/requestApproval")
    );
    assert_eq!(agent_texts(&lines), ["codex changed notes.txt"]);
}

/// The command approval path, as codex-m2-approval@4646-4648 (decline) and @8562-8596 (accept).
#[test]
fn run_asks_before_a_command_unless_approvals_are_off() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("untrusted");
    f.turn(&thread, "run:touch a.txt");
    let lines = f.until(|v| method(v) == "item/commandExecution/requestApproval");
    let mut all = lines.clone();
    let started = lines
        .iter()
        .find(|v| method(v) == "item/started" && v["params"]["item"]["type"] == "commandExecution")
        .unwrap();
    let item = &started["params"]["item"];
    assert_eq!(
        (item["status"].as_str(), item["source"].as_str()),
        (Some("inProgress"), Some("agent"))
    );
    assert!(lines.iter().any(|v| method(v) == "thread/status/changed"
        && v["params"]["status"]["activeFlags"] == json!(["waitingOnApproval"])));
    let req = lines.last().unwrap();
    let p = &req["params"];
    let mut keys: Vec<&str> = p.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "availableDecisions",
            "command",
            "commandActions",
            "cwd",
            "environmentId",
            "itemId",
            "kind",
            "proposedExecpolicyAmendment",
            "startedAtMs",
            "threadId",
            "turnId"
        ],
        "the recorded params, with no `reason`"
    );
    assert_eq!(p["itemId"], item["id"]);
    assert_eq!(p["kind"], "command");
    assert_eq!(p["commandActions"][0]["command"], "touch a.txt");
    assert_eq!(p["command"], "/bin/bash -lc 'touch a.txt'");
    assert!(
        !p["availableDecisions"]
            .as_array()
            .unwrap()
            .contains(&json!("decline")),
        "as recorded, `decline` is not listed but is honoured"
    );
    // A plain command is split into argv (codex-m2-approval@4647: ["touch","approval-a.txt"]).
    assert_eq!(p["proposedExecpolicyAmendment"], json!(["touch", "a.txt"]));
    assert_eq!(
        p["availableDecisions"],
        json!(["accept", { "acceptWithExecpolicyAmendment": { "execpolicy_amendment": ["touch", "a.txt"] } }, "cancel"])
    );
    f.write(json!({ "id": req["id"], "jsonrpc": "2.0", "result": { "decision": "decline" } }));
    let lines = f.until(completed);
    all.extend(lines.iter().cloned());
    // codex-m2-approval@4646-4648: the waiting flag comes before the command item starts.
    assert_eq!(
        trace(&all, "commandExecution"),
        [
            "active",
            "turn/started",
            "waiting",
            "started",
            "request",
            "resolved",
            "active",
            "completed",
            "idle",
            "turn/completed"
        ]
    );
    assert!(lines.iter().any(|v| method(v) == "serverRequest/resolved" && v["params"]["requestId"] == req["id"]));
    let done = lines
        .iter()
        .find(|v| {
            method(v) == "item/completed" && v["params"]["item"]["type"] == "commandExecution"
        })
        .unwrap();
    let item = &done["params"]["item"];
    assert_eq!(item["status"], "declined");
    assert!(
        item["aggregatedOutput"].is_null() && item.get("aggregatedOutput").is_some(),
        "aggregatedOutput is null, not absent: {item}"
    );
    assert!(item["exitCode"].is_null());
    assert!(
        done["params"].get("completedAtMs").is_some(),
        "item/completed carries completedAtMs"
    );
    assert_eq!(
        agent_texts(&lines),
        ["codex was not allowed to run touch a.txt"]
    );
    assert_eq!(
        lines.last().unwrap()["params"]["turn"]["status"],
        "completed",
        "a declined command does not end the turn"
    );

    f.turn(&thread, "run:printf hi > b.txt");
    let req = f
        .until(|v| method(v) == "item/commandExecution/requestApproval")
        .pop()
        .unwrap();
    // A command with shell syntax stays one bash -lc argv (codex-m2-approval@8562).
    assert_eq!(
        req["params"]["proposedExecpolicyAmendment"],
        json!(["/bin/bash", "-lc", "printf hi > b.txt"])
    );
    f.write(json!({ "id": req["id"], "jsonrpc": "2.0", "result": { "decision": "accept" } }));
    let lines = f.until(completed);
    let done = lines
        .iter()
        .find(|v| {
            method(v) == "item/completed" && v["params"]["item"]["type"] == "commandExecution"
        })
        .unwrap();
    assert_eq!(
        (
            done["params"]["item"]["status"].as_str(),
            done["params"]["item"]["exitCode"].as_i64()
        ),
        (Some("completed"), Some(0))
    );
    assert_eq!(
        done["params"]["item"]["aggregatedOutput"],
        "ran printf hi > b.txt\n"
    );
    assert_eq!(agent_texts(&lines), ["codex ran printf hi > b.txt"]);

    // run-quiet: an accepted command with no output: aggregatedOutput null, durationMs 0 (codex-m2-approval@8596).
    f.turn(&thread, "run-quiet:true");
    let req = f
        .until(|v| method(v) == "item/commandExecution/requestApproval")
        .pop()
        .unwrap();
    f.write(
        json!({ "id": req["id"], "jsonrpc": "2.0", "result": { "decision": "acceptForSession" } }),
    );
    let lines = f.until(completed);
    let item = &lines
        .iter()
        .find(|v| {
            method(v) == "item/completed" && v["params"]["item"]["type"] == "commandExecution"
        })
        .unwrap()["params"]["item"];
    assert_eq!(
        (
            item["status"].as_str(),
            item["exitCode"].as_i64(),
            item["durationMs"].as_i64()
        ),
        (Some("completed"), Some(0), Some(0))
    );
    assert!(
        item["aggregatedOutput"].is_null() && item.get("aggregatedOutput").is_some(),
        "{item}"
    );
    assert_eq!(agent_texts(&lines), ["codex ran true"]);

    let mut open = Fake::start(&["app-server"], state.path(), None);
    let thread = open.open("never");
    open.turn(&thread, "run:touch a.txt");
    let lines = open.until(completed);
    assert!(
        !lines
            .iter()
            .any(|v| method(v) == "item/commandExecution/requestApproval")
    );
    assert_eq!(agent_texts(&lines), ["codex ran touch a.txt"]);
}

/// Pins fake-codex's own choice for an unrecorded case: an interrupt while an approval is
/// pending answers `{}` and ends the turn `interrupted` with no `serverRequest/resolved` and no
/// clearing `activeFlags: []` line (the `idle` status line follows directly).
#[test]
fn an_interrupt_during_a_pending_approval_resolves_nothing() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("untrusted");
    f.turn(&thread, "run:touch a.txt");
    let mut all = f.until(|v| method(v) == "item/commandExecution/requestApproval");
    let turn = all.last().unwrap()["params"]["turnId"]
        .as_str()
        .unwrap()
        .to_string();
    let interrupt = f.request(
        "turn/interrupt",
        json!({ "threadId": thread, "turnId": turn }),
    );
    let lines = f.until(completed);
    assert!(
        lines
            .iter()
            .any(|v| v["id"] == interrupt && v["result"] == json!({}))
    );
    assert_eq!(
        lines.last().unwrap()["params"]["turn"]["status"],
        "interrupted"
    );
    all.extend(lines);
    assert_eq!(
        trace(&all, "commandExecution"),
        [
            "active",
            "turn/started",
            "waiting",
            "started",
            "request",
            "idle",
            "turn/completed"
        ]
    );
}

/// fake-codex does not implement `turn/steer` (the driver never sends it): -32601 between and
/// during turns, and the running turn is unaffected.
#[test]
fn turn_steer_is_not_handled() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("never");
    let id = f.request("turn/steer", json!({ "threadId": thread, "expectedTurnId": "turn-x", "input": [{ "type": "text", "text": "x" }] }));
    assert_eq!(f.response(id)["error"]["code"], -32601);

    f.turn(&thread, "hang");
    let started = f
        .until(|v| method(v) == "item/started" && v["params"]["item"]["type"] == "commandExecution")
        .pop()
        .unwrap();
    let turn = started["params"]["turnId"].as_str().unwrap().to_string();
    let id = f.request("turn/steer", json!({ "threadId": thread, "expectedTurnId": turn, "input": [{ "type": "text", "text": "x" }] }));
    assert_eq!(f.response(id)["error"]["code"], -32601);
    f.request(
        "turn/interrupt",
        json!({ "threadId": thread, "turnId": turn }),
    );
    assert_eq!(
        f.until(completed).last().unwrap()["params"]["turn"]["status"],
        "interrupted"
    );
}

/// Resume after an interrupt (codex-m2-resume@232): the replay names the interrupted turn, and
/// the thread takes a new turn.
#[test]
fn a_second_process_resumes_after_an_interrupt() {
    let state = tempfile::tempdir().unwrap();
    let mut first = Fake::start(&["app-server"], state.path(), None);
    let thread = first.open("never");
    first.turn(&thread, "hang");
    let started = first
        .until(|v| method(v) == "item/started" && v["params"]["item"]["type"] == "commandExecution")
        .pop()
        .unwrap();
    let turn = started["params"]["turnId"].clone();
    first.request(
        "turn/interrupt",
        json!({ "threadId": thread, "turnId": turn }),
    );
    first.until(completed);
    drop(first.stdin);
    first.child.wait().unwrap();

    let mut second = Fake::start(&["app-server"], state.path(), None);
    let id = second.request(
        "initialize",
        json!({ "clientInfo": { "name": "t", "version": "0" } }),
    );
    assert_eq!(
        second.response(id)["result"]["userAgent"],
        "fake-codex/0.160.1"
    );
    let id = second.request(
        "thread/resume",
        json!({ "threadId": thread, "approvalPolicy": "never", "sandbox": "danger-full-access" }),
    );
    let resumed = second.response(id);
    assert_eq!(resumed["result"]["thread"]["cliVersion"], "0.160.1");
    let replay = second
        .until(|v| method(v) == "thread/tokenUsage/updated")
        .pop()
        .unwrap();
    assert_eq!(
        replay["params"]["turnId"], turn,
        "the replay names the interrupted turn"
    );
    assert_eq!(replay["params"]["tokenUsage"]["total"]["totalTokens"], 110);
    second.turn(&thread, "hello");
    let lines = second.until(completed);
    assert_eq!(agent_texts(&lines), ["codex received: hello"]);
    let usage = lines
        .iter()
        .find(|v| method(v) == "thread/tokenUsage/updated")
        .unwrap();
    assert_eq!(usage["params"]["tokenUsage"]["total"]["totalTokens"], 220);
}

#[test]
fn mystery_reports_what_it_got_back() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("never");
    f.turn(&thread, "mystery");
    let req = f
        .until(|v| method(v) == "item/permissions/requestApproval")
        .pop()
        .unwrap();
    f.write(json!({ "id": req["id"], "jsonrpc": "2.0", "error": { "code": -32601, "message": "not handled" } }));
    let lines = f.until(completed);
    assert_eq!(agent_texts(&lines), ["codex got back error: not handled"]);
}

#[test]
fn a_second_process_resumes_and_forks_with_a_usage_replay() {
    let state = tempfile::tempdir().unwrap();
    let mut first = Fake::start(&["app-server"], state.path(), None);
    let thread = first.open("never");
    first.turn(&thread, "hello");
    let done = first.until(completed).pop().unwrap();
    let old_turn = done["params"]["turn"]["id"].clone();
    drop(first.stdin);
    first.child.wait().unwrap();

    let mut second = Fake::start(&["app-server"], state.path(), None);
    let id = second.request(
        "initialize",
        json!({ "clientInfo": { "name": "t", "version": "0" } }),
    );
    second.response(id);
    let id = second.request(
        "thread/resume",
        json!({ "threadId": thread, "approvalPolicy": "never", "sandbox": "danger-full-access" }),
    );
    assert_eq!(
        second.response(id)["result"]["thread"]["id"],
        thread.as_str()
    );
    let replay = second
        .until(|v| method(v) == "thread/tokenUsage/updated")
        .pop()
        .unwrap();
    assert_eq!(
        replay["params"]["turnId"], old_turn,
        "the replay names the old turn"
    );
    assert_eq!(replay["params"]["tokenUsage"]["total"]["totalTokens"], 110);

    let id = second.request(
        "thread/fork",
        json!({ "threadId": thread, "approvalPolicy": "never", "sandbox": "danger-full-access" }),
    );
    let fork = second.response(id);
    let forked = fork["result"]["thread"]["id"].as_str().unwrap();
    assert_ne!(forked, thread);
    assert_eq!(fork["result"]["thread"]["forkedFromId"], thread.as_str());

    let id = second.request("thread/resume", json!({ "threadId": "thr-nope" }));
    assert_eq!(
        second.response(id)["error"]["message"],
        "thread not found: thr-nope"
    );
}

#[test]
fn crash_exits_3_with_a_stderr_line_and_crash_once_crashes_once() {
    let state = tempfile::tempdir().unwrap();
    let mut f = Fake::start(&["app-server"], state.path(), None);
    let thread = f.open("never");
    f.turn(&thread, "crash-once");
    let status = f.child.wait().unwrap();
    assert_eq!(status.code(), Some(3));
    let mut err = String::new();
    std::io::Read::read_to_string(f.child.stderr.as_mut().unwrap(), &mut err).unwrap();
    assert!(err.contains("fake-codex: crash requested"), "{err}");

    let mut again = Fake::start(&["app-server"], state.path(), None);
    let thread = again.open("never");
    again.turn(
        &thread,
        "Your previous attempt did not finish.\n\ncrash-once",
    );
    let lines = again.until(completed);
    assert_eq!(
        lines.last().unwrap()["params"]["turn"]["status"],
        "completed"
    );
}

#[test]
fn the_log_records_argv_and_every_line_received() {
    let state = tempfile::tempdir().unwrap();
    let log = state.path().join("codex.log");
    let mut f = Fake::start(
        &["app-server", "--enable", "default_mode_request_user_input"],
        state.path(),
        Some(&log),
    );
    f.open("never");
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(
        text.lines()
            .next()
            .unwrap()
            .starts_with("argv: app-server --enable default_mode_request_user_input"),
        "{text}"
    );
    assert!(
        text.contains("in: {") && text.contains("\"method\":\"thread/start\""),
        "{text}"
    );
}
