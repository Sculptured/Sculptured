//! OpenCode's ACP lines turned into driver events, as recorded with OpenCode 1.19.4
//! (`crates/fixtures/recordings/opencode-*.jsonl`).
//!
//! - Text and thoughts arrive as chunks (`agent_message_chunk`, `agent_thought_chunk`) under one
//!   `messageId` per model call; they are kept until that message ends (another message, a tool call,
//!   the turn's end) and emitted whole.
//! - A tool call is announced (`ToolCall`) at its first update that is running or finished: the
//!   `tool_call` itself carries only `{cwd}` as input (opencode-tool), the update the real input.
//! - Cost: `usage_update.cost` is OpenCode's own estimate of the *session's* total. A turn's cost is how
//!   much that total rose. It is unknown for the first turn of a resumed session (the earlier total
//!   is not reported, opencode-resume-2), and a total that falls (0 after a cancel, opencode-interrupt)
//!   is ignored.
//! - Tokens: the prompt's `usage` covers only the turn's last model call (OpenCode's
//!   `latestAssistantMessage`), so they understate a turn that called the model more than once.
use crate::DriverEvent;
use model::{
    Answer, ContextBasis, Cost, CostSource, LimitKind, QuestionPrompt, QuestionSource,
    ThinkingForm, TurnStatus,
};
use serde_json::{Value, json};
use std::collections::HashMap;

/// The result of a tool call still running when its turn ended.
pub const TOOL_UNFINISHED: &str = "(no result: the turn ended before this finished)";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Thought,
}

struct Chunks {
    kind: Kind,
    id: String,
    text: String,
}

struct Tool {
    name: String,
    input: Value,
    announced: bool,
}

/// A permission request waiting for the operator: its JSON-RPC id and its options' ids by kind.
struct Asked {
    rpc_id: Value,
    options: Vec<(String, String)>,
}

/// What one line produced: events, replies to write, and whether the running turn ended.
#[derive(Debug, Default)]
pub struct Parsed {
    pub events: Vec<DriverEvent>,
    pub replies: Vec<Value>,
    pub turn_ended: bool,
}

#[derive(Default)]
pub struct AcpParser {
    /// The running prompt's request id and turn id.
    prompt: Option<(u64, String)>,
    chunks: Option<Chunks>,
    tools: HashMap<String, Tool>,
    tool_order: Vec<String>,
    asked: HashMap<String, Asked>,
    /// The session's total cost as last reported; None until known (a resumed session).
    cost_seen: Option<f64>,
    /// This turn's cost so far; None when it cannot be known.
    turn_cost: Option<f64>,
}

impl AcpParser {
    /// A parser for a session that starts now (`fresh`: its cost so far is 0) or is resumed.
    #[must_use]
    pub fn new(fresh: bool) -> Self {
        Self {
            cost_seen: fresh.then_some(0.0),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn busy(&self) -> bool {
        self.prompt.is_some()
    }

    #[must_use]
    pub fn current_turn(&self) -> Option<&str> {
        self.prompt.as_ref().map(|(_, t)| t.as_str())
    }

    /// The prompt request `id` was sent for `turn`.
    pub fn on_prompt_sent(&mut self, id: u64, turn: String) {
        self.prompt = Some((id, turn));
        self.turn_cost = self.cost_seen.map(|_| 0.0);
    }

    fn turn(&self) -> Option<String> {
        self.current_turn().map(str::to_string)
    }

    pub fn parse_line(&mut self, line: &str) -> Parsed {
        let mut out = Parsed::default();
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            out.events.push(DriverEvent::Unrecognized {
                raw: json!({ "stdout": line }),
            });
            return out;
        };
        match (v.get("method").and_then(Value::as_str), v.get("id")) {
            (Some(method), Some(id)) => self.on_request(method, id, &v["params"], &mut out),
            (Some("session/update"), None) => self.on_update(&v["params"]["update"], &mut out),
            (Some(_), None) => out.events.push(DriverEvent::Unrecognized { raw: v }),
            (None, _) => self.on_response(&v, &mut out),
        }
        out
    }

    fn on_response(&mut self, v: &Value, out: &mut Parsed) {
        let ours = self
            .prompt
            .as_ref()
            .is_some_and(|(id, _)| v["id"].as_u64() == Some(*id));
        if ours {
            self.end_turn(v, out);
        }
    }
}

/// Session updates.
impl AcpParser {
    fn on_update(&mut self, u: &Value, out: &mut Parsed) {
        match u["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => self.chunk(Kind::Text, u, out),
            "agent_thought_chunk" => self.chunk(Kind::Thought, u, out),
            "tool_call" | "tool_call_update" => self.tool_update(u, out),
            "usage_update" => self.usage_update(u, out),
            // Noise: the slash commands, the model list, the mode, the title, our own prompt.
            "available_commands_update"
            | "config_option_update"
            | "current_mode_update"
            | "session_info_update"
            | "user_message_chunk" => {}
            _ => out
                .events
                .push(DriverEvent::Unrecognized { raw: u.clone() }),
        }
    }

