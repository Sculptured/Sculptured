//! Bounded task dispatch through reusable stages. Edges may branch and loop.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct Workflow {
    /// Board placement only; changing it does not alter workflow execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input_position: Option<BoardPosition>,
    #[serde(default)]
    pub input: String,
    /// The agent that calls dispatch_work; empty when `dispatch` makes the batch instead.
    #[serde(default)]
    pub dispatcher: String,
    pub entry: String,
    pub collector: String,
    pub max_tasks: u32,
    pub max_steps: u32,
    pub stages: Vec<WorkflowStage>,
    /// Pauses new work while this project check fails (Advanced).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub guard: Option<Guard>,
    /// per-item: each item gets its own git branch and folder (Advanced).
    #[serde(default, skip_serializing_if = "CopiesMode::is_shared")]
    #[ts(as = "Option<CopiesMode>", optional)]
    pub copies: CopiesMode,
    /// Sculptured runs this command at start and its output is the batch (no dispatcher agent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dispatch: Option<crate::WorkflowDispatch>,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum CopiesMode {
    #[default]
    Shared,
    PerItem,
}
impl CopiesMode {
    #[must_use]
    pub fn is_shared(&self) -> bool {
        *self == CopiesMode::Shared
    }
}

/// How a review panel decides: every reviewer, or the first approval.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Join {
    #[default]
    All,
    Any,
}
impl Join {
    #[must_use]
    pub fn is_all(&self) -> bool {
        *self == Join::All
    }
}

/// One reviewer's part in a panel stage: their assignment, and their vote once given.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct Review {
    pub agent: String,
    pub assignment: Option<String>,
    pub outcome: Option<String>,
    pub result: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct BoardPosition {
    pub x: i32,
    pub y: i32,
}

pub const DEFAULT_CHECK_TIMEOUT_S: u32 = 600;

/// An angle a review-panel reviewer takes, so a panel covers what one reviewer would miss.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ReviewPersona {
    RedTeam,
    Adversarial,
    Simplifier,
    User,
    Expert,
    GoodFriend,
    Ownership,
}

impl ReviewPersona {
    pub const ALL: [ReviewPersona; 7] = [
        Self::RedTeam,
        Self::Adversarial,
        Self::Simplifier,
        Self::User,
        Self::Expert,
        Self::GoodFriend,
        Self::Ownership,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::RedTeam => "Red team",
            Self::Adversarial => "Adversary",
            Self::Simplifier => "Simplifier",
            Self::User => "User",
            Self::Expert => "Domain expert",
            Self::GoodFriend => "Honest friend",
            Self::Ownership => "Ownership",
        }
    }

    /// What the reviewer looks for, and when the work passes from its angle.
    #[must_use]
    pub fn brief(self) -> &'static str {
        match self {
            Self::RedTeam => {
                "Look for ways to break it: untrusted input, injection, data loss, races, anything an attacker or an accident could exploit. It passes when it holds up under attack."
            }
            Self::Adversarial => {
                "Assume it is wrong and look for why: false assumptions, missed edge cases, failure modes, broken compatibility. It passes when its reasoning survives a direct challenge."
            }
            Self::Simplifier => {
                "Look for needless complexity: parts that could go, concepts that could merge, changes with a wider reach than needed. It passes when a smart newcomer could explain it simply."
            }
            Self::User => {
                "Look at it as someone who has never seen it: confusing steps, unclear names, errors that do not say what to do. It passes when they could use it without a guide."
            }
            Self::Expert => {
                "Review it as a peer in its field would: domain errors, wrong conventions, claims a specialist would question. It passes when such a peer would sign off."
            }
            Self::GoodFriend => {
                "Say what others will not: the cost, the risk at 3 a.m., the thing that will hurt later. It passes when there is nothing you would rather they heard now than later."
            }
            Self::Ownership => {
                "Look at who owns each fact and decision: data with two owners, guesses with no way back, parts that disagree about the same value. It passes when every fact has one owner and every guess can be undone."
            }
        }
    }
}

/// After `after_rounds` rounds at a stage, its items go to `agent` (typically a stronger model's agent).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Escalation {
    pub after_rounds: u32,
    pub agent: String,
}

/// A project check run every `every_minutes` while the team runs; while it fails, no new work goes out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Guard {
    pub command: String,
    pub every_minutes: u32,
    /// Seconds before the guard command is stopped (default 600, at most 3600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout_s: Option<u32>,
}

/// The guard's last result (verified: Sculptured ran the command) and whether a person resumed past a failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct GuardState {
    pub passed: bool,
    #[ts(type = "number")]
    pub at_ms: i64,
    pub evidence: String,
    pub resumed_by_person: bool,
}

/// Who runs a stage: an agent (default), Sculptured running a command, or a person.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum StageKind {
    #[default]
    Agent,
    Check,
    Human,
    Merge,
}
impl StageKind {
    #[must_use]
    pub fn is_agent(&self) -> bool {
        *self == StageKind::Agent
    }
}

/// Who settled a step: provenance for the Run view.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum StepBy {
    #[default]
    Agent,
    Check,
    Person,
    Sculptured,
}

