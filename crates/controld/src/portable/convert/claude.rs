//! Claude Code session files (`~/.claude/projects/<project>/<session>.jsonl`): one line per message
//! content block, sharing `message.id`; prompts, tool results and bookkeeping lines in between.
use super::{Ev, Read, Step, Thread, Upload};
use crate::error::ApiError;
use model::cap::MAX_TEXT;
use model::{EventBody, Provenance, TurnStatus};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Lines that only keep Claude Code's own books: nothing to show.
const BOOKKEEPING: &[&str] = &[
    "system",
    "summary",
    "queue-operation",
    "attachment",
    "last-prompt",
    "atis-latch",
    "file-history-snapshot",
];

pub(super) fn read(files: &[Upload]) -> Result<Read, ApiError> {
    let (subs, mains): (Vec<&Upload>, Vec<&Upload>) = files
        .iter()
        .filter(|f| f.name.ends_with(".jsonl"))
        .partition(|f| is_subagent_file(f));
    let main = one_main(&mains)?;
    let mut readers = read_main(main);
    readers.extend(subs.into_iter().map(read_sub));
    Ok(super::claude_links::link(readers, &metas(files)))
}

/// A subagent's own file: `agent-<id>.jsonl`, or lines marked as a sidechain with their agent's id.
fn is_subagent_file(f: &Upload) -> bool {
    let first = f
        .text
        .lines()
        .find(|l| !l.trim().is_empty())
        .and_then(|l| serde_json::from_str::<Value>(l).ok());
    f.name.starts_with("agent-")
        || first.is_some_and(|v| v["isSidechain"] == true && v["agentId"].is_string())
}

fn one_main<'a>(mains: &[&'a Upload]) -> Result<&'a Upload, ApiError> {
    match mains {
        [one] => Ok(one),
        [] => Err(super::invalid(
            "Choose the session's own .jsonl file too, not only its subagents' files",
            1,
            "no main session file",
        )),
        many => Err(super::invalid(
            &format!(
                "These files are from {} different sessions; import them one at a time",
                many.len()
            ),
            1,
            &many
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )),
    }
}

/// The main file: its own lines, and (older sessions) the sidechain lines of its subagents.
fn read_main(f: &Upload) -> Vec<Reader> {
    let mut main = Reader::new("main");
    let mut side: Vec<Reader> = Vec::new();
    let matching_text = f.text.lines().filter(|l| !l.trim().is_empty());
    for line in matching_text {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            main.push(main.prev, unrecognized(Value::String(line.to_string())));
            continue;
        };
        if v["isSidechain"] != true {
            main.line(&v);
            continue;
        }
        if side.is_empty() || (v["parentUuid"].is_null() && v["type"] == "user") {
            side.push(Reader::new(&format!("sidechain-{}", side.len() + 1)));
        }
        side.last_mut().expect("just pushed").line(&v);
    }
    std::iter::once(main).chain(side).collect()
}

fn read_sub(f: &Upload) -> Reader {
    let stem = f.name.trim_end_matches(".jsonl").to_string();
    let lines: Vec<&str> = f.text.lines().filter(|l| !l.trim().is_empty()).collect();
    let id = lines
        .first()
        .and_then(|l| serde_json::from_str::<Value>(l).ok())
        .and_then(|v| v["agentId"].as_str().map(str::to_string));
    let mut r = Reader::new(&id.unwrap_or_else(|| stem.clone()));
    r.stem = Some(stem);
    for line in lines {
        match serde_json::from_str::<Value>(line) {
            Ok(v) => r.line(&v),
            Err(_) => r.push(r.prev, unrecognized(Value::String(line.to_string()))),
        }
    }
    r
}

/// `agent-<id>.meta.json` files, by their stem (`agent-<id>`).
fn metas(files: &[Upload]) -> HashMap<String, Value> {
    files
        .iter()
        .filter(|f| f.name.ends_with(".meta.json"))
        .filter_map(|f| {
            Some((
                f.name.trim_end_matches(".meta.json").to_string(),
                serde_json::from_str(&f.text).ok()?,
            ))
        })
        .collect()
}

