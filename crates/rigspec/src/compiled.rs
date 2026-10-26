use crate::Diagnostic;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Runtime {
    ClaudeCode,
    Codex,
    Opencode,
    Pi,
    Omp,
    Terminal,
    Stub,
}

impl Runtime {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "claude-code" => Self::ClaudeCode,
            "codex" => Self::Codex,
            "opencode" => Self::Opencode,
            "pi" => Self::Pi,
            "omp" => Self::Omp,
            "terminal" => Self::Terminal,
            "stub" => Self::Stub,
            _ => return None,
        })
    }
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::Terminal => "terminal",
            Self::Stub => "stub",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum EdgeKind {
    DelegatesTo,
    SpawnedBy,
    CanObserve,
    CollaboratesWith,
    EscalatesTo,
}

impl EdgeKind {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "delegates_to" => Self::DelegatesTo,
            "spawned_by" => Self::SpawnedBy,
            "can_observe" => Self::CanObserve,
            "collaborates_with" => Self::CollaboratesWith,
            "escalates_to" => Self::EscalatesTo,
            _ => return None,
        })
    }
    /// Only these two have runtime effect in M1: they order launches.
    #[must_use]
    pub fn orders_launch(self) -> bool {
        matches!(self, Self::DelegatesTo | Self::SpawnedBy)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RestorePolicy {
    ResumeIfPossible,
    RelaunchFresh,
    CheckpointOnly,
}

impl RestorePolicy {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "resume_if_possible" => Self::ResumeIfPossible,
            "relaunch_fresh" => Self::RelaunchFresh,
            "checkpoint_only" => Self::CheckpointOnly,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompiledPod {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompiledSeat {
    pub id: String,
    pub pod: String,
    pub member: String,
    pub runtime: Runtime,
    pub cwd: String,
    pub agent_ref: String,
    pub profile: String,
    pub label: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub restore_policy: RestorePolicy,
    pub permission_policy: Option<String>,
    /// Claude Code seats: the person's model connection (an id the gateway keeps). None = the CLI's
    /// own sign-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub connection: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompiledEdge {
    pub kind: EdgeKind,
    pub from: String,
    pub to: String,
    /// True when the runtime acts on this edge (launch order in M1); false = design intent.
    pub enforced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompiledRig {
    pub name: String,
    pub summary: Option<String>,
    pub spec_hash: String,
    pub pods: Vec<CompiledPod>,
    pub seats: Vec<CompiledSeat>,
    pub edges: Vec<CompiledEdge>,
    pub launch_layers: Vec<Vec<String>>,
    /// Warnings and not-enforced notices (never errors: compile fails on errors).
    pub diagnostics: Vec<Diagnostic>,
}