/// The id of the item that assembles a batch's accepted results.
pub const ASSEMBLY: &str = "assembly";

/// Where a work item is. Completion of an agent stage is agent-reported.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum TaskStatus {
    /// Waiting for an agent, a check or a merge to take it.
    #[default]
    Queued,
    /// An agent, panel, check or merge is working on it.
    Assigned,
    /// At a human stage, waiting for a person.
    Waiting,
    Complete,
    Blocked,
}

impl TaskStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Assigned => "assigned",
            Self::Waiting => "waiting",
            Self::Complete => "complete",
            Self::Blocked => "blocked",
        }
    }
}

/// An item parked at a human stage, and since when.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Waiting {
    pub stage: String,
    #[ts(type = "number")]
    pub since_ms: i64,
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct WorkflowStage {
    pub id: String,
    pub name: String,
    /// Empty for check and human stages.
    #[serde(default)]
    pub agent: String,
    /// Additional interchangeable agents for concurrent topics at this stage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub workers: Vec<String>,
    /// An outcome selects one route. Null completes this item; "blocked" is reserved.
    pub routes: BTreeMap<String, Option<String>>,
    #[serde(default, skip_serializing_if = "StageKind::is_agent")]
    #[ts(as = "Option<StageKind>", optional)]
    pub kind: StageKind,
    /// check: the shell command, run in the team's project folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub command: Option<String>,
    /// check: seconds before the command is stopped (default 600, at most 3600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout_s: Option<u32>,
    /// human: what the person decides. The outcomes are the route keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prompt: Option<String>,
    /// How often one item may enter this stage; entering past it takes `on_limit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_rounds: Option<u32>,
    /// The stage to take when `max_rounds` is reached; none blocks the item.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub on_limit: Option<String>,
    /// A review panel: 2–6 agents who all take the same item (instead of `agent`/`workers`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub reviewers: Vec<String>,
    /// The angle each panel reviewer takes, by agent id (ideas from Advocate). None: its own judgement.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    #[ts(
        as = "Option<std::collections::BTreeMap<String, ReviewPersona>>",
        optional
    )]
    pub personas: std::collections::BTreeMap<String, ReviewPersona>,
    #[serde(default, skip_serializing_if = "Join::is_all")]
    #[ts(as = "Option<Join>", optional)]
    pub join: Join,
    /// Panel outcome order, worst first (default: blocked, then any non-"approved", then "approved").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub priority: Vec<String>,
    /// Hands items to a stronger agent after a number of rounds here (single-agent stages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub escalate: Option<Escalation>,
    /// Board placement of the stage's own box (check, person, merge, review panel); none places it
    /// automatically. Changing it does not alter workflow execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub position: Option<BoardPosition>,
    /// merge: the branch to merge into (default: the project's current branch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub into: Option<String>,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TopicMode {
    #[default]
    Independent,
    Dependent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DispatchTask {
    pub id: String,
    pub title: String,
    pub context: String,
    #[serde(default)]
    #[ts(as = "Option<TopicMode>", optional)]
    pub mode: TopicMode,
    #[serde(default)]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub depends_on: Vec<String>,
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct WorkflowStep {
    pub assignment: String,
    pub stage: String,
    pub agent: String,
    pub outcome: String,
    pub result: String,
    #[ts(type = "number")]
    pub at_ms: i64,
    #[serde(default)]
    #[ts(as = "Option<StepBy>", optional)]
    pub by: StepBy,
    /// Verified output (a check's command, exit status, end of output) or a limit note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub evidence: Option<String>,
    /// A panel's votes by reviewer (late ones are keyed `<agent> (late)`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(as = "Option<BTreeMap<String, String>>", optional)]
    pub votes: BTreeMap<String, String>,
    /// A panel's reviewers and their assignments, kept so a late vote can be matched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Review>>", optional)]
    pub panel: Vec<Review>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct WorkflowTask {
    pub input: DispatchTask,
    pub stage: String,
    pub status: TaskStatus,
    pub assignment: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub assigned_agent: Option<String>,
    pub history: Vec<WorkflowStep>,
    pub problem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub waiting: Option<Waiting>,
    /// copies: per-item — the item's own folder, once made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub copy: Option<String>,
    /// The current assignment went to the stage's escalation agent.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub escalated: bool,
    /// A panel stage in progress: one entry per reviewer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Review>>", optional)]
    pub reviews: Vec<Review>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct WorkflowRun {
    pub rig: String,
    #[ts(type = "number")]
    pub start_seq: i64,
    pub batch_id: String,
    pub summary: String,
    pub config: Workflow,
    pub tasks: Vec<WorkflowTask>,
    pub assembly: WorkflowTask,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub guard: Option<GuardState>,
}

impl WorkflowRun {
    /// Every item, then the assembly.
    pub fn all_tasks(&self) -> impl Iterator<Item = &WorkflowTask> {
        self.tasks.iter().chain(std::iter::once(&self.assembly))
    }

    pub fn all_tasks_mut(&mut self) -> impl Iterator<Item = &mut WorkflowTask> {
        self.tasks
            .iter_mut()
            .chain(std::iter::once(&mut self.assembly))
    }
}
