use super::{
    Answer, DriverEvent, EXPIRED_ANSWER, HashMap, HashSet, Map, Totals, Value, VecDeque, json,
};

/// Notifications that carry nothing Sculptured reads (M0 §Codex facts and the M2 refresh, every recording).
/// `warning` is not here: it is runtime text and is kept as `Unrecognized` evidence (review fix round 1).
pub(super) const IGNORED: [&str; 10] = [
    "account/updated",
    "deprecationNotice",
    "hook/completed",
    "hook/started",
    "remoteControl/status/changed",
    "thread/goal/cleared",
    "thread/started",
    "thread/status/changed",
    "turn/diff/updated",
    "item/commandExecution/outputDelta",
];

/// A server request waiting for the operator's answer.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Pending {
    /// `requestUserInput`: the first question's id gets the answer; the others an empty list.
    Question {
        rpc_id: Value,
        key: String,
        others: Vec<String>,
    },
    Permission {
        rpc_id: Value,
    },
}

/// What one stdout line produced.
#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub events: Vec<DriverEvent>,
    /// JSON-RPC lines to write back now.
    pub replies: Vec<Value>,
    /// A turn completed: the driver may start the next queued message.
    pub turn_ended: bool,
    /// The message whose `turn/start` was refused (it never ran).
    pub refused_start: Option<String>,
}

#[derive(Default)]
pub struct CodexParser {
    pub(super) thread: Option<String>,
    /// The model the thread runs (from the `thread/start|resume|fork` answer), for pricing its tokens.
    pub(super) model: Option<String>,
    /// `turn/start` requests sent and not yet started: (request id, message id), oldest first.
    pub(super) starts: VecDeque<(u64, String)>,
    /// (thread, turn) pairs this parser started; usage for anything else is a replay.
    pub(super) started: HashSet<(String, String)>,
    pub(super) current: Option<String>,
    /// Tool calls started and not finished: (item id, turn id).
    pub(super) open_calls: Vec<(String, String)>,
    /// Tool items by id, for the permission prompts that name them.
    pub(super) items: HashMap<String, Value>,
    pub(super) seen: HashMap<String, Totals>,
    pub(super) pending: HashMap<String, Pending>,
    /// Request id (as text) → question id, for `serverRequest/resolved`.
    pub(super) by_rpc: HashMap<String, String>,
    /// Per process: server request ids restart at 0 in every app-server process.
    pub(super) nonce: String,
    /// Unknown items already kept as evidence when they started: item id → turn id. Dropped when
    /// they complete or their turn ends.
    pub(super) unknown_items: HashMap<String, String>,
}

