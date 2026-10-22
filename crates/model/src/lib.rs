mod group_block;
pub use group_block::{BlockSource, GroupBlock};
mod action;
mod budget;
pub mod cap;
mod caps;
pub mod component;
pub mod contract;
pub mod protection;
pub mod skill;
pub use component::{Component, ComponentKind};
pub mod safety;
pub use budget::{
    ClaudeSetup, ContinueMode, PlanBudget, PlanPause, RunBudget, SOFT_SHARE, SoftHard,
};
pub use contract::{Authority, DataAccess, SharedValue};
pub use protection::{GuardRules, Protection};
pub use safety::SafetyRule;
pub use skill::Skill;
mod dispatch;
mod draft;
mod event;
mod ids;
mod limit;
mod question;
mod report;
mod status;
mod workflow;
pub use dispatch::{DispatchRecord, WorkflowDispatch};
mod briefing;
pub use briefing::BriefingLevel;
pub mod pricing;
pub use pricing::{Cost, CostSource};
pub use workflow::{
    ASSEMBLY, BoardPosition, CopiesMode, DEFAULT_CHECK_TIMEOUT_S, DispatchTask, Escalation, Guard,
    GuardState, Join, Review, ReviewPersona, StageKind, StepBy, TaskStatus, TopicMode, Waiting,
    Workflow, WorkflowRun, WorkflowStage, WorkflowStep, WorkflowTask,
};

pub use action::{ActionClass, RiskFlag};
pub use caps::Caps;
pub use draft::{
    AgentAdvanced, AgentKind, ConnectionKind, DEFAULT_GROUP_ID, DraftAgent, DraftConnection,
    DraftGroup, DraftIssue, ElementKind, ElementRef, IssueSeverity, ModelDefaults,
    PermissionPosture, ResumeBehaviour, Schedule, TeamDraft, TeamInput, TeamLimits, TokenCount,
    TokenLimit,
};
pub use event::{
    Actor, ContextBasis, EventBody, EventRecord, HandoffSource, HandoffStuck, Provenance, Subject,
    ThinkingForm, Wave,
};
pub use ids::{IdError, SeatId, valid_segment};
pub use limit::{LimitKind, MAX_RESETS, PlanUsage, PlanWindow, limit_resets};
pub use question::{
    Answer, DecisionSource, QuestionOption, QuestionPrompt, QuestionSource, QuestionState,
    RestoreDecision, SessionMode, Urgency, timeout_instruction,
};
pub use report::{Finding, MAX_FINDING, MAX_FINDINGS, ReportVerdict, cap_findings};
pub use status::{
    Desired, MessageIntent, MessageState, OutcomeEvidence, SeatActivity, SeatLifecycle,
    TenureCause, TurnStatus, WorkState,
};

/// Milliseconds since the Unix epoch.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as i64
}
