//! The work ledger: work items, their attempts, decisions and sessions.
use model::{Finding, OutcomeEvidence, Provenance, ReportVerdict, TenureCause, Wave, WorkState};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum WorkKind {
    /// A head agent's Job, sent at Start.
    Job,
    /// A head agent without a Job: the first message the operator sent it.
    FirstMessage,
    /// A downstream agent's first handoff (it carries the agent's Job).
    FirstHandoff,
    /// A "More work" wave message.
    MoreWork,
}

/// Why an attempt started (the end of the previous one). Exactly the handoff retry causes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum AttemptCause {
    First,
    AfterInterrupted,
    AfterFailed,
    AfterAgentStopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum AttemptEnd {
    Running,
    Completed,
    Interrupted,
    Failed,
    AgentStopped,
}

/// One `report` call (always agent-reported, `◇ reported`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FindingView {
    #[ts(type = "number")]
    pub seq: i64,
    #[ts(type = "number")]
    pub at_ms: i64,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub verdict: ReportVerdict,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AttemptView {
    /// 1-based.
    pub n: u32,
    pub turn_id: String,
    pub tenure: u32,
    pub cause: AttemptCause,
    /// This attempt read a continuation (Restart this agent, a restore or a new session).
    pub continued: bool,
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[ts(type = "number | null")]
    pub ended_at_ms: Option<i64>,
    pub end: AttemptEnd,
    /// The turn's last reply, verbatim, cut at 600 characters (agent-reported). None when none was captured.
    pub reply: Option<String>,
    /// Captured test runs in this attempt's turn (the action interpreter) that exited 0 / did not.
    pub captured_test_runs: u32,
    pub captured_test_failures: u32,
    pub findings: Vec<FindingView>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WorkItemView {
    /// The work message's id (the original, for a re-sent handoff).
    pub id: String,
    pub seat: String,
    pub kind: WorkKind,
    /// Upstream seats whose results it carries; empty for a Job or first message.
    pub from: Vec<String>,
    pub wave: Option<Wave>,
    #[ts(type = "number")]
    pub queued_at_ms: i64,
    /// pending · in-progress · blocked · done · canceled. "done" means a work turn ended Completed (reported).
    pub state: WorkState,
    /// Plain words for blocked and canceled ("interrupted", "not sent: …"); verbatim reasons stay verbatim.
    pub state_reason: Option<String>,
    /// `reported-complete` when done; `captured-tool` when the completing turn also had a captured test run that passed. Never verified.
    pub outcome: Option<OutcomeEvidence>,
    /// Copies sent after the first (Resend, Restart and restore continuations).
    pub copies: u32,
    pub attempts: Vec<AttemptView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum DecisionKind {
    Answered,
    LateAnswer,
    AgentDecided,
    AllowedOnTimeout,
    DecisionUnknown,
    Withdrawn,
    NewSession,
    RestoreDecided,
    HandOverAgain,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DecisionEntry {
    #[ts(type = "number")]
    pub at_ms: i64,
    pub kind: DecisionKind,
    pub seat: Option<String>,
    /// A plain sentence; quoted parts are verbatim.
    pub text: String,
    pub provenance: Provenance,
    pub question_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Ledger {
    pub rig: String,
    /// The team's agents in handoff order (upstream first): the step board's rows.
    pub agents: Vec<String>,
    /// The current start's `RigBooted` seq; None before the first start (then everything is empty).
    #[ts(type = "number | null")]
    pub start_seq: Option<i64>,
    #[ts(type = "number | null")]
    pub started_at_ms: Option<i64>,
    /// The team was stopped after this start.
    pub ended: bool,
    /// Oldest first.
    pub work: Vec<WorkItemView>,
    /// Newest first.
    pub decisions: Vec<DecisionEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TenureView {
    pub tenure: u32,
    pub cause: TenureCause,
    /// The operator's reason (new session, restore decision), or the system's ("resumed after a restart").
    pub reason: Option<String>,
    pub driver: String,
    pub native_session_id: Option<String>,
    #[ts(type = "number")]
    pub started_at_ms: i64,
    #[ts(type = "number | null")]
    pub ended_at_ms: Option<i64>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    /// Files this session edited, from interpreted actions (≈ heuristic): the "left behind" summary.
    pub files_touched: Vec<String>,
}