pub(super) fn rpc_text(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub(super) fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

impl CodexParser {
    pub fn set_thread(&mut self, id: &str) {
        self.thread = Some(id.to_string());
    }

    #[must_use]
    pub fn thread_id(&self) -> Option<&str> {
        self.thread.as_deref()
    }

    pub fn on_turn_start_sent(&mut self, rpc_id: u64, message_id: String) {
        self.starts.push_back((rpc_id, message_id));
    }

    /// A turn runs or is being started: the next message must wait.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.current.is_some() || !self.starts.is_empty()
    }

    #[must_use]
    pub fn current_turn(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn parse_line(&mut self, line: &str) -> Parsed {
        let mut out = Parsed::default();
        let line = line.trim();
        if line.is_empty() {
            return out;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            out.events.push(DriverEvent::Unrecognized {
                raw: Value::String(line.to_string()),
            });
            return out;
        };
        let method = v.get("method").and_then(Value::as_str).map(str::to_string);
        match (method, v.get("id").cloned()) {
            (Some(m), Some(id)) => self.server_request(&m, id, &v, &mut out),
            (Some(m), None) => self.notification(&m, &v, &mut out),
            (None, Some(id)) => self.response(&id, &v, &mut out),
            (None, None) => out.events.push(DriverEvent::Unrecognized { raw: v }),
        }
        out
    }

    /// Removes the pending `turn/start` with request id `rpc_id`; returns its message.
    pub(super) fn take_start(&mut self, rpc_id: u64) -> Option<String> {
        let i = self.starts.iter().position(|(r, _)| *r == rpc_id)?;
        self.starts.remove(i).map(|(_, m)| m)
    }

    pub(super) fn start_turn(
        &mut self,
        rpc_id: Option<u64>,
        thread: Option<String>,
        turn: String,
        out: &mut Parsed,
    ) {
        let thread = thread.or_else(|| self.thread.clone()).unwrap_or_default();
        if self.started.contains(&(thread.clone(), turn.clone())) {
            // Otherwise `turn/started` after the response (the usual order) is the same turn: nothing new.
            if let Some(id) = rpc_id {
                self.start_joined_known_turn(id, turn, out);
            }
            return;
        }
        let message = match rpc_id {
            Some(id) => self.take_start(id),
            None => self.starts.pop_front().map(|(_, m)| m),
        };
        self.started.insert((thread, turn.clone()));
        self.current = Some(turn.clone());
        out.events.push(DriverEvent::TurnStarted {
            turn_id: turn.clone(),
        });
        if let Some(message_id) = message {
            out.events.push(DriverEvent::MessageConsumed {
                message_id,
                turn_id: turn,
            });
        }
    }

    /// A response to one of our `turn/start`s that names a turn already started: Codex 0.160.1
    /// steers a mid-turn `turn/start` into the active turn (no error, no new turn, no
    /// `turn/started`; codex-m2-interrupt@6654-6656). The message joined that turn. It must not
    /// stay pending either way, or `busy()` would never clear.
    pub(super) fn start_joined_known_turn(&mut self, rpc_id: u64, turn: String, out: &mut Parsed) {
        let Some(message_id) = self.take_start(rpc_id) else {
            return;
        };
        if self.current.as_deref() == Some(turn.as_str()) {
            out.events.push(DriverEvent::MessageConsumed {
                message_id,
                turn_id: turn,
            });
        } else {
            // A turn that already ended (never observed): nothing ran the message, so it is
            // handed back like a refused start, and the odd answer is kept as evidence.
            out.refused_start = Some(message_id);
            out.events.push(DriverEvent::Unrecognized {
                raw: json!({ "turnStartAnsweredWithEndedTurn": turn }),
            });
        }
    }

    /// Keeps the model a `thread/start|resume|fork` answer names, which prices the thread's tokens.
    /// The driver's handshake reads that answer itself, so it hands it here.
    pub(crate) fn note_model(&mut self, answer: &Value) {
        if let Some(m) = answer["result"]["model"].as_str() {
            self.model = Some(m.to_string());
        }
    }

    pub(super) fn response(&mut self, id: &Value, v: &Value, out: &mut Parsed) {
        if let Some(t) = v["result"]["thread"]["id"].as_str() {
            self.thread = Some(t.to_string());
        }
        self.note_model(v);
        let ours = id
            .as_u64()
            .filter(|n| self.starts.iter().any(|(r, _)| r == n));
        match (ours, v.get("error")) {
            (Some(n), Some(_)) => {
                out.refused_start = self.take_start(n);
                out.events
                    .push(DriverEvent::Unrecognized { raw: v.clone() });
            }
            (Some(n), None) => match s(&v["result"]["turn"]["id"]) {
                Some(turn) => self.start_turn(Some(n), None, turn, out),
                None => out
                    .events
                    .push(DriverEvent::Unrecognized { raw: v.clone() }),
            },
            (None, Some(_)) => out
                .events
                .push(DriverEvent::Unrecognized { raw: v.clone() }),
            // Answers to initialize, thread/*, turn/interrupt: nothing to show.
            (None, None) => {}
        }
    }

    pub(super) fn notification(&mut self, method: &str, v: &Value, out: &mut Parsed) {
        let p = &v["params"];
        let turn = s(&p["turnId"])
            .or_else(|| self.current.clone())
            .unwrap_or_default();
        match method {
            "turn/started" => {
                if let Some(t) = s(&p["turn"]["id"]) {
                    self.start_turn(None, s(&p["threadId"]), t, out);
                }
            }
            "item/started" => self.item_started(&p["item"], turn, out),
            "account/rateLimits/updated" => out
                .events
                .extend(model::PlanUsage::from_codex(&p["rateLimits"]).map(DriverEvent::PlanUsage)),
            "item/completed" => self.item_completed(&p["item"], turn, out),
            "thread/tokenUsage/updated" => self.token_usage(p, turn, out),
            "turn/completed" => self.turn_completed(p, turn, out),
            "serverRequest/resolved" => {
                if let Some(q) = self.by_rpc.remove(&rpc_text(&p["requestId"])) {
                    self.pending.remove(&q);
                    out.events
                        .push(DriverEvent::QuestionResolved { question_id: q });
                }
            }
            "mcpServer/startupStatus/updated" => {
                if p["name"] == crate::MCP_SERVER && p["status"] == "failed" {
                    out.events
                        .push(DriverEvent::Unrecognized { raw: p.clone() });
                }
            }
            m if IGNORED.contains(&m) || m.ends_with("/delta") || m.ends_with("Delta") => {}
            _ => out
                .events
                .push(DriverEvent::Unrecognized { raw: v.clone() }),
        }
    }

    /// `codex-<thread>-<nonce>-<request id>`: a resumed session keeps its thread id and the new
    /// process numbers its requests from 0 again, so the nonce keeps the ids of two processes apart.
    pub(super) fn question_id(&mut self, id: &Value) -> String {
        if self.nonce.is_empty() {
            self.nonce = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        }
        format!(
            "codex-{}-{}-{}",
            self.thread.as_deref().unwrap_or("unknown"),
            self.nonce,
            rpc_text(id)
        )
    }

    /// The JSON-RPC response that answers question `question_id`; None when it is not pending.
    /// The request id stays mapped until Codex's `serverRequest/resolved`, which then yields
    /// `QuestionResolved` (codex-user-input@4821, codex-m2-approval@4647).
    pub fn answer_reply(&mut self, question_id: &str, answer: &Answer) -> Option<Value> {
        Some(match self.pending.remove(question_id)? {
            Pending::Question {
                rpc_id,
                key,
                others,
            } => question_reply(rpc_id, key, others, answer),
            Pending::Permission { rpc_id } => {
                let decision = match answer {
                    Answer::Allow | Answer::Expired => "accept",
                    Answer::Deny { .. } | Answer::Text { .. } => "decline",
                };
                json!({ "id": rpc_id, "jsonrpc": "2.0", "result": { "decision": decision } })
            }
        })
    }
}

pub(super) fn question_reply(
    rpc_id: Value,
    key: String,
    others: Vec<String>,
    answer: &Answer,
) -> Value {
    let text = match answer {
        Answer::Text { text } => text.clone(),
        Answer::Expired => EXPIRED_ANSWER.to_string(),
        Answer::Allow => "allow".to_string(),
        Answer::Deny { .. } => "deny".to_string(),
    };
    let mut answers = Map::new();
    answers.insert(key, json!({ "answers": [text] }));
    for o in others {
        answers.insert(o, json!({ "answers": [] }));
    }
    json!({ "id": rpc_id, "jsonrpc": "2.0", "result": { "answers": answers } })
}
