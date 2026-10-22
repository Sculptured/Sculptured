//! A team run's events as OpenTelemetry spans: run → agent session → turn → tool, with tokens, cost
//! and the observed model on each turn (`gen_ai.*`, and `lmnr.*` for Laminar). Texts only when the
//! team chose to send prompts and outputs. Pure: the exporter feeds events in and sends what ends.
use model::{EventBody, EventRecord, Subject, TurnStatus};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Texts sent with "Also prompts and outputs" are cut at this many bytes.
pub const TEXT_CAP: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub enum Attr {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

#[derive(Clone, Debug)]
pub struct SpanEvent {
    pub name: String,
    pub at_ms: i64,
    pub attributes: Vec<(String, Attr)>,
}

#[derive(Clone, Debug)]
pub struct Span {
    pub trace_id: String,
    pub span_id: String,
    pub parent: Option<String>,
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub attributes: Vec<(String, Attr)>,
    pub events: Vec<SpanEvent>,
    pub error: bool,
}

/// What a run is: the team, where its events begin, the agents' names, and whether texts go too.
pub struct Ctx {
    pub team: String,
    pub team_name: String,
    pub start_seq: i64,
    /// Seat id → the agent's name in the design.
    pub names: HashMap<String, String>,
    pub full: bool,
}

/// A span that has begun and not ended.
struct Open {
    span: Span,
    seat: String,
    input: String,
    output: String,
    tokens: [u64; 4],
    cost: Option<f64>,
}

/// Follows a run's events and gives back each span as it ends.
pub struct Builder {
    ctx: Ctx,
    trace_id: String,
    run_id: String,
    run_start: Option<i64>,
    run_events: Vec<SpanEvent>,
    /// By key: `agent:<seat>#<session>`, `turn:<seat>#<session>:<turn>`, `tool:<seat>#<session>:<call>`
    /// (turn ids start again in each session).
    open: HashMap<String, Open>,
    /// A seat's current turn, its driver and its observed model.
    turn_of: HashMap<String, String>,
    driver_of: HashMap<String, String>,
    model_of: HashMap<String, String>,
    queued: HashMap<String, String>,
    session_of: HashMap<String, u32>,
}

fn id(seed: &str, bytes: usize) -> String {
    hex::encode(&Sha256::digest(seed.as_bytes())[..bytes])
}

fn cap(s: &str) -> String {
    if s.len() <= TEXT_CAP {
        return s.to_string();
    }
    let mut end = TEXT_CAP;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn system(driver: &str) -> &'static str {
    if driver.contains("codex") {
        "openai"
    } else {
        "anthropic"
    }
}

impl Builder {
    #[must_use]
    pub fn new(ctx: Ctx) -> Builder {
        let trace_id = id(&format!("run:{}:{}", ctx.team, ctx.start_seq), 16);
        let run_id = id(&format!("run-span:{}:{}", ctx.team, ctx.start_seq), 8);
        Builder {
            ctx,
            trace_id,
            run_id,
            run_start: None,
            run_events: vec![],
            open: HashMap::new(),
            turn_of: HashMap::new(),
            driver_of: HashMap::new(),
            model_of: HashMap::new(),
            queued: HashMap::new(),
            session_of: HashMap::new(),
        }
    }

    fn begin(
        &mut self,
        key: String,
        seat: &str,
        name: String,
        parent: Option<String>,
        at_ms: i64,
        attributes: Vec<(String, Attr)>,
    ) {
        let span = Span {
            trace_id: self.trace_id.clone(),
            span_id: id(&format!("{}:{key}", self.trace_id), 8),
            parent,
            name,
            start_ms: at_ms,
            end_ms: at_ms,
            attributes,
            events: vec![],
            error: false,
        };
        self.open.insert(
            key,
            Open {
                span,
                seat: seat.to_string(),
                input: String::new(),
                output: String::new(),
                tokens: [0; 4],
                cost: None,
            },
        );
    }

    fn span_id_of(&self, key: &str) -> Option<String> {
        self.open.get(key).map(|o| o.span.span_id.clone())
    }

    /// The spans this event ends (often none).
    pub fn feed(&mut self, e: &EventRecord) -> Vec<Span> {
        self.run_start.get_or_insert(e.at_ms);
        match &e.subject {
            Subject::Seat(seat) => self.seat_event(seat, e),
            Subject::Rig(_) => {
                self.rig_event(e);
                vec![]
            }
            Subject::Host(_) => vec![],
        }
    }

    fn rig_event(&mut self, e: &EventRecord) {
        let (name, attrs) = match &e.body {
            EventBody::BudgetStateChanged { state, reason } => (
                format!("budget {state}"),
                vec![("reason".to_string(), Attr::Str(reason.clone()))],
            ),
            EventBody::CheckRan { .. } => return self.check(e, None),
            _ => return,
        };
        self.run_events.push(SpanEvent {
            name,
            at_ms: e.at_ms,
            attributes: attrs,
        });
    }

    /// A check on the run (the team's or an agent's).
    fn check(&mut self, e: &EventRecord, seat: Option<&str>) {
        let EventBody::CheckRan {
            command, passed, ..
        } = &e.body
        else {
            return;
        };
        let mut attributes = vec![("command".to_string(), Attr::Str(command.clone()))];
        attributes.extend(seat.map(|s| ("sculptured.seat".to_string(), Attr::Str(s.into()))));
        self.run_events.push(SpanEvent {
            name: if *passed {
                "check passed"
            } else {
                "check failed"
            }
            .into(),
            at_ms: e.at_ms,
            attributes,
        });
    }

    fn seat_event(&mut self, seat: &str, e: &EventRecord) -> Vec<Span> {
        if let EventBody::TenureStarted { tenure, .. } = &e.body {
            self.session_of.insert(seat.to_string(), *tenure);
        }
        let sk = format!("{seat}#{}", self.session_of.get(seat).copied().unwrap_or(0));
        match &e.body {
            EventBody::TenureStarted { tenure, driver, .. } => {
                self.driver_of.insert(seat.to_string(), driver.clone());
                let name = self
                    .ctx
                    .names
                    .get(seat)
                    .or_else(|| self.ctx.names.get(crate::seat_ids::member_of(seat)))
                    .cloned()
                    .unwrap_or_else(|| seat.to_string());
                let attrs = vec![
                    ("sculptured.agent".into(), Attr::Str(name.clone())),
                    ("sculptured.seat".into(), Attr::Str(seat.into())),
                    ("sculptured.driver".into(), Attr::Str(driver.clone())),
                    ("sculptured.session".into(), Attr::Int(i64::from(*tenure))),
                    ("sculptured.computer".into(), Attr::Str(e.host.clone())),
                ];
                self.begin(
                    format!("agent:{sk}"),
                    seat,
                    format!("agent {name}"),
                    Some(self.run_id.clone()),
                    e.at_ms,
                    attrs,
                );
                vec![]
            }
            EventBody::TenureEnded { exit_code, .. } => self
                .end(
                    &format!("agent:{sk}"),
                    e.at_ms,
                    exit_code.is_some_and(|c| c != 0),
                )
                .into_iter()
                .collect(),
            EventBody::ModelObserved {
                observed: Some(m), ..
            } => {
                self.model_of.insert(seat.to_string(), m.clone());
                vec![]
            }
            EventBody::MessageQueued {
                message_id, text, ..
            } => {
                self.queued.insert(message_id.clone(), text.clone());
                vec![]
            }
            EventBody::TurnStarted { turn_id } => {
                self.turn_of.insert(seat.to_string(), turn_id.clone());
                let parent = self
                    .span_id_of(&format!("agent:{sk}"))
                    .or_else(|| Some(self.run_id.clone()));
                let driver = self.driver_of.get(seat).cloned().unwrap_or_default();
                let mut attrs = vec![
                    ("lmnr.span.type".into(), Attr::Str("LLM".into())),
                    ("gen_ai.system".into(), Attr::Str(system(&driver).into())),
                ];
                if let Some(m) = self.model_of.get(seat) {
                    attrs.push(("gen_ai.request.model".into(), Attr::Str(m.clone())));
                    attrs.push(("gen_ai.response.model".into(), Attr::Str(m.clone())));
                }
                self.begin(
                    format!("turn:{sk}:{turn_id}"),
                    seat,
                    "turn".into(),
                    parent,
                    e.at_ms,
                    attrs,
                );
                vec![]
            }
            EventBody::MessageConsumed {
                message_id,
                turn_id,
            } => {
                let text = self.queued.remove(message_id).unwrap_or_default();
                if let Some(o) = self.open.get_mut(&format!("turn:{sk}:{turn_id}")) {
                    o.input.push_str(&text);
                }
                vec![]
            }
            EventBody::AgentText { turn_id, text } => {
                let key = self.turn_key(seat, &sk, turn_id.as_deref());
                if let Some(o) = key.and_then(|k| self.open.get_mut(&k)) {
                    o.output.push_str(text);
                }
                vec![]
            }
            EventBody::UsageReported {
                turn_id,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                cost,
                ..
            } => {
                let key = self.turn_key(seat, &sk, turn_id.as_deref());
                if let Some(o) = key.and_then(|k| self.open.get_mut(&k)) {
                    let paired_tokens = o.tokens.iter_mut().zip([
                        input_tokens,
                        output_tokens,
                        cache_read_tokens,
                        cache_write_tokens,
                    ]);
                    for (t, n) in paired_tokens {
                        *t += n;
                    }
                    if let Some(c) = cost {
                        o.cost = Some(o.cost.unwrap_or(0.0) + c.usd);
                    }
                }
                vec![]
            }
            EventBody::TurnEnded { turn_id, status } => {
                self.turn_of.remove(seat);
                let word = match status {
                    TurnStatus::Completed => "completed",
                    TurnStatus::Interrupted => "interrupted",
                    TurnStatus::Failed => "failed",
                };
                // Interrupted (stopped by a person or a limit) is not an error; a failure is.
                let ended = self.end(
                    &format!("turn:{sk}:{turn_id}"),
                    e.at_ms,
                    *status == TurnStatus::Failed,
                );
                ended
                    .map(|mut s| {
                        s.attributes
                            .push(("sculptured.turn_status".into(), Attr::Str(word.into())));
                        s
                    })
                    .into_iter()
                    .collect()
            }
            EventBody::ToolCalled {
                turn_id,
                call_id,
                name,
                input,
                parent_call_id,
            } => {
                let parent = parent_call_id
                    .as_ref()
                    .and_then(|p| self.span_id_of(&format!("tool:{sk}:{p}")))
                    .or_else(|| {
                        self.turn_key(seat, &sk, turn_id.as_deref())
                            .and_then(|k| self.span_id_of(&k))
                    })
                    .or_else(|| self.span_id_of(&format!("agent:{sk}")))
                    .or_else(|| Some(self.run_id.clone()));
                let attrs = vec![
                    ("lmnr.span.type".into(), Attr::Str("TOOL".into())),
                    ("gen_ai.tool.name".into(), Attr::Str(name.clone())),
                ];
                self.begin(
                    format!("tool:{sk}:{call_id}"),
                    seat,
                    format!("tool {name}"),
                    parent,
                    e.at_ms,
                    attrs,
                );
                if let Some(o) = self.open.get_mut(&format!("tool:{sk}:{call_id}")) {
                    o.input = input.to_string();
                }
                vec![]
            }
            EventBody::ToolResulted {
                call_id,
                output,
                is_error,
                ..
            } => {
                if let Some(o) = self.open.get_mut(&format!("tool:{sk}:{call_id}")) {
                    o.output = output.clone();
                }
                self.end(&format!("tool:{sk}:{call_id}"), e.at_ms, *is_error)
                    .into_iter()
                    .collect()
            }
            EventBody::QuestionRaised { question_id, .. } => {
                self.run_events.push(SpanEvent {
                    name: "question asked".into(),
                    at_ms: e.at_ms,
                    attributes: vec![
                        ("question".into(), Attr::Str(question_id.clone())),
                        ("sculptured.seat".into(), Attr::Str(seat.into())),
                    ],
                });
                vec![]
            }
            EventBody::QuestionAnswered { question_id, .. } => {
                self.run_events.push(SpanEvent {
                    name: "question answered".into(),
                    at_ms: e.at_ms,
                    attributes: vec![("question".into(), Attr::Str(question_id.clone()))],
                });
                vec![]
            }
            EventBody::StallSuspected { reason } => {
                self.run_events.push(SpanEvent {
                    name: "stall suspected".into(),
                    at_ms: e.at_ms,
                    attributes: vec![
                        ("reason".into(), Attr::Str(reason.clone())),
                        ("sculptured.seat".into(), Attr::Str(seat.into())),
                    ],
                });
                vec![]
            }
            EventBody::CheckRan { .. } => {
                self.check(e, Some(seat));
                vec![]
            }
            EventBody::HandedOver { .. } => {
                self.run_events.push(SpanEvent {
                    name: "handed over".into(),
                    at_ms: e.at_ms,
                    attributes: vec![("sculptured.seat".into(), Attr::Str(seat.into()))],
                });
                vec![]
            }
            _ => vec![],
        }
    }

    fn turn_key(&self, seat: &str, sk: &str, turn_id: Option<&str>) -> Option<String> {
        turn_id
            .map(str::to_string)
            .or_else(|| self.turn_of.get(seat).cloned())
            .map(|t| format!("turn:{sk}:{t}"))
    }

    fn end(&mut self, key: &str, at_ms: i64, error: bool) -> Option<Span> {
        let o = self.open.remove(key)?;
        let mut s = o.span;
        (s.end_ms, s.error) = (at_ms, error);
        if key.starts_with("turn:") {
            let [i, out, cr, cw] = o.tokens;
            s.attributes.push((
                "gen_ai.usage.input_tokens".into(),
                Attr::Int((i + cr + cw) as i64),
            ));
            s.attributes
                .push(("gen_ai.usage.output_tokens".into(), Attr::Int(out as i64)));
            s.attributes.push((
                "gen_ai.usage.cache_read_input_tokens".into(),
                Attr::Int(cr as i64),
            ));
            s.attributes.push((
                "gen_ai.usage.cache_creation_input_tokens".into(),
                Attr::Int(cw as i64),
            ));
            if let Some(c) = o.cost {
                s.attributes
                    .push(("gen_ai.usage.cost".into(), Attr::Float(c)));
            }
        }
        if self.ctx.full && (key.starts_with("turn:") || key.starts_with("tool:")) {
            if !o.input.is_empty() {
                s.attributes
                    .push(("lmnr.span.input".into(), Attr::Str(cap(&o.input))));
            }
            if !o.output.is_empty() {
                s.attributes
                    .push(("lmnr.span.output".into(), Attr::Str(cap(&o.output))));
            }
        }
        let _ = o.seat;
        Some(s)
    }

    /// Agent, turn and tool spans begun and not yet ended.
    #[must_use]
    pub fn open_spans(&self) -> usize {
        self.open.len()
    }

    /// Ends every open span at `end_ms`, marked as an error and `sculptured.incomplete` (an agent that
    /// never reported its end), so nothing begun is left out of the trace.
    pub fn close_open(&mut self, end_ms: i64) -> Vec<Span> {
        let keys: Vec<String> = self.open.keys().cloned().collect();
        keys.iter()
            .filter_map(|k| self.end(k, end_ms, true))
            .map(|mut s| {
                s.attributes
                    .push(("sculptured.incomplete".into(), Attr::Bool(true)));
                s
            })
            .collect()
    }

    /// The run's own span, once the run has ended: from its first event to `end_ms`, with what happened.
    pub fn finish_run(&mut self, end_ms: i64) -> Span {
        let attrs = vec![
            (
                "sculptured.team".into(),
                Attr::Str(self.ctx.team_name.clone()),
            ),
            (
                "sculptured.team_id".into(),
                Attr::Str(self.ctx.team.clone()),
            ),
            ("lmnr.span.type".into(), Attr::Str("DEFAULT".into())),
        ];
        Span {
            trace_id: self.trace_id.clone(),
            span_id: self.run_id.clone(),
            parent: None,
            name: format!("run {}", self.ctx.team_name),
            start_ms: self.run_start.unwrap_or(end_ms),
            end_ms,
            attributes: attrs,
            events: std::mem::take(&mut self.run_events),
            error: false,
        }
    }
}

fn attr_json(k: &str, v: &Attr) -> Value {
    let value = match v {
        Attr::Str(s) => json!({ "stringValue": s }),
        Attr::Int(n) => json!({ "intValue": n.to_string() }),
        Attr::Float(f) => json!({ "doubleValue": f }),
        Attr::Bool(b) => json!({ "boolValue": b }),
    };
    json!({ "key": k, "value": value })
}

fn ns(ms: i64) -> String {
    (i128::from(ms) * 1_000_000).to_string()
}

/// The spans without prompts, outputs and tool inputs and outputs (when only timings are sent).
#[must_use]
pub fn without_texts(mut spans: Vec<Span>) -> Vec<Span> {
    for s in &mut spans {
        s.attributes
            .retain(|(k, _)| k != "lmnr.span.input" && k != "lmnr.span.output");
    }
    spans
}

/// OTLP/HTTP JSON for these spans (`POST …/v1/traces`).
#[must_use]
pub fn otlp(spans: &[Span]) -> Value {
    let spans: Vec<Value> = spans.iter().map(|s| {
        let mut v = json!({
            "traceId": s.trace_id, "spanId": s.span_id, "name": s.name, "kind": 1,
            "startTimeUnixNano": ns(s.start_ms), "endTimeUnixNano": ns(s.end_ms),
            "attributes": s.attributes.iter().map(|(k, v)| attr_json(k, v)).collect::<Vec<_>>(),
            "events": s.events.iter().map(|e| json!({ "name": e.name, "timeUnixNano": ns(e.at_ms), "attributes": e.attributes.iter().map(|(k, v)| attr_json(k, v)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            "status": { "code": if s.error { 2 } else { 1 } },
        });
        if let Some(p) = &s.parent {
            v["parentSpanId"] = json!(p);
        }
        v
    }).collect();
    json!({ "resourceSpans": [{
        "resource": { "attributes": [attr_json("service.name", &Attr::Str("sculptured".into()))] },
        "scopeSpans": [{ "scope": { "name": "sculptured" }, "spans": spans }],
    }] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Actor, EventBody, EventRecord, Provenance, Subject, TurnStatus};

    fn ev(seq: i64, seat: &str, body: EventBody) -> EventRecord {
        EventRecord {
            seq,
            host: "h".into(),
            host_seq: seq as u64,
            at_ms: 1_000 * seq,
            actor: Actor::System("test".into()),
            subject: Subject::Seat(seat.into()),
            provenance: Provenance::ProtocolObserved,
            body,
        }
    }
    fn rig_ev(seq: i64, body: EventBody) -> EventRecord {
        EventRecord {
            subject: Subject::Rig("t".into()),
            ..ev(seq, "x", body)
        }
    }
    fn tenure_started() -> EventBody {
        EventBody::TenureStarted {
            tenure: 1,
            cause: model::TenureCause::Fresh,
            driver: "claude-code".into(),
            native_session_id: None,
            caps: Default::default(),
        }
    }
    fn ctx(full: bool) -> Ctx {
        Ctx {
            team: "t".into(),
            team_name: "My team".into(),
            start_seq: 0,
            names: [("team.a@t".to_string(), "Writer".to_string())].into(),
            full,
        }
    }
    fn attr<'a>(s: &'a Span, k: &str) -> Option<&'a Attr> {
        s.attributes
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v)
    }

    fn one_turn(full: bool) -> (Vec<Span>, Builder) {
        let a = "team.a@t";
        let mut b = Builder::new(ctx(full));
        let mut out = vec![];
        for e in [
            ev(1, a, tenure_started()),
            ev(
                2,
                a,
                EventBody::ModelObserved {
                    requested: None,
                    observed: Some("claude-sonnet-x".into()),
                    provider: None,
                    effort: None,
                    rerouted_from: None,
                    reason: None,
                },
            ),
            ev(
                3,
                a,
                EventBody::MessageQueued {
                    message_id: "m1".into(),
                    intent: model::MessageIntent::Instruct,
                    text: "Write the intro".into(),
                },
            ),
            ev(
                4,
                a,
                EventBody::TurnStarted {
                    turn_id: "t1".into(),
                },
            ),
            ev(
                5,
                a,
                EventBody::MessageConsumed {
                    message_id: "m1".into(),
                    turn_id: "t1".into(),
                },
            ),
            ev(
                6,
                a,
                EventBody::ToolCalled {
                    turn_id: Some("t1".into()),
                    call_id: "c1".into(),
                    name: "Read".into(),
                    input: serde_json::json!({"path":"a.md"}),
                    parent_call_id: None,
                },
            ),
            ev(
                7,
                a,
                EventBody::ToolResulted {
                    turn_id: Some("t1".into()),
                    call_id: "c1".into(),
                    output: "x".repeat(10_000),
                    is_error: false,
                },
            ),
            ev(
                8,
                a,
                EventBody::AgentText {
                    turn_id: Some("t1".into()),
                    text: "Here it is.".into(),
                },
            ),
            ev(
                9,
                a,
                EventBody::UsageReported {
                    turn_id: Some("t1".into()),
                    input_tokens: 100,
                    output_tokens: 50,
                    cache_read_tokens: 10,
                    cache_write_tokens: 0,
                    cost: Some(model::Cost {
                        usd: 0.01,
                        source: model::CostSource::ClaudeCode,
                    }),
                    in_turn: false,
                },
            ),
            ev(
                10,
                a,
                EventBody::TurnEnded {
                    turn_id: "t1".into(),
                    status: TurnStatus::Completed,
                },
            ),
            ev(
                11,
                a,
                EventBody::TenureEnded {
                    tenure: 1,
                    exit_code: Some(0),
                    signal: None,
                    stderr_tail: String::new(),
                },
            ),
        ] {
            out.extend(b.feed(&e));
        }
        (out, b)
    }

    #[test]
    fn a_turn_with_a_tool_and_usage_nests_under_its_agent_and_run() {
        let (spans, mut b) = one_turn(false);
        let names: Vec<&str> = spans.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["tool Read", "turn", "agent Writer"],
            "each sent when it ends"
        );
        let (tool, turn, agent) = (&spans[0], &spans[1], &spans[2]);
        let run = b.finish_run(20_000);
        assert!(
            spans
                .iter()
                .chain([&run])
                .all(|s| s.trace_id == run.trace_id && s.trace_id.len() == 32)
        );
        assert_eq!(
            (
                tool.parent.as_deref(),
                turn.parent.as_deref(),
                agent.parent.as_deref(),
                run.parent.as_deref()
            ),
            (
                Some(turn.span_id.as_str()),
                Some(agent.span_id.as_str()),
                Some(run.span_id.as_str()),
                None
            )
        );
        assert_eq!((turn.start_ms, turn.end_ms), (4_000, 10_000));
        assert_eq!(
            attr(turn, "gen_ai.usage.input_tokens"),
            Some(&Attr::Int(110))
        );
        assert_eq!(
            attr(turn, "gen_ai.usage.output_tokens"),
            Some(&Attr::Int(50))
        );
        assert_eq!(
            attr(turn, "gen_ai.usage.cache_read_input_tokens"),
            Some(&Attr::Int(10))
        );
        assert_eq!(attr(turn, "gen_ai.usage.cost"), Some(&Attr::Float(0.01)));
        assert_eq!(
            attr(turn, "gen_ai.response.model"),
            Some(&Attr::Str("claude-sonnet-x".into()))
        );
        assert_eq!(
            attr(turn, "gen_ai.system"),
            Some(&Attr::Str("anthropic".into()))
        );
        assert_eq!(attr(turn, "lmnr.span.type"), Some(&Attr::Str("LLM".into())));
        assert_eq!(
            attr(tool, "lmnr.span.type"),
            Some(&Attr::Str("TOOL".into()))
        );
        assert_eq!(
            attr(agent, "sculptured.agent"),
            Some(&Attr::Str("Writer".into()))
        );
        assert_eq!(
            attr(agent, "sculptured.computer"),
            Some(&Attr::Str("h".into())),
            "the computer it ran on"
        );
        assert_eq!(
            attr(&run, "sculptured.team"),
            Some(&Attr::Str("My team".into()))
        );
        assert!(
            spans
                .iter()
                .all(|s| attr(s, "lmnr.span.input").is_none()
                    && attr(s, "lmnr.span.output").is_none()),
            "timings, tokens and costs only"
        );
    }

    #[test]
    fn prompts_and_outputs_are_sent_only_when_chosen_and_capped() {
        let (spans, _) = one_turn(true);
        let (tool, turn) = (&spans[0], &spans[1]);
        assert_eq!(
            attr(turn, "lmnr.span.input"),
            Some(&Attr::Str("Write the intro".into()))
        );
        assert_eq!(
            attr(turn, "lmnr.span.output"),
            Some(&Attr::Str("Here it is.".into()))
        );
        let Some(Attr::Str(out)) = attr(tool, "lmnr.span.output") else {
            panic!("tool output")
        };
        assert!(out.len() <= 4096 + 3, "capped: {}", out.len());
    }

    #[test]
    fn failures_are_marked_and_the_run_carries_what_happened() {
        let a = "team.a@t";
        let mut b = Builder::new(ctx(false));
        let mut out = vec![];
        for e in [
            ev(1, a, tenure_started()),
            ev(
                2,
                a,
                EventBody::TurnStarted {
                    turn_id: "t1".into(),
                },
            ),
            ev(
                3,
                a,
                EventBody::ToolCalled {
                    turn_id: Some("t1".into()),
                    call_id: "c1".into(),
                    name: "Bash".into(),
                    input: serde_json::json!({}),
                    parent_call_id: None,
                },
            ),
            ev(
                4,
                a,
                EventBody::ToolResulted {
                    turn_id: Some("t1".into()),
                    call_id: "c1".into(),
                    output: "no".into(),
                    is_error: true,
                },
            ),
            ev(
                5,
                a,
                EventBody::TurnEnded {
                    turn_id: "t1".into(),
                    status: TurnStatus::Failed,
                },
            ),
            rig_ev(
                6,
                EventBody::BudgetStateChanged {
                    state: "wrap-up".into(),
                    reason: "Tokens: 85%".into(),
                },
            ),
        ] {
            out.extend(b.feed(&e));
        }
        assert!(
            out.iter().all(|s| s.error),
            "a failed tool and a failed turn are errors"
        );
        let run = b.finish_run(9_000);
        assert_eq!(
            run.events
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["budget wrap-up"]
        );
        assert_eq!(run.end_ms, 9_000);
    }

    #[test]
    fn otlp_json_has_the_shape_collectors_read() {
        let (spans, _) = one_turn(false);
        let v = otlp(&spans);
        let s = &v["resourceSpans"][0]["scopeSpans"][0]["spans"][1];
        assert_eq!(
            v["resourceSpans"][0]["resource"]["attributes"][0],
            serde_json::json!({"key":"service.name","value":{"stringValue":"sculptured"}})
        );
        assert_eq!(s["name"], "turn");
        assert_eq!(s["startTimeUnixNano"], "4000000000");
        assert_eq!(s["kind"], 1);
        assert!(s["attributes"].as_array().unwrap().contains(
            &serde_json::json!({"key":"gen_ai.usage.input_tokens","value":{"intValue":"110"}})
        ));
        assert_eq!(s["status"]["code"], 1);
    }

    fn session(tenure: u32) -> EventBody {
        EventBody::TenureStarted {
            tenure,
            cause: model::TenureCause::Fresh,
            driver: "claude-code".into(),
            native_session_id: None,
            caps: Default::default(),
        }
    }
    fn session_end(tenure: u32) -> EventBody {
        EventBody::TenureEnded {
            tenure,
            exit_code: Some(0),
            signal: None,
            stderr_tail: String::new(),
        }
    }

    #[test]
    fn each_session_of_an_agent_gets_its_own_spans() {
        let a = "team.a@t";
        let mut b = Builder::new(ctx(false));
        let mut spans = vec![];
        for (n, t) in [(0, 1), (10, 2)] {
            for e in [
                ev(n + 1, a, session(t)),
                ev(
                    n + 2,
                    a,
                    EventBody::TurnStarted {
                        turn_id: "turn-1".into(),
                    },
                ),
                ev(
                    n + 3,
                    a,
                    EventBody::TurnEnded {
                        turn_id: "turn-1".into(),
                        status: TurnStatus::Completed,
                    },
                ),
                ev(n + 4, a, session_end(t)),
            ] {
                spans.extend(b.feed(&e));
            }
        }
        let ids: std::collections::HashSet<&str> =
            spans.iter().map(|s| s.span_id.as_str()).collect();
        assert_eq!(
            (spans.len(), ids.len()),
            (4, 4),
            "two agent and two turn spans, none sharing an id"
        );
    }

    #[test]
    fn a_check_on_an_agent_shows_on_the_run() {
        let mut b = Builder::new(ctx(false));
        b.feed(&ev(
            1,
            "team.a@t",
            EventBody::CheckRan {
                command: "npm test".into(),
                passed: false,
                exit_code: Some(1),
                timed_out: false,
                output: "1 failed".into(),
                duration_ms: 5,
            },
        ));
        let run = b.finish_run(9_000);
        assert_eq!(
            run.events
                .iter()
                .map(|e| e.name.as_str())
                .collect::<Vec<_>>(),
            ["check failed"]
        );
    }

    #[test]
    fn spans_still_open_when_the_run_ends_are_sent_as_incomplete() {
        let a = "team.a@t";
        let mut b = Builder::new(ctx(false));
        b.feed(&ev(1, a, session(1)));
        b.feed(&ev(
            2,
            a,
            EventBody::TurnStarted {
                turn_id: "turn-1".into(),
            },
        ));
        let mut closed = b.close_open(5_000);
        closed.sort_by_key(|s| s.name.clone());
        assert_eq!(
            closed.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["agent Writer", "turn"]
        );
        assert!(closed.iter().all(|s| s.error
            && s.end_ms == 5_000
            && attr(s, "sculptured.incomplete") == Some(&Attr::Bool(true))));
        assert_eq!(b.open_spans(), 0);
    }

    #[test]
    fn texts_can_be_left_out_when_sending() {
        let (spans, _) = one_turn(true);
        assert!(spans.iter().any(|s| attr(s, "lmnr.span.input").is_some()));
        let bare = without_texts(spans);
        assert!(
            bare.iter()
                .all(|s| attr(s, "lmnr.span.input").is_none()
                    && attr(s, "lmnr.span.output").is_none())
        );
    }

    #[test]
    fn a_tool_call_outside_any_agent_session_hangs_under_the_run() {
        let mut b = Builder::new(ctx(false));
        b.feed(&ev(
            1,
            "team.a@t",
            EventBody::ToolCalled {
                turn_id: None,
                call_id: "c1".into(),
                name: "Read".into(),
                input: serde_json::json!({}),
                parent_call_id: None,
            },
        ));
        let spans = b.feed(&ev(
            2,
            "team.a@t",
            EventBody::ToolResulted {
                turn_id: None,
                call_id: "c1".into(),
                output: "x".into(),
                is_error: false,
            },
        ));
        let run = b.finish_run(3_000);
        assert_eq!(
            spans[0].parent.as_deref(),
            Some(run.span_id.as_str()),
            "never a second root"
        );
    }

    #[test]
    fn a_turn_says_how_it_ended_and_only_a_failure_is_an_error() {
        let a = "team.a@t";
        let mut b = Builder::new(ctx(false));
        b.feed(&ev(1, a, session(1)));
        let mut ended = |n: i64, status: TurnStatus| {
            b.feed(&ev(
                n,
                a,
                EventBody::TurnStarted {
                    turn_id: format!("t{n}"),
                },
            ));
            b.feed(&ev(
                n + 1,
                a,
                EventBody::TurnEnded {
                    turn_id: format!("t{n}"),
                    status,
                },
            ))
            .remove(0)
        };
        let (done, stopped, failed) = (
            ended(2, TurnStatus::Completed),
            ended(4, TurnStatus::Interrupted),
            ended(6, TurnStatus::Failed),
        );
        assert_eq!(
            attr(&stopped, "sculptured.turn_status"),
            Some(&Attr::Str("interrupted".into()))
        );
        assert_eq!(
            (done.error, stopped.error, failed.error),
            (false, false, true)
        );
    }
}
