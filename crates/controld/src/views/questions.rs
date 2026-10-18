//! Questions, decisions and Needs you.
use super::SeatView;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use model::{Answer, DecisionSource, LimitKind, Provenance, QuestionPrompt, QuestionSource, QuestionState, RiskFlag};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RestoreOffer {
    /// Why, verbatim (the restart, a resume error, or why a resume cannot be tried).
    pub reason: String,
    /// There is a session to resume: the agent counts in its team's one "Resume N agents?" ask
    /// (decision 1; nothing is resumed until the operator answers it).
    pub can_resume: bool,
}

/// The agent's last turn ended on the provider's usage or rate limit (Task 1's `LimitReached`). Nothing
/// retries it; Needs you says when it resets and that the operator sends "continue" after that.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LimitHit {
    pub kind: LimitKind,
    /// The reset time in the CLI's own words ("4:30am"); None when they name none (shown as unknown).
    pub resets: Option<String>,
    /// The CLI's words, verbatim (evidence).
    pub text: String,
    #[ts(type = "number")]
    pub at_ms: i64,
}

/// A heuristic stall: the agent keeps failing the same way inside its turn, or has gone silent in it.
/// Flagged only; nothing acts on it. Cleared by a successful tool result (a silent one by any output),
/// a new turn (for example a nudge) or a new session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Stall {
    /// Why, with the repeated error verbatim (evidence).
    pub reason: String,
    #[ts(type = "number")]
    pub at_ms: i64,
    /// No output for a long time in its turn, rather than a repeated error.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub silent: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DecisionView {
    /// What the agent decided, verbatim; "unknown" when it did not say.
    pub text: String,
    pub rationale: Option<String>,
    pub source: DecisionSource,
    pub provenance: Provenance,
    #[ts(type = "number")]
    pub at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QuestionView {
    pub id: String,
    pub seat: String,
    pub rig: String,
    pub source: QuestionSource,
    pub prompt: QuestionPrompt,
    pub state: QuestionState,
    #[ts(type = "number")]
    pub opened_at_ms: i64,
    #[ts(type = "number | null")]
    pub deadline_ms: Option<i64>,
    pub answer: Option<Answer>,
    #[ts(type = "number | null")]
    pub answered_at_ms: Option<i64>,
    /// An answer given after expiry, delivered as an overriding instruction.
    pub late_answer: Option<Answer>,
    pub decision: Option<DecisionView>,
    pub withdrawn_reason: Option<String>,
    pub acknowledged: bool,
    /// Permission prompts: what Sculptured's reading of the tool call says it would do (≈ heuristic). Empty when none found.
    pub consequence: Vec<RiskFlag>,
    /// The expiry's event seq, so the UI can link what the agent did next.
    #[ts(type = "number | null")]
    pub expired_seq: Option<i64>,
    /// The recorded answer (or expiry) may not have reached the agent: why, in the host's or program's words
    /// (verbatim). None when it was confirmed or never in doubt. "Send again" re-sends it.
    pub unconfirmed: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum InboxKind {
    Question,
    Permission,
    /// Expired, not yet acknowledged: shows the agent's decision.
    Expired,
    /// An agent in attention (stopped unexpectedly, lost, failed launch), failed, or stale.
    Attention,
    /// An agent whose handoffs cannot go on by themselves (`SeatView.handoff_stuck`).
    Stuck,
    /// A restore that waits for the operator's decision (one per agent; the inbox UI groups a team's
    /// resumable ones into its one "Resume N agents?" row).
    Restore,
    /// A running agent that keeps failing the same way (`SeatView.stalled`). Heuristic; never acted on.
    Stalled,
    /// A running agent whose last turn ended on a usage or rate limit (`SeatView.limit_hit`). No automatic retry.
    Limit,
    /// An answered question whose answer may not have reached the agent (`QuestionView.unconfirmed`),
    /// while the agent may still be waiting for it (its process runs and the asking turn is current).
    Unconfirmed,
    /// An item waiting at a human stage of a workflow (`InboxItem.decision`).
    Decision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InboxItem {
    /// `q:<question id>` or `seat:<seat id>`; stable across refreshes.
    pub id: String,
    pub kind: InboxKind,
    pub rig: String,
    pub seat: Option<String>,
    #[ts(type = "number")]
    pub opened_at_ms: i64,
    pub question: Option<QuestionView>,
    pub seat_view: Option<SeatView>,
    #[serde(default)]
    pub decision: Option<WaitingDecision>,
}

/// A workflow item waiting for a person: the stage's question, its outcomes and the latest steps as evidence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WaitingDecision {
    pub team: String,
    pub team_name: String,
    #[ts(type = "number")]
    pub start_seq: i64,
    pub task: String,
    pub title: String,
    pub stage: String,
    pub stage_name: String,
    pub prompt: String,
    pub outcomes: Vec<String>,
    #[ts(type = "number")]
    pub since_ms: i64,
    /// The last three steps, oldest first.
    pub recent: Vec<model::WorkflowStep>,
}

