//! Codex rollouts (`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`): `{timestamp, type, payload}` lines.
//! One file is one thread; a sub-agent thread names its parent thread in `session_meta.source`.
use super::{Ev, Read, Step, Thread, Upload};
use crate::error::ApiError;
use model::cap::MAX_TEXT;
use model::{EventBody, Provenance, ThinkingForm, TurnStatus};
use serde_json::Value;

pub(super) fn read(files: &[Upload]) -> Result<Read, ApiError> {
    let read: Vec<(Thread, Option<String>)> = files
        .iter()
        .filter(|f| f.name.ends_with(".jsonl"))
        .map(read_file)
        .collect();
    let roots = read.iter().filter(|(_, parent)| parent.is_none()).count();
    if roots > 1 {
        return Err(super::invalid(
            &format!("These files are from {roots} different sessions; import them one at a time"),
            1,
            "several main threads",
        ));
    }
    let ids: Vec<Option<String>> = read.iter().map(|(t, _)| t.session_id.clone()).collect();
    let mut out = Read::default();
    for (mut t, parent) in read {
        match parent {
            Some(p) if ids.contains(&Some(p.clone())) => t.parent = Some(p),
            Some(p) => out
                .missing
                .push(format!("{}: its parent thread {p} was not included", t.key)),
            None => {}
        }
        out.threads.push(t);
    }
    Ok(out)
}

