//! Server requests: native questions and approvals become operator questions; anything else is refused.
use super::items::changes;
use super::{CodexParser, Parsed, Pending};
use crate::DriverEvent;
use model::{QuestionOption, QuestionPrompt, QuestionSource};
use serde_json::{Value, json};
use std::collections::HashMap;

type Asked = (QuestionPrompt, QuestionSource, Pending);

/// Adds an approval request's `reason` when it has one. `item/fileChange/requestApproval` carried one
/// in M0 (codex-approval@4805); `item/commandExecution/requestApproval` has no such key in 0.160.1
/// (codex-m2-approval@4647), so nothing is invented for it.
fn with_reason(mut input: Value, params: &Value) -> Value {
    if let Some(reason) = params["reason"].as_str() {
        input["reason"] = Value::String(reason.to_string());
    }
    input
}

fn permission_prompt(text: String, tool: &str, input: Value) -> QuestionPrompt {
    QuestionPrompt {
        urgency: None,
        header: None,
        text,
        options: vec![],
        allow_other: false,
        tool: Some(tool.into()),
        tool_input: Some(input),
    }
}

/// `requestUserInput`: the first question is asked; None when there is none.
fn user_input(p: &Value, id: &Value) -> Option<Asked> {
    let qs = p["questions"].as_array().cloned().unwrap_or_default();
    let first = qs.first()?;
    let options = first["options"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|o| QuestionOption {
            label: o["label"].as_str().unwrap_or_default().to_string(),
            description: crate::codex::parser::s(&o["description"]),
        })
        .collect();
    let prompt = QuestionPrompt {
        urgency: None,
        header: crate::codex::parser::s(&first["header"]),
        text: first["question"].as_str().unwrap_or_default().to_string(),
        options,
        allow_other: first["isOther"].as_bool().unwrap_or(true),
        tool: None,
        tool_input: None,
    };
    let others = qs
        .iter()
        .skip(1)
        .filter_map(|q| crate::codex::parser::s(&q["id"]))
        .collect();
    let key = crate::codex::parser::s(&first["id"]).unwrap_or_default();
    Some((
        prompt,
        QuestionSource::NativeQuestion,
        Pending::Question {
            rpc_id: id.clone(),
            key,
            others,
        },
    ))
}

fn file_change(p: &Value, id: &Value, items: &HashMap<String, Value>) -> Asked {
    let item = crate::codex::parser::s(&p["itemId"])
        .and_then(|i| items.get(&i).cloned())
        .unwrap_or(Value::Null);
    let changes = changes(&item);
    let paths: Vec<&str> = changes.iter().filter_map(|c| c["path"].as_str()).collect();
    let what = if paths.is_empty() {
        "files".to_string()
    } else {
        paths.join(", ")
    };
    let input = with_reason(json!({ "changes": changes }), p);
    let prompt = permission_prompt(
        format!("Allow Codex to change {what}?"),
        "fileChange",
        input,
    );
    (
        prompt,
        QuestionSource::NativePermission,
        Pending::Permission { rpc_id: id.clone() },
    )
}

/// Always the whole command, verbatim (see `full_command`). Schema `kind` (`command` | `writeStdin`,
/// default `command`) and `networkApprovalContext` ({host, protocol}) say what is under review; only
/// `command` was recorded (0.160.1).
fn command_approval(p: &Value, id: &Value) -> Asked {
    let command = crate::codex::parser::items::full_command(p);
    let mut input = with_reason(json!({ "command": command, "cwd": p["cwd"] }), p);
    let network = &p["networkApprovalContext"];
    let text = if let Some(host) = network["host"].as_str() {
        input["network"] = network.clone();
        let protocol = network["protocol"].as_str().unwrap_or("unknown");
        format!("Allow Codex network access to {host} ({protocol}) for `{command}`?")
    } else if p["kind"] == "writeStdin" {
        format!("Allow Codex to send input to the running command `{command}`?")
    } else {
        format!("Allow Codex to run `{command}`?")
    };
    if let Some(kind) = p["kind"].as_str().filter(|k| *k != "command") {
        input["kind"] = Value::String(kind.to_string());
    }
    let prompt = permission_prompt(text, "commandExecution", input);
    (
        prompt,
        QuestionSource::NativePermission,
        Pending::Permission { rpc_id: id.clone() },
    )
}

impl CodexParser {
    pub(super) fn server_request(&mut self, method: &str, id: Value, v: &Value, out: &mut Parsed) {
        let p = &v["params"];
        let asked = match method {
            "item/tool/requestUserInput" => user_input(p, &id),
            "item/fileChange/requestApproval" => Some(file_change(p, &id, &self.items)),
            "item/commandExecution/requestApproval" => Some(command_approval(p, &id)),
            _ => None,
        };
        let Some((prompt, source, pending)) = asked else {
            return self.refuse(method, id, v, out);
        };
        let question_id = self.question_id(&id);
        self.by_rpc
            .insert(crate::codex::parser::rpc_text(&id), question_id.clone());
        self.pending.insert(question_id.clone(), pending);
        out.events.push(DriverEvent::QuestionRaised {
            question_id,
            source,
            prompt,
            turn_id: crate::codex::parser::s(&p["turnId"]),
        });
    }

    /// Nobody here can answer it; leaving it unanswered would hang the turn.
    fn refuse(&mut self, method: &str, id: Value, v: &Value, out: &mut Parsed) {
        out.replies.push(json!({
            "jsonrpc": "2.0", "id": id,
            "error": { "code": -32601, "message": format!("Sculptured does not handle {method}") },
        }));
        out.events
            .push(DriverEvent::Unrecognized { raw: v.clone() });
    }
}
