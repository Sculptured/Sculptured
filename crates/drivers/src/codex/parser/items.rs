//! Thread items: tool calls and their results, agent text, and items kept as evidence.
use super::{CodexParser, Parsed};
use crate::DriverEvent;
use serde_json::{Value, json};

/// Item types with nothing to show beyond what other events carry.
/// `sleep` is not quiet: it is how a Codex agent waits after asking in plain text (codex-user-input-noflag),
/// and it shows as a tool call so the wait is visible.
const QUIET_ITEMS: [&str; 2] = ["userMessage", "reasoning"];

/// Item types that are not tool calls but are handled on `item/completed` (text or quiet).
const KNOWN_ITEMS: [&str; 3] = ["agentMessage", "userMessage", "reasoning"];

/// The command Codex will run, verbatim: `command` itself. `commandActions` is the server's
/// "best-effort parsed command actions for friendly display" (schema) and may show only part of a
/// compound command (`cat notes.txt && rm -rf ~/work` parses to two actions), so it never stands in
/// for the command. Only when `command` is missing are all the actions shown, one per line.
pub(super) fn full_command(v: &Value) -> String {
    if let Some(c) = v["command"].as_str() {
        return c.to_string();
    }
    let actions: Vec<&str> = v["commandActions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["command"].as_str())
        .collect();
    if actions.is_empty() {
        "unknown".to_string()
    } else {
        actions.join("\n")
    }
}

/// A file change's changes as `{path, kind}`.
pub(super) fn changes(item: &Value) -> Vec<Value> {
    item["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| json!({ "path": c["path"], "kind": c["kind"]["type"] }))
        .collect()
}

fn tool_call(item: &Value, turn: &str) -> Option<DriverEvent> {
    let (name, input) = match item["type"].as_str()? {
        "commandExecution" => (
            "commandExecution".to_string(),
            json!({ "command": full_command(item), "cwd": item["cwd"] }),
        ),
        "fileChange" => (
            "fileChange".to_string(),
            json!({ "changes": changes(item) }),
        ),
        "mcpToolCall" => (
            format!(
                "mcp__{}__{}",
                item["server"].as_str().unwrap_or("unknown"),
                item["tool"].as_str().unwrap_or("unknown")
            ),
            item["arguments"].clone(),
        ),
        "webSearch" => ("webSearch".to_string(), json!({ "query": item["query"] })),
        "sleep" => (
            "sleep".to_string(),
            json!({ "durationMs": item["durationMs"] }),
        ),
        _ => return None,
    };
    Some(DriverEvent::ToolCall {
        turn_id: Some(turn.to_string()),
        call_id: crate::codex::parser::s(&item["id"])?,
        name,
        input,
        parent_call_id: None,
    })
}

/// `aggregatedOutput` is null, not "", when a command printed nothing or never ran
/// (codex-m2-approval@4648 declined, @8596 exit 0). A command that did not complete and
/// left no output shows its status word verbatim (`declined`), so the card says why.
fn command_output(item: &Value, status: &str) -> (String, bool) {
    let failed = item["exitCode"].as_i64().is_some_and(|c| c != 0) || status != "completed";
    let output = item["aggregatedOutput"].as_str().unwrap_or("");
    let output = if output.is_empty() && status != "completed" {
        status
    } else {
        output
    };
    (output.to_string(), failed)
}

fn mcp_output(item: &Value, status: &str) -> (String, bool) {
    match item["error"]["message"].as_str() {
        Some(m) => (m.to_string(), true),
        None => {
            let text: Vec<&str> = item["result"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c["text"].as_str())
                .collect();
            (text.join("\n"), status == "failed")
        }
    }
}

fn tool_result(item: &Value, turn: &str) -> DriverEvent {
    let status = item["status"].as_str().unwrap_or("");
    let (output, is_error) = match item["type"].as_str().unwrap_or("") {
        "commandExecution" => command_output(item, status),
        "fileChange" => {
            let n = item["changes"].as_array().map_or(0, Vec::len);
            (
                format!("{status}: {n} file{}", if n == 1 { "" } else { "s" }),
                status != "completed",
            )
        }
        "mcpToolCall" => mcp_output(item, status),
        _ => (String::new(), status == "failed"),
    };
    DriverEvent::ToolResult {
        turn_id: Some(turn.to_string()),
        call_id: crate::codex::parser::s(&item["id"]).unwrap_or_default(),
        output,
        is_error,
    }
}

impl CodexParser {
    pub(super) fn item_started(&mut self, item: &Value, turn: String, out: &mut Parsed) {
        if let Some(call) = tool_call(item, &turn) {
            let id = crate::codex::parser::s(&item["id"]).unwrap_or_default();
            self.items.insert(id.clone(), item.clone());
            self.open_calls.push((id, turn));
            out.events.push(call);
        } else if !KNOWN_ITEMS.contains(&item["type"].as_str().unwrap_or("")) {
            // An item type nobody here maps: kept now, so one that never completes still leaves a
            // trace. Its `item/completed` then adds nothing (one piece of evidence per item).
            if let Some(id) = crate::codex::parser::s(&item["id"]) {
                self.unknown_items.insert(id, turn);
            }
            out.events
                .push(DriverEvent::Unrecognized { raw: item.clone() });
        }
    }

    pub(super) fn item_completed(&mut self, item: &Value, turn: String, out: &mut Parsed) {
        let kind = item["type"].as_str().unwrap_or("");
        if kind == "agentMessage" {
            if let Some(text) = item["text"].as_str().filter(|t| !t.is_empty()) {
                out.events.push(DriverEvent::Text {
                    turn_id: Some(turn),
                    text: text.to_string(),
                });
            }
        } else if let Some(call) = tool_call(item, &turn) {
            let id = crate::codex::parser::s(&item["id"]).unwrap_or_default();
            match self.open_calls.iter().position(|(c, _)| *c == id) {
                Some(i) => {
                    self.open_calls.remove(i);
                }
                None => out.events.push(call),
            }
            self.items.remove(&id);
            out.events.push(tool_result(item, &turn));
        } else if !QUIET_ITEMS.contains(&kind) {
            let kept_at_start = crate::codex::parser::s(&item["id"])
                .is_some_and(|id| self.unknown_items.remove(&id).is_some());
            if !kept_at_start {
                out.events
                    .push(DriverEvent::Unrecognized { raw: item.clone() });
            }
        }
    }
}