fn body(b: EventBody) -> Ev {
    Ev::Body(b, Provenance::ProtocolObserved)
}

fn unrecognized(raw: Value) -> Ev {
    let text = raw.to_string();
    body(EventBody::Unrecognized {
        raw: if text.len() > MAX_TEXT {
            Value::String(model::cap::truncate(text, MAX_TEXT))
        } else {
            raw
        },
    })
}

/// One agent's file, read line by line.
pub(super) struct Reader {
    pub(super) t: Thread,
    pub(super) prev: i64,
    turn: Option<String>,
    turns: u32,
    /// Its file's stem (`agent-<id>`), to find its `.meta.json`.
    pub(super) stem: Option<String>,
    /// The subagents it started (`Agent` / `Task` calls), in order.
    pub(super) calls: Vec<Call>,
    /// `message.id`s whose usage was already counted (a message spans several lines).
    seen: HashSet<String>,
}

impl Reader {
    pub(super) fn new(key: &str) -> Self {
        Self {
            t: Thread {
                key: key.into(),
                runtime: "claude-stream",
                ..Thread::default()
            },
            prev: 0,
            turn: None,
            turns: 0,
            stem: None,
            calls: Vec::new(),
            seen: HashSet::new(),
        }
    }

    pub(super) fn push(&mut self, at_ms: i64, ev: Ev) {
        self.t.steps.push(Step { at_ms, ev });
    }

    pub(super) fn line(&mut self, v: &Value) {
        let at = crate::portable::convert::ts_ms(v, self.prev);
        self.prev = at;
        if self.t.session_id.is_none() {
            self.t.session_id = v["sessionId"].as_str().map(str::to_string);
        }
        if self.t.cwd.is_none() {
            self.t.cwd = v["cwd"].as_str().map(str::to_string);
        }
        match v["type"].as_str().unwrap_or("") {
            "user" => self.user(v, at),
            "assistant" => self.assistant(v, at),
            "ai-title" => {
                self.t.name = v["aiTitle"]
                    .as_str()
                    .map(str::to_string)
                    .or(self.t.name.take())
            }
            t if BOOKKEEPING.contains(&t) => {}
            _ => self.push(at, unrecognized(v.clone())),
        }
    }

    pub(super) fn finish(mut self) -> Thread {
        self.end_turn(self.prev);
        self.t
    }

    fn user(&mut self, v: &Value, at: i64) {
        if v["isCompactSummary"] == true {
            self.push(
                at,
                body(EventBody::AgentText {
                    turn_id: None,
                    text: "[context compacted]".into(),
                }),
            );
            return;
        }
        if let Some(text) = prompt_text(v) {
            self.end_turn(at);
            self.push(at, Ev::Prompt { text, from: None });
            return;
        }
        let matching_items = v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["type"] == "tool_result");
        for b in matching_items {
            let call_id = b["tool_use_id"].as_str().unwrap_or_default().to_string();
            let output = model::cap::truncate(result_text(&b["content"]), MAX_TEXT);
            let is_error = b["is_error"].as_bool().unwrap_or(false);
            self.push(
                at,
                body(EventBody::ToolResulted {
                    turn_id: self.turn.clone(),
                    call_id,
                    output,
                    is_error,
                }),
            );
        }
    }

    fn assistant(&mut self, v: &Value, at: i64) {
        let turn = self.ensure_turn(at);
        let m = &v["message"];
        if let Some(model) = m["model"].as_str().filter(|m| !m.starts_with('<')) {
            self.t.models.push(model.to_string());
        }
        let flattened_items = m["content"].as_array().into_iter().flatten();
        for b in flattened_items {
            if let Some(call) = Call::of(b) {
                self.calls.push(call);
            }
            if let Some(ev) = block(b, &turn) {
                self.push(at, ev);
            }
        }
        let first = m["id"]
            .as_str()
            .is_some_and(|id| self.seen.insert(id.to_string()));
        if first && m["usage"].is_object() {
            self.push(at, body(usage(&m["usage"], turn)));
        }
    }

    fn ensure_turn(&mut self, at: i64) -> String {
        if let Some(t) = &self.turn {
            return t.clone();
        }
        self.turns += 1;
        let t = format!("turn-{}", self.turns);
        self.turn = Some(t.clone());
        self.push(at, body(EventBody::TurnStarted { turn_id: t.clone() }));
        t
    }

    fn end_turn(&mut self, at: i64) {
        if let Some(turn_id) = self.turn.take() {
            self.push(
                at,
                body(EventBody::TurnEnded {
                    turn_id,
                    status: TurnStatus::Completed,
                }),
            );
        }
    }
}