    fn chunk(&mut self, kind: Kind, u: &Value, out: &mut Parsed) {
        let id = u["messageId"].as_str().unwrap_or_default().to_string();
        let text = u["content"]["text"].as_str().unwrap_or_default();
        if self
            .chunks
            .as_ref()
            .is_some_and(|c| c.kind != kind || c.id != id)
        {
            self.flush(out);
        }
        self.chunks
            .get_or_insert_with(|| Chunks {
                kind,
                id,
                text: String::new(),
            })
            .text
            .push_str(text);
    }

    /// Emits the message kept so far, if it has any text.
    fn flush(&mut self, out: &mut Parsed) {
        let Some(c) = self.chunks.take() else {
            return;
        };
        if c.text.trim().is_empty() {
            return;
        }
        let turn_id = self.turn();
        out.events.push(match c.kind {
            Kind::Text => DriverEvent::Text {
                turn_id,
                text: c.text,
            },
            Kind::Thought => DriverEvent::Thinking {
                turn_id,
                item_id: Some(c.id),
                parent_call_id: None,
                form: ThinkingForm::Full,
                text: c.text,
                estimated_tokens: None,
            },
        });
    }

    fn usage_update(&mut self, u: &Value, out: &mut Parsed) {
        let used = u["used"].as_u64().unwrap_or(0);
        // A cancelled or failed turn reports 0: not a context figure.
        if used > 0 {
            out.events.push(DriverEvent::Context {
                turn_id: self.turn(),
                used_tokens: used,
                window_tokens: u["size"].as_u64().filter(|s| *s > 0),
                basis: ContextBasis::Reported,
            });
        }
        if let Some(total) = u["cost"]["amount"].as_f64() {
            self.cost_reported(total);
        }
    }

    fn cost_reported(&mut self, total: f64) {
        match self.cost_seen {
            Some(seen) if total > seen => {
                self.turn_cost = self.turn_cost.map(|c| c + (total - seen));
                self.cost_seen = Some(total);
            }
            Some(_) => {}
            None => self.cost_seen = Some(total),
        }
    }
}

/// Tool calls.
impl AcpParser {
    fn tool_update(&mut self, u: &Value, out: &mut Parsed) {
        self.flush(out);
        let Some(id) = u["toolCallId"].as_str().map(str::to_string) else {
            return;
        };
        if !self.tools.contains_key(&id) {
            let name = u["name"].as_str().or(u["title"].as_str()).unwrap_or("tool");
            self.tools.insert(
                id.clone(),
                Tool {
                    name: name.to_string(),
                    input: Value::Null,
                    announced: false,
                },
            );
            self.tool_order.push(id.clone());
        }
        let status = u["status"].as_str().unwrap_or("pending");
        let tool = self.tools.get_mut(&id).expect("inserted above");
        if !tool.announced && u.get("rawInput").is_some_and(|i| !i.is_null()) {
            tool.input = u["rawInput"].clone();
        }
        if status != "pending" {
            self.announce(&id, out);
        }
        if matches!(status, "completed" | "failed") {
            self.finish_tool(&id, tool_output(u), status == "failed", out);
        }
    }

    fn announce(&mut self, id: &str, out: &mut Parsed) {
        let turn_id = self.turn();
        let Some(tool) = self.tools.get_mut(id).filter(|t| !t.announced) else {
            return;
        };
        tool.announced = true;
        out.events.push(DriverEvent::ToolCall {
            turn_id,
            call_id: id.to_string(),
            name: tool.name.clone(),
            input: tool.input.clone(),
            parent_call_id: None,
        });
    }

    fn finish_tool(&mut self, id: &str, output: String, is_error: bool, out: &mut Parsed) {
        self.tools.remove(id);
        self.tool_order.retain(|t| t != id);
        out.events.push(DriverEvent::ToolResult {
            turn_id: self.turn(),
            call_id: id.to_string(),
            output,
            is_error,
        });
    }

