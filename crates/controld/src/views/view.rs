use super::{
    Caps, CompiledRig, Deserialize, Desired, Diagnostic, DraftIssue, EventRecord, LimitHit,
    Provenance, RestoreOffer, Runtime, SeatActivity, SeatLifecycle, Serialize, Stall, TS,
    TeamDraft,
};

/// One seat's projected state. Every field comes from recorded events; unknown stays None.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SeatView {
    pub id: String,
    pub rig: String,
    pub pod: String,
    pub member: String,
    pub runtime: Runtime,
    pub label: Option<String>,
    pub host: Option<String>,
    pub lifecycle: SeatLifecycle,
    pub lifecycle_reason: Option<String>,
    pub activity: SeatActivity,
    pub tenure: u32,
    pub native_session_id: Option<String>,
    pub caps: Option<Caps>,
    pub current_turn: Option<String>,
    pub current_tool: Option<String>,
    #[ts(type = "number | null")]
    pub last_event_ms: Option<i64>,
    /// None until the first UsageReported: a driver without usage, or a turn whose
    /// usage was not reported, leaves the total unknown rather than 0.
    #[ts(type = "number | null")]
    pub tokens_in: Option<u64>,
    #[ts(type = "number | null")]
    pub tokens_out: Option<u64>,
    /// Provenance of the latest lifecycle change; None before the first one (planned).
    pub lifecycle_provenance: Option<Provenance>,
    /// Upstream seats this seat waits for before its first handoff (heuristic, from
    /// controld's `handoff-waiting` record). Absent when it is not waiting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waiting_for: Option<Vec<String>>,
    /// Why this seat's handoffs are stuck (heuristic, controld's `handoff-waiting` record).
    /// Absent when nothing is stuck.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub handoff_stuck: Option<model::HandoffStuck>,
    /// A message to it waits for a free slot (the run budget's "most agents working at once").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub slot_wait: bool,
    /// Questions waiting for the operator (Needs you).
    #[serde(default)]
    pub open_questions: u32,
    /// Risk flags raised in this agent's history (≈ heuristic, the action interpreter).
    #[serde(default)]
    pub risk_count: u32,
    /// Set while a restore waits for the operator's decision.
    #[serde(default)]
    pub restore_offer: Option<RestoreOffer>,
    /// The folder the agent was launched in; None before its first launch.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Set while the agent's last turn ended on a usage or rate limit; cleared by its next turn.
    #[serde(default)]
    pub limit_hit: Option<LimitHit>,
    /// The model the current session asked for, resolved at launch (None = the CLI's own default, or
    /// not known yet). Cleared when a new session starts.
    #[serde(default)]
    pub model_requested: Option<String>,
    /// The model the CLI reports using (protocol-observed), in its own words; None until it says.
    #[serde(default)]
    pub model_observed: Option<String>,
    /// Heuristic: keeps failing the same way in its turn. None when not stalled.
    #[serde(default)]
    pub stalled: Option<Stall>,
    /// The latest check Sculptured ran for this agent (its design's check command); None before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub check: Option<SeatCheck>,
}

/// A check's state as Sculptured observed it. `passed` is None while it runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SeatCheck {
    pub command: String,
    pub passed: Option<bool>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output: String,
    #[ts(type = "number")]
    pub at_ms: i64,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RigSummary {
    pub name: String,
    pub desired: Desired,
    pub seat_count: u32,
    pub running_count: u32,
    /// Seats in attention, failed or stale.
    pub attention_count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RigSnapshot {
    pub rig: CompiledRig,
    pub desired: Desired,
    /// Spec declaration order.
    pub seats: Vec<SeatView>,
    /// Highest event seq already reflected in this snapshot.
    #[ts(type = "number")]
    pub last_seq: i64,
}

/// One live update: the event plus the seat's state after it (when the subject is a seat).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StreamItem {
    pub event: EventRecord,
    pub seat: Option<SeatView>,
}

/// One live update on the every-team stream (`/api/stream?all=true`): the item, tagged
/// with its rig. `rig` is null for a host event, which concerns every rig on that host.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TaggedStreamItem {
    pub rig: Option<String>,
    #[serde(flatten)]
    pub item: StreamItem,
}

/// Stored response for an idempotent request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub fingerprint: String,
    pub status: u16,
    pub body: String,
}

/// A seat that `stop rig` could not stop, and the host's reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StopFailure {
    pub seat: String,
    pub reason: String,
}

/// Response body of `POST /api/rigs/{name}/stop`. `failed` is empty when every stop succeeded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StopRigOutcome {
    pub failed: Vec<StopFailure>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TeamSummary {
    pub id: String,
    pub name: String,
    pub agent_count: u32,
    #[ts(type = "number")]
    pub updated_at_ms: i64,
    /// None until the team has been started.
    pub started: Option<RigSummary>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TeamRecord {
    pub draft: TeamDraft,
    /// The draft revision the team was last started from.
    #[ts(type = "number | null")]
    pub started_revision: Option<u64>,
    /// True while any agent may be running or a start is in flight: structural edits wait for Stop team.
    pub live: bool,
    pub issues: Vec<DraftIssue>,
    /// Where the team came from, when it wasn't built by hand (e.g. generated by AI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub origin: Option<TeamOrigin>,
}

/// How a team was made, shown on it as provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TeamOrigin {
    /// "generated"
    pub kind: String,
    /// "claude" | "codex"
    pub cli: String,
    pub model: Option<String>,
    #[ts(type = "number")]
    pub at_ms: i64,
    /// The user's description.
    pub prompt: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TeamSpec {
    /// RigSpec YAML plus the "# sculptured:" layout comment.
    pub yaml: String,
    pub valid: bool,
    pub issues: Vec<DraftIssue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TeamImport {
    pub team: TeamRecord,
    /// Parts of the spec the builder could not keep, with their spec paths.
    pub dropped: Vec<Diagnostic>,
    /// What Design shows on the new team, such as a missing folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<DraftIssue>>", optional)]
    pub warnings: Vec<DraftIssue>,
}

/// CLI-reported tokens of the turn that took one workflow assignment (sums of its usage reports), and its
/// estimated cost where a source prices it (`model::pricing`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TurnTokens {
    #[ts(type = "number")]
    pub input: u64,
    #[ts(type = "number")]
    pub output: u64,
    #[ts(type = "number")]
    pub cache_read: u64,
    #[ts(type = "number")]
    pub cache_write: u64,
    /// The sum of the reports' estimated costs in US dollars; None when no report had one.
    pub cost_usd: Option<f64>,
    pub cost_source: Option<model::CostSource>,
    /// Some reports in the turn had no cost, so `cost_usd` is a lower bound.
    pub cost_partial: bool,
}