fn read_file(f: &Upload) -> (Thread, Option<String>) {
    let mut r = Reader {
        t: Thread {
            runtime: "codex-app-server",
            ..Thread::default()
        },
        ..Reader::default()
    };
    let matching_text = f.text.lines().filter(|l| !l.trim().is_empty());
    for line in matching_text {
        match serde_json::from_str::<Value>(line) {
            Ok(v) => r.line(&v),
            Err(_) => r.push(r.prev, unrecognized(Value::String(line.to_string()))),
        }
    }
    r.end(r.prev, TurnStatus::Completed);
    if r.t.key.is_empty() {
        r.t.key = f.name.trim_end_matches(".jsonl").to_string();
    }
    (r.t, r.parent)
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

#[derive(Default)]
struct Reader {
    t: Thread,
    prev: i64,
    turn: Option<String>,
    turns: u32,
    /// The last cumulative `total_token_usage`: input, cached input, output.
    last: [u64; 3],
    parent: Option<String>,
}

impl Reader {
    fn push(&mut self, at_ms: i64, ev: Ev) {
        self.t.steps.push(Step { at_ms, ev });
    }

    fn line(&mut self, v: &Value) {
        let at = crate::portable::convert::ts_ms(v, self.prev);
        self.prev = at;
        let p = &v["payload"];
        match (
            v["type"].as_str().unwrap_or(""),
            p["type"].as_str().unwrap_or(""),
        ) {
            ("session_meta", _) => self.meta(p),
            ("turn_context", _) => self
                .t
                .models
                .extend(p["model"].as_str().map(str::to_string)),
            ("response_item", "message") => self.message(p, at),
            ("response_item", "reasoning") => self.reasoning(p, at),
            ("response_item", "function_call" | "custom_tool_call") => self.call(p, at),
            ("response_item", "function_call_output" | "custom_tool_call_output") => {
                self.output(p, at)
            }
            ("event_msg", "task_started") => drop(self.ensure_turn(at)),
            ("event_msg", "task_complete") => self.end(at, TurnStatus::Completed),
            ("event_msg", "turn_aborted") => self.end(at, TurnStatus::Interrupted),
            ("event_msg", "token_count") => self.tokens(&p["info"]["total_token_usage"], at),
            ("compacted", _) => self.push(
                at,
                body(EventBody::AgentText {
                    turn_id: self.turn.clone(),
                    text: "[context compacted]".into(),
                }),
            ),
            // Bookkeeping, and event_msg copies of the response items above.
            ("event_msg" | "world_state" | "token_usage_record", _) => {}
            _ => self.push(at, unrecognized(v.clone())),
        }
    }

    fn meta(&mut self, p: &Value) {
        // A sub-agent's rollout repeats its parent's meta after its own: the first one is this thread.
        if !self.t.key.is_empty() {
            return;
        }
        self.t.key = p["id"].as_str().unwrap_or_default().to_string();
        self.t.session_id = p["id"].as_str().map(str::to_string);
        self.t.cwd = p["cwd"].as_str().map(str::to_string);
        self.parent = p["source"]
            .is_object()
            .then(|| find_str(&p["source"], "parent_thread_id"))
            .flatten();
    }

    fn message(&mut self, p: &Value, at: i64) {
        let text = |kind: &str| {
            p["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| c["type"] == kind)
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        match p["role"].as_str() {
            Some("user") => {
                let t = text("input_text");
                if !t.trim().is_empty()
                    && !t.trim_start().starts_with('<')
                    && !t.starts_with("# AGENTS.md")
                {
                    self.end(at, TurnStatus::Completed);
                    self.push(
                        at,
                        Ev::Prompt {
                            text: t.trim().to_string(),
                            from: None,
                        },
                    );
                }
            }
            Some("assistant") => {
                let turn_id = Some(self.ensure_turn(at));
                self.push(
                    at,
                    body(EventBody::AgentText {
                        turn_id,
                        text: model::cap::truncate(text("output_text"), MAX_TEXT),
                    }),
                );
            }
            _ => {}
        }
    }

    fn reasoning(&mut self, p: &Value, at: i64) {
        let text: Vec<&str> = p["summary"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["text"].as_str())
            .collect();
        let (form, text) = if text.is_empty() {
            (ThinkingForm::Hidden, String::new())
        } else {
            (ThinkingForm::Summary, text.join("\n"))
        };
        let turn_id = Some(self.ensure_turn(at));
        let item_id = p["id"].as_str().map(str::to_string);
        let b = EventBody::ThinkingRecorded {
            turn_id,
            item_id,
            parent_call_id: None,
            form,
            text: model::cap::truncate(text, MAX_TEXT),
            estimated_tokens: None,
        };
        self.push(at, Ev::Body(b, Provenance::AgentReported));
    }

    fn call(&mut self, p: &Value, at: i64) {
        let turn_id = Some(self.ensure_turn(at));
        let raw = p
            .get("arguments")
            .or_else(|| p.get("input"))
            .cloned()
            .unwrap_or(Value::Null);
        let input = raw
            .as_str()
            .and_then(|s| {
                serde_json::from_str::<Value>(s)
                    .ok()
                    .filter(Value::is_object)
            })
            .unwrap_or(raw);
        let (call_id, name) = (
            p["call_id"].as_str().unwrap_or_default().to_string(),
            p["name"].as_str().unwrap_or_default().to_string(),
        );
        self.push(
            at,
            body(EventBody::ToolCalled {
                turn_id,
                call_id,
                name,
                input: model::cap::cap_input(input),
                parent_call_id: None,
            }),
        );
    }

    fn output(&mut self, p: &Value, at: i64) {
        let out = &p["output"];
        let text = out
            .as_str()
            .map(str::to_string)
            .or_else(|| out["output"].as_str().map(str::to_string))
            .unwrap_or_else(|| out.to_string());
        let call_id = p["call_id"].as_str().unwrap_or_default().to_string();
        self.push(
            at,
            body(EventBody::ToolResulted {
                turn_id: self.turn.clone(),
                call_id,
                output: model::cap::truncate(text, MAX_TEXT),
                is_error: false,
            }),
        );
    }

    /// Codex counts are cumulative over the session: report what this step added.
    fn tokens(&mut self, total: &Value, at: i64) {
        let n = |k: &str| total[k].as_u64().unwrap_or(0);
        let now = [
            n("input_tokens"),
            n("cached_input_tokens"),
            n("output_tokens"),
        ];
        if !total.is_object() || now == self.last {
            return;
        }
        let d: Vec<u64> = now
            .iter()
            .zip(self.last)
            .map(|(a, b)| a.saturating_sub(b))
            .collect();
        self.last = now;
        let b = EventBody::UsageReported {
            in_turn: false,
            turn_id: self.turn.clone(),
            input_tokens: d[0].saturating_sub(d[1]),
            output_tokens: d[2],
            cache_read_tokens: d[1],
            cache_write_tokens: 0,
            cost: None,
        };
        self.push(at, body(b));
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

    fn end(&mut self, at: i64, status: TurnStatus) {
        if let Some(turn_id) = self.turn.take() {
            self.push(at, body(EventBody::TurnEnded { turn_id, status }));
        }
    }
}

/// The first string field named `key` anywhere inside `v`.
fn find_str(v: &Value, key: &str) -> Option<String> {
    match v {
        Value::Object(m) => m
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| m.values().find_map(|x| find_str(x, key))),
        Value::Array(a) => a.iter().find_map(|x| find_str(x, key)),
        _ => None,
    }
}