/// A subagent an agent started: its `Agent` (or older `Task`) tool call.
#[derive(Clone, Debug)]
pub(super) struct Call {
    pub(super) id: String,
    pub(super) description: Option<String>,
    pub(super) agent_type: Option<String>,
    pub(super) prompt: Option<String>,
}

impl Call {
    fn of(b: &Value) -> Option<Self> {
        if b["type"] != "tool_use" || !matches!(b["name"].as_str(), Some("Agent" | "Task")) {
            return None;
        }
        let s = |k: &str| b["input"][k].as_str().map(str::to_string);
        Some(Self {
            id: b["id"].as_str()?.to_string(),
            description: s("description"),
            agent_type: s("subagent_type"),
            prompt: s("prompt"),
        })
    }
}

/// One content block of an agent's message, as an event (thinking is not shown, like the live view).
fn block(b: &Value, turn: &str) -> Option<Ev> {
    let turn_id = Some(turn.to_string());
    match b["type"].as_str().unwrap_or("") {
        "text" => Some(body(EventBody::AgentText {
            turn_id,
            text: model::cap::truncate(
                b["text"].as_str().unwrap_or_default().to_string(),
                MAX_TEXT,
            ),
        })),
        "tool_use" => Some(body(EventBody::ToolCalled {
            turn_id,
            call_id: b["id"].as_str().unwrap_or_default().to_string(),
            name: b["name"].as_str().unwrap_or_default().to_string(),
            input: model::cap::cap_input(b["input"].clone()),
            parent_call_id: None,
        })),
        "thinking" | "redacted_thinking" => None,
        _ => Some(unrecognized(b.clone())),
    }
}

fn usage(u: &Value, turn: String) -> EventBody {
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    EventBody::UsageReported {
        in_turn: false,
        turn_id: Some(turn),
        input_tokens: n("input_tokens"),
        output_tokens: n("output_tokens"),
        cache_read_tokens: n("cache_read_input_tokens"),
        cache_write_tokens: n("cache_creation_input_tokens"),
        cost: None,
    }
}

/// Text Claude Code itself puts in user lines: slash commands, shell mode, notices, interruptions.
const OWN: &[&str] = &[
    "<command-",
    "<local-command-",
    "[Request interrupted",
    "<bash-input>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<task-notification>",
    "<system-reminder>",
];

/// One piece of a prompt: its words, "[image]" for a picture, nothing for Claude Code's own text.
fn piece(b: &Value) -> Option<String> {
    match b["type"].as_str()? {
        "text" => b["text"]
            .as_str()
            .map(str::trim)
            .filter(|t| !t.is_empty() && !OWN.iter().any(|p| t.starts_with(p)))
            .map(str::to_string),
        "image" => Some("[image]".into()),
        _ => None,
    }
}

/// The person's words, when this user line is a prompt: not a tool result, not Claude Code's own
/// lines (`isMeta`, transcript-only lines, slash commands, shell mode, notices, interruptions).
pub(super) fn prompt_text(v: &Value) -> Option<String> {
    if v["isMeta"] == true || v["isVisibleInTranscriptOnly"] == true {
        return None;
    }
    let text = match &v["message"]["content"] {
        Value::String(s) => piece(&serde_json::json!({ "type": "text", "text": s }))?,
        Value::Array(blocks) if !blocks.iter().any(|b| b["type"] == "tool_result") => blocks
            .iter()
            .filter_map(piece)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    // A picture alone, with no words, is not a prompt either.
    (!text.is_empty() && text != "[image]").then(|| model::cap::truncate(text, MAX_TEXT))
}

fn result_text(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
