mod group_block;
pub use group_block::{BlockSource, GroupBlock};
mod action;
pub mod cap;
mod caps;
mod budget;
pub mod protection;
pub mod contract;
pub mod skill;
pub mod component;
pub use component::{Component, ComponentKind};
pub mod safety;
pub use safety::SafetyRule;
pub use skill::Skill;
pub use contract::{Authority, DataAccess, SharedValue};
pub use protection::{GuardRules, Protection};
pub use budget::{ClaudeSetup, ContinueMode, PlanBudget, PlanPause, RunBudget, SoftHard, SOFT_SHARE};
mod draft;
mod event;
mod ids;
mod limit;
mod question;
mod report;
mod status;
mod workflow;
pub mod pricing;
pub use pricing::{Cost, CostSource};
pub use workflow::{ReviewPersona, Escalation, Guard, GuardState, TaskStatus, ASSEMBLY, BoardPosition, CopiesMode, DispatchTask, Join, Review, StageKind, StepBy, TopicMode, Waiting, Workflow, WorkflowRun, WorkflowStage, WorkflowStep, WorkflowTask, DEFAULT_CHECK_TIMEOUT_S};

pub use action::{ActionClass, RiskFlag};
pub use question::{
    timeout_instruction, Answer, DecisionSource, QuestionOption, QuestionPrompt, QuestionSource, QuestionState, Urgency,
    RestoreDecision, SessionMode,
};
pub use report::{cap_findings, Finding, ReportVerdict, MAX_FINDING, MAX_FINDINGS};
pub use caps::Caps;
pub use draft::{Schedule, TeamLimits, TokenCount, TokenLimit, 
    AgentAdvanced, AgentKind, ConnectionKind, DraftAgent, DraftConnection, DraftGroup, DraftIssue, ElementKind,
    ElementRef, IssueSeverity, ModelDefaults, PermissionPosture, ResumeBehaviour, TeamDraft, TeamInput, DEFAULT_GROUP_ID,
};
pub use event::{
    Actor, ContextBasis, EventBody, EventRecord, HandoffSource, HandoffStuck, Provenance, Subject, ThinkingForm, Wave,
};
pub use ids::{valid_segment, IdError, SeatId};
pub use limit::{limit_resets, LimitKind, PlanUsage, PlanWindow, MAX_RESETS};
pub use status::{
    Desired, MessageIntent, MessageState, OutcomeEvidence, SeatActivity, SeatLifecycle, TenureCause, TurnStatus,
    WorkState,
};

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as i64
}
