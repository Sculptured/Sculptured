use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

fn run(input: &str) -> (Vec<Value>, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fake-claude"))
        .args([
            "-p",
            "--session-id",
            "s-1",
            "--dangerously-skip-permissions",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let lines = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (lines, out.status.code().unwrap())
}

fn user(text: &str) -> String {
    format!(
        "{}\n",
        json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": text }] } })
    )
}

/// `type`, or `type/subtype` when the line has a subtype.
fn kinds(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .map(|v| match v["subtype"].as_str() {
            Some(s) => format!("{}/{s}", v["type"].as_str().unwrap()),
            None => v["type"].as_str().unwrap().to_string(),
        })
        .collect()
}

#[test]
fn fake_claude_startup_emits_hooks_but_no_init() {
    // claude-init: with no user message, only the SessionStart hook lines appear.
    let (lines, code) = run("");
    assert_eq!(code, 0);
    assert_eq!(
        kinds(&lines),
        ["system/hook_started", "system/hook_response"]
    );
    assert_eq!(lines[0]["session_id"], "s-1");
}

#[test]
fn fake_claude_basic_turn() {
    let (lines, code) = run(&user("hello"));
    assert_eq!(code, 0);
    assert_eq!(
        kinds(&lines),
        [
            "system/hook_started",
            "system/hook_response",
            "system/init",
            "user",
            "system/thinking_tokens",
            "assistant",
            "assistant",
            "rate_limit_event",
            "result/success",
        ]
    );
    assert_eq!(lines[2]["session_id"], "s-1");
    assert_eq!(lines[2]["permissionMode"], "bypassPermissions");
    assert_eq!(lines[3]["isReplay"], true);
    assert!(lines[3]["parent_tool_use_id"].is_null());
    assert_eq!(lines[5]["message"]["content"][0]["type"], "thinking");
    assert_eq!(
        lines[5]["message"]["id"], lines[6]["message"]["id"],
        "one API message, two lines"
    );
    assert_eq!(lines[6]["message"]["content"][0]["text"], "fake: hello");
    assert_eq!(lines[8]["usage"]["input_tokens"], 12);
    assert_eq!(lines[8]["modelUsage"]["fake"]["inputTokens"], 12);
}

#[test]
fn fake_claude_interrupt_matches_recorded_shape_and_keeps_queued_message() {
    let interrupt = "{\"type\":\"control_request\",\"request_id\":\"i-1\",\"request\":{\"subtype\":\"interrupt\"}}\n";
    let (lines, code) = run(&format!("{}{}{interrupt}", user("hang"), user("after")));
    assert_eq!(code, 0);
    assert_eq!(
        kinds(&lines),
        [
            "system/hook_started",
            "system/hook_response",
            "system/init",
            "user",
            "assistant",
            "control_response",
            "user",
            "user",
            "result/error_during_execution",
            "system/init",
            "user",
            "system/thinking_tokens",
            "assistant",
            "assistant",
            "rate_limit_event",
            "result/success",
        ]
    );
    assert_eq!(lines[4]["message"]["content"][0]["id"], "toolu_hang");
    assert_eq!(lines[5]["response"]["request_id"], "i-1");
    assert_eq!(lines[5]["response"]["response"]["still_queued"], json!([]));
    assert_eq!(
        lines[6]["message"]["content"][0]["tool_use_id"],
        "toolu_hang"
    );
    assert_eq!(lines[6]["message"]["content"][0]["is_error"], true);
    assert_eq!(
        lines[7]["message"]["content"][0]["text"],
        "[Request interrupted by user for tool use]"
    );
    assert!(lines[7].get("isReplay").is_none());
    assert_eq!(lines[8]["stop_reason"], "tool_use");
    assert_eq!(lines[8]["terminal_reason"], "aborted_tools");
    assert_eq!(lines[10]["isReplay"], true);
    assert_eq!(lines[10]["message"]["content"][0]["text"], "after");
    assert_eq!(
        lines[15]["modelUsage"]["fake"]["outputTokens"], 14,
        "modelUsage is session-cumulative"
    );
}

#[test]
fn fake_claude_crash_exits_3() {
    let (_, code) =
        run("{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"crash\"}}\n");
    assert_eq!(code, 3);
}