    /// Tools still open when the turn ends get a result saying so.
    fn finish_open_tools(&mut self, out: &mut Parsed) {
        for id in std::mem::take(&mut self.tool_order) {
            self.announce(&id, out);
            self.finish_tool(&id, TOOL_UNFINISHED.to_string(), true, out);
        }
    }
}

/// A tool's result: OpenCode's `rawOutput.output`, else the text of its content.
fn tool_output(u: &Value) -> String {
    if let Some(s) = u["rawOutput"]["output"].as_str() {
        return s.to_string();
    }
    u["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["content"]["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The end of a prompt: its usage and how it ended.
impl AcpParser {
    fn end_turn(&mut self, v: &Value, out: &mut Parsed) {
        let turn = self.turn().expect("a prompt is running");
        self.flush(out);
        self.finish_open_tools(out);
        let cancelled = self.cancel_asked(out);
        out.replies.extend(cancelled);
        let status = match v.get("error") {
            Some(e) => failed(e, &turn, out),
            None => self.ended(&v["result"], &turn, out),
        };
        out.events.push(DriverEvent::TurnEnded {
            turn_id: turn,
            status,
        });
        self.prompt = None;
        out.turn_ended = true;
    }

    fn ended(&mut self, r: &Value, turn: &str, out: &mut Parsed) -> TurnStatus {
        if let Some(usage) = self.usage(&r["usage"], turn) {
            out.events.push(usage);
        }
        let stop = r["stopReason"].as_str().unwrap_or("end_turn");
        match stop {
            "end_turn" => TurnStatus::Completed,
            "cancelled" => TurnStatus::Interrupted,
            other => {
                out.events.push(DriverEvent::Unrecognized {
                    raw: json!({ "note": format!("OpenCode stopped the turn: {other}") }),
                });
                TurnStatus::Failed
            }
        }
    }

    /// The turn's usage: the last model call's tokens and the turn's cost. None when there is nothing.
    fn usage(&self, u: &Value, turn: &str) -> Option<DriverEvent> {
        let n = |k: &str| u[k].as_u64().unwrap_or(0);
        let cost = self.turn_cost.filter(|c| *c > 0.0).map(|usd| Cost {
            usd,
            source: CostSource::Opencode,
        });
        let output = n("outputTokens") + n("thoughtTokens");
        let any = n("inputTokens") + output + n("cachedReadTokens") + n("cachedWriteTokens") > 0;
        (any || cost.is_some()).then(|| DriverEvent::Usage {
            turn_id: Some(turn.to_string()),
            input_tokens: n("inputTokens"),
            output_tokens: output,
            cache_read_tokens: n("cachedReadTokens"),
            cache_write_tokens: n("cachedWriteTokens"),
            cost,
            in_turn: false,
        })
    }
}

/// A prompt that ended in an error: the error as evidence, and a limit when it names one.
fn failed(e: &Value, turn: &str, out: &mut Parsed) -> TurnStatus {
    let text = e["message"]
        .as_str()
        .unwrap_or("OpenCode failed the turn")
        .to_string();
    out.events.push(DriverEvent::Unrecognized {
        raw: json!({ "error": e }),
    });
    if let Some(kind) = limit_kind(&text) {
        out.events.push(DriverEvent::LimitReached {
            turn_id: Some(turn.to_string()),
            kind,
            resets: model::limit_resets(&text),
            text,
        });
    }
    TurnStatus::Failed
}

/// OpenRouter's words for running out of credit or being rate limited (opencode-error).
fn limit_kind(text: &str) -> Option<LimitKind> {
    let t = text.to_ascii_lowercase();
    if t.contains("insufficient credits") || t.contains("402") {
        Some(LimitKind::Usage)
    } else if t.contains("rate limit") || t.contains("429") {
        Some(LimitKind::Rate)
    } else {
        None
    }
}

/// Requests from OpenCode: permission prompts become operator questions; others are refused.
impl AcpParser {
    fn on_request(&mut self, method: &str, id: &Value, p: &Value, out: &mut Parsed) {
        if method != "session/request_permission" {
            out.replies
                .push(json!({ "jsonrpc": "2.0", "id": id, "error": {
                "code": -32601, "message": format!("Sculptured does not offer {method}") } }));
            return;
        }
        self.flush(out);
        let question_id = format!("perm-{}", id_text(id));
        let asked = Asked {
            rpc_id: id.clone(),
            options: option_kinds(&p["options"]),
        };
        self.asked.insert(question_id.clone(), asked);
        out.events.push(DriverEvent::QuestionRaised {
            question_id,
            source: QuestionSource::NativePermission,
            prompt: self.permission_prompt(&p["toolCall"]),
            turn_id: self.turn(),
        });
    }

    fn permission_prompt(&self, call: &Value) -> QuestionPrompt {
        let title = call["title"].as_str().unwrap_or("a tool");
        let tool = call["toolCallId"]
            .as_str()
            .and_then(|id| self.tools.get(id))
            .map_or_else(|| title.to_string(), |t| t.name.clone());
        QuestionPrompt {
            urgency: None,
            header: None,
            text: permission_text(call, title),
            options: vec![],
            allow_other: false,
            tool: Some(tool),
            tool_input: Some(call["rawInput"].clone()),
        }
    }

    /// The reply for the operator's answer to `question_id`; None when no such question is open.
    pub fn answer_reply(&mut self, question_id: &str, answer: &Answer) -> Option<Value> {
        let asked = self.asked.remove(question_id)?;
        let pick = |kinds: [&str; 2]| {
            kinds
                .iter()
                .find_map(|k| asked.options.iter().find(|(kind, _)| kind == k))
                .map(|(_, id)| id.clone())
        };
        let option = match answer {
            // D8: a permission prompt nobody answered is allowed.
            Answer::Allow | Answer::Expired => pick(["allow_once", "allow_always"]),
            Answer::Deny { .. } | Answer::Text { .. } => pick(["reject_once", "reject_always"]),
        };
        let outcome = match option {
            Some(id) => json!({ "outcome": "selected", "optionId": id }),
            None => json!({ "outcome": "cancelled" }),
        };
        Some(json!({ "jsonrpc": "2.0", "id": asked.rpc_id, "result": { "outcome": outcome } }))
    }

    /// Open permission requests answered "cancelled", as ACP requires when their turn is cancelled.
    pub fn cancel_asked(&mut self, out: &mut Parsed) -> Vec<Value> {
        let mut replies = Vec::new();
        for (question_id, a) in std::mem::take(&mut self.asked) {
            replies.push(json!({ "jsonrpc": "2.0", "id": a.rpc_id,
                "result": { "outcome": { "outcome": "cancelled" } } }));
            out.events
                .push(DriverEvent::QuestionResolved { question_id });
        }
        replies
    }
}

/// A permission request's options as (kind, id): `[("allow_once", "once"), …]`.
fn option_kinds(options: &Value) -> Vec<(String, String)> {
    options
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| {
            Some((
                o["kind"].as_str()?.to_string(),
                o["optionId"].as_str()?.to_string(),
            ))
        })
        .collect()
}

fn id_text(id: &Value) -> String {
    id.as_str().map_or_else(|| id.to_string(), str::to_string)
}

/// "Allow OpenCode to run `echo hi`?", by the kind of tool (ACP `ToolKind`).
fn permission_text(call: &Value, title: &str) -> String {
    let input = &call["rawInput"];
    match call["kind"].as_str().unwrap_or("other") {
        "execute" => {
            let command = input["command"].as_str().unwrap_or(title);
            format!("Allow OpenCode to run `{command}`?")
        }
        "edit" | "delete" | "move" => format!("Allow OpenCode to change {}?", paths(call, title)),
        "fetch" => {
            let url = input["url"].as_str().unwrap_or(title);
            format!("Allow OpenCode to fetch {url}?")
        }
        _ => format!("Allow OpenCode to use {title}?"),
    }
}

fn paths(call: &Value, title: &str) -> String {
    let found: Vec<&str> = call["locations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|l| l["path"].as_str())
        .chain(call["rawInput"]["filePath"].as_str())
        .collect();
    if found.is_empty() {
        title.to_string()
    } else {
        found.join(", ")
    }
}

/// `provider` from OpenCode's `provider/model`.
#[must_use]
pub fn provider_of(model: &str) -> Option<String> {
    model.split_once('/').map(|(p, _)| p.to_string())
}

/// The model OpenCode says the session uses, from a `configOptions` list.
#[must_use]
pub fn current_model(config_options: &Value) -> Option<String> {
    config_options
        .as_array()?
        .iter()
        .find(|o| o["id"] == "model")?["currentValue"]
        .as_str()
        .map(str::to_string)
}
