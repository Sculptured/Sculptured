use crate::{
    ActionClass, Answer, Caps, DecisionSource, Desired, Finding, LimitKind, MessageIntent, QuestionPrompt, QuestionSource,
    ReportVerdict, RestoreDecision, RiskFlag, SeatActivity, SeatLifecycle, SessionMode, TenureCause, TurnStatus,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Provenance {
    ProtocolObserved,
    AgentReported,
    FilesystemObserved,
    CapturedTool,
    Verified,
    Heuristic,
    Operator,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(tag = "kind", content = "id", rename_all = "kebab-case")]
#[ts(export)]
pub enum Actor {
    Human(String),
    Seat(String),
    System(String),
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Actor::Human(id) => write!(f, "human:{id}"),
            Actor::Seat(id) => write!(f, "seat:{id}"),
            Actor::System(id) => write!(f, "system:{id}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(tag = "kind", content = "id", rename_all = "kebab-case")]
#[ts(export)]
pub enum Subject {
    Rig(String),
    Seat(String),
    Host(String),
}

impl Subject {
    pub fn key(&self) -> String {
        match self {
            Subject::Rig(id) => format!("rig:{id}"),
            Subject::Seat(id) => format!("seat:{id}"),
            Subject::Host(id) => format!("host:{id}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[ts(export)]
pub enum EventBody {
    TeamTopologyChanged { parent: String, child: String },
    RigBooted { spec_hash: String, seat_count: u32 },
    RigDesiredChanged { desired: Desired },
    SeatLifecycleChanged { lifecycle: SeatLifecycle, reason: Option<String> },
    SeatActivityChanged { activity: SeatActivity },
    TenureStarted { tenure: u32, cause: TenureCause, driver: String, native_session_id: Option<String>, caps: Caps },
    TenureEnded { tenure: u32, exit_code: Option<i32>, signal: Option<i32>, stderr_tail: String },
    TurnStarted { turn_id: String },
    AgentText { turn_id: Option<String>, text: String },
    ToolCalled {
        turn_id: Option<String>,
        call_id: String,
        name: String,
        input: serde_json::Value,
        parent_call_id: Option<String>,
    },
    ToolResulted { turn_id: Option<String>, call_id: String, output: String, is_error: bool },
    UsageReported {
        turn_id: Option<String>,
        #[ts(type = "number")]
        input_tokens: u64,
        #[ts(type = "number")]
        output_tokens: u64,
        #[ts(type = "number")]
        cache_read_tokens: u64,
        #[ts(type = "number")]
        cache_write_tokens: u64,
        /// The turn's estimated cost and where it came from; absent when no source prices it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        cost: Option<crate::Cost>,
        /// Counted from one model call while the turn ran (no cost); the turn's last report has the rest.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(as = "Option<bool>", optional)]
        in_turn: bool,
    },
    TurnEnded { turn_id: String, status: TurnStatus },
    MessageQueued { message_id: String, intent: MessageIntent, text: String },
    MessageDelivered { message_id: String },
    MessageConsumed { message_id: String, turn_id: String },
    MessageFailed { message_id: String, reason: String },
    /// controld keeps the message until a slot frees (the run budget's "most agents working at once").
    MessageHeld { message_id: String, reason: String },
    /// A protection refused one of the agent's tool calls, with the reason the agent read.
    ProtectionApplied { call_id: String, reason: String },
    /// Heuristic: the agent has been in a turn with no output for a long time (an idea from Gas Town's
    /// patrol). Flagged only; the operator decides. Its next output clears it.
    StallSuspected { reason: String },
    /// The agent's context passed the team's ceiling: it writes its notes and continues fresh.
    ContextCeilingReached {
        #[ts(type = "number")]
        used_tokens: u64,
        #[ts(type = "number")]
        ceiling: u64,
    },
    EventsLost {
        #[ts(type = "number")]
        count: u64,
    },
    Unrecognized { raw: serde_json::Value },
    /// controld: this seat waits for these upstream seats (along "hands work to"
    /// connections) to finish before it gets its first handoff, and whether its handoffs
    /// are stuck. Empty and None = neither waiting nor stuck.
    HandoffWaiting {
        waiting_for: Vec<String>,
        #[serde(default)]
        stuck: Option<HandoffStuck>,
    },
    /// controld: these upstream turns were handed over to this seat as message `message_id`
    /// in the start that began at `start_seq` (its `RigBooted`). Recorded in the same
    /// transaction as that message, so a turn is never handed twice. `resend_of` names the
    /// earlier message this one re-sends, when it is a resend.
    HandedOver {
        #[serde(default)]
        #[ts(type = "number")]
        start_seq: i64,
        message_id: String,
        from: Vec<HandoffSource>,
        #[serde(default)]
        resend_of: Option<String>,
        /// The "More work" wave this message belongs to, if any.
        #[serde(default)]
        wave: Option<Wave>,
    },
    /// The operator asked this seat to hand its latest finished turn over again
    /// ("Hand over again"). `turn_seq` is that turn's `TurnEnded` seq.
    HandoverRequested {
        turn_id: String,
        #[ts(type = "number")]
        turn_seq: i64,
    },
    /// The agent (or its CLI) asks the operator something and waits.
    QuestionRaised { question_id: String, source: QuestionSource, prompt: QuestionPrompt, turn_id: Option<String> },
    /// `late`: the question had already expired; the answer is an instruct message overriding the agent's decision.
    QuestionAnswered { question_id: String, answer: Answer, late: bool },
    QuestionExpired { question_id: String },
    QuestionDecided { question_id: String, decision: String, rationale: Option<String>, source: DecisionSource },
    QuestionWithdrawn { question_id: String, reason: String },
    /// The operator has seen an expired question; it leaves Needs you.
    QuestionAcknowledged { question_id: String },
    /// The agent's `report` call: findings about its work (agent-reported). `turn_id` is the
    /// turn running when it was received, so the work ledger can place it in an attempt.
    FindingsReported { turn_id: Option<String>, summary: String, findings: Vec<Finding>, verdict: ReportVerdict },
    /// An agent report or tool call that failed validation (spec §6.3). Evidence; never interrupts the agent.
    ReportRejected { tool: String, reason: String, raw: serde_json::Value },
    ActionInterpreted {
        call_id: String,
        class: ActionClass,
        #[serde(default)]
        paths: Vec<String>,
        #[serde(default)]
        risks: Vec<RiskFlag>,
    },
    /// The operator asked for a new session (spec §6.8 handover). A reason is required.
    NewSessionRequested { mode: SessionMode, reason: String },
    /// A restore needs the operator: the resume failed or cannot be tried.
    RestoreNeeded { reason: String, can_resume: bool },
    RestoreDecided { decision: RestoreDecision },
    /// The turn ended on the provider's usage or rate limit (`text`: the CLI's words, verbatim; `resets`:
    /// the time those words name, if any). The operator sends "continue" once it resets; Sculptured
    /// never retries by itself. Recorded just before the turn's `TurnEnded { Failed }`.
    LimitReached { turn_id: Option<String>, kind: LimitKind, resets: Option<String>, text: String },
    /// controld launched this agent without optional abilities its host (an older agent runner) lacks:
    /// `features` names them (`host_proto::features`), `note` says so in plain words and how to update.
    /// Never posture: a restricted agent is refused instead.
    FeaturesDropped { features: Vec<String>, note: String },
    /// A recorded answer could not reach the agent for certain (its host was not connected, or the
    /// agent's program refused it): the question is open again. `reason`: the host's words, verbatim.
    QuestionAnswerFailed { question_id: String, reason: String },
    /// The agent's program reports the question settled (hostd maps the driver's `QuestionResolved`).
    /// Sculptured records its own answer or expiry before delivering it, so a question still open
    /// when this arrives was settled by the program itself, without Sculptured's answer. On an answered
    /// (or expired) question it confirms the delivery.
    QuestionResolved { question_id: String },
    /// The recorded answer (or expiry) may not have reached the agent: it was sent but no reply came, or a
    /// "Send again" was refused. `reason`: the host's or program's words, verbatim. The question stays
    /// answered; Needs you shows it until a "Send again" or the program confirms it.
    QuestionAnswerUnconfirmed { question_id: String, reason: String },
    /// A "Send again" of the recorded answer (or expiry) reached the agent: the doubt is cleared.
    QuestionAnswerConfirmed { question_id: String },
    /// The model a session asked for and, once the CLI says, the model it reports using.
    /// `requested`: what Sculptured passed (None = the CLI's own default). `observed`: the CLI's own
    /// words (Claude `system/init.model`, Codex `thread/start` result `model` or `model/rerouted.toModel`);
    /// None until the CLI says. Recorded after the session's `TenureStarted`, which clears the seat's model.
    ModelObserved {
        requested: Option<String>,
        observed: Option<String>,
        /// Codex `modelProvider` (e.g. "openai"); None for Claude.
        #[serde(default)]
        provider: Option<String>,
        /// Codex `reasoningEffort`, when reported.
        #[serde(default)]
        effort: Option<String>,
        /// Codex `model/rerouted` `fromModel`.
        #[serde(default)]
        rerouted_from: Option<String>,
        /// Codex reroute reason, verbatim.
        #[serde(default)]
        reason: Option<String>,
    },
    /// Intermediate reasoning the CLI showed (agent-reported). `text` verbatim, capped by
    /// `model::cap::truncate(_, MAX_TEXT)`; "" when `form` is `Hidden`.
    ThinkingRecorded {
        turn_id: Option<String>,
        /// Codex reasoning item id; Claude message id.
        #[serde(default)]
        item_id: Option<String>,
        /// Set for a subagent's thinking (Claude `parent_tool_use_id`).
        #[serde(default)]
        parent_call_id: Option<String>,
        form: ThinkingForm,
        text: String,
        /// Claude `system/thinking_tokens` estimate, when reported.
        #[serde(default)]
        #[ts(type = "number | null")]
        estimated_tokens: Option<u64>,
    },
    /// How full the agent's context was after a model call.
    ContextReported {
        turn_id: Option<String>,
        #[ts(type = "number")]
        used_tokens: u64,
        /// The CLI's runtime window (None = the CLI did not say).
        #[ts(type = "number | null")]
        window_tokens: Option<u64>,
        basis: ContextBasis,
    },
    /// The CLI's report of how much of the signed-in plan is used (account-wide).
    PlanUsageReported { usage: crate::PlanUsage },
    /// The run's budget changed state: `wrap-up` (its soft value was reached) or `stopped` (its hard value).
    BudgetStateChanged { state: String, reason: String },
    /// Sculptured began running the agent's check command (its design's "check", in its folder).
    CheckStarted { command: String },
    /// The check's outcome as Sculptured observed it: the command's own exit status, not the agent's word.
    CheckRan {
        command: String,
        passed: bool,
        exit_code: Option<i32>,
        timed_out: bool,
        /// The last part of its combined output.
        output: String,
        #[ts(type = "number")]
        duration_ms: u64,
    },
}

/// What the CLI showed of a model's thinking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ThinkingForm {
    /// A summary of the reasoning (Claude thinking summaries, Codex reasoning summary).
    Summary,
    /// The reasoning text itself.
    Full,
    /// Thinking happened but the CLI returned no text.
    Hidden,
}

/// Where a context figure comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ContextBasis {
    /// The CLI's own per-call total.
    Reported,
    /// Sculptured adds up the CLI's numbers.
    Estimated,
}

/// A wave of "More work": started by one agent's finished work (or a "Hand over again"
/// press on it), it runs every agent downstream of that origin once.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Wave {
    /// The seq of the event that started it: the origin's `TurnEnded` or its `HandoverRequested`.
    #[ts(type = "number")]
    pub id: i64,
    /// The seat the wave started at.
    pub origin: String,
}

/// Why a seat's handoffs are stuck, in plain words naming agents (never seat ids).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct HandoffStuck {
    /// E.g. "Airframe needs attention", "Planner has no job", "the handoff from Lead was not sent".
    pub reason: String,
    /// True when re-sending a recorded handoff can help (it was not sent or not delivered).
    pub can_resend: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct HandoffSource {
    /// The upstream seat.
    pub seat: String,
    pub turn_id: String,
    /// The seq of that turn's `TurnEnded` event: the handoff's identity.
    #[ts(type = "number")]
    pub turn_seq: i64,
    /// Set when the operator's "Hand over again" sent this turn: that request's event seq.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub requested_seq: Option<i64>,
}

impl HandoffSource {
    /// What makes this handoff unique within a start: the request, else the turn.
    pub fn key(&self) -> i64 {
        self.requested_seq.unwrap_or(self.turn_seq)
    }
}

/// One append-only event. `seq` is controld's global order (0 before ingest);
/// `(host, host_seq)` is the producer's identity for deduplication.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct EventRecord {
    #[ts(type = "number")]
    pub seq: i64,
    pub host: String,
    #[ts(type = "number")]
    pub host_seq: u64,
    #[ts(type = "number")]
    pub at_ms: i64,
    pub actor: Actor,
    pub subject: Subject,
    pub provenance: Provenance,
    pub body: EventBody,
}
