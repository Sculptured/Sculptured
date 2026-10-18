//! The team graph the builder edits. Ids are assigned once and never change; names are
//! free text and reach the RigSpec only as labels.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The group (pod) of agents that have no explicit group.
pub const DEFAULT_GROUP_ID: &str = "team";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum AgentKind {
    Claude,
    Codex,
    /// Runs the stub driver: no model calls, no cost.
    Practice,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum PermissionPosture {
    #[default]
    Unrestricted,
    Restricted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ResumeBehaviour {
    #[default]
    ResumeIfPossible,
    RelaunchFresh,
    CheckpointOnly,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct AgentAdvanced {
    /// This agent's own model, overriding its group's and team's default for its kind. None = inherit.
    pub model: Option<String>,
    #[serde(default)]
    pub permission: PermissionPosture,
    #[serde(default)]
    pub resume: ResumeBehaviour,
    /// Tokens this agent may use in one run; reaching it soft-stops the agent. None = no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub token_limit: Option<TokenLimit>,
    /// Turns this agent may take in one run (its children's included).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub turn_limit: Option<crate::SoftHard<u32>>,
    /// Minutes this agent may spend working in one run (its children's included).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub minute_limit: Option<crate::SoftHard<u32>>,
    /// A shell command Sculptured runs in the agent's folder after each turn it finishes (exit 0 =
    /// passed). None = no check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub check: Option<String>,
    /// What this agent reads and writes, and what else it changes (MADL `data_access`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub data_access: Option<crate::DataAccess>,
    /// The decisions this agent owns (MADL `authority`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub authority: Option<crate::Authority>,
    /// This agent's skills; only their names and descriptions are in its context until it uses one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::Skill>>", optional)]
    pub skills: Vec<crate::Skill>,
    /// The components it is given, by id: built in (`kindex`, `signet`) or one of the team's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub integrations: Vec<String>,
}

/// Limits a person set on a team's spawned agents. None = no limit; Sculptured sets none of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct TeamLimits {
    /// Most agents in the team at once, its own and spawned children together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_agents: Option<u32>,
    /// Most levels of children: 1 lets the team's agents spawn children that cannot spawn their own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_child_levels: Option<u32>,
}

impl TeamLimits {
    pub fn is_empty(&self) -> bool {
        self.max_agents.is_none() && self.max_child_levels.is_none()
    }
}

/// A per-agent token limit for one run (since the team started).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct TokenLimit {
    /// The hard value: reaching it stops the agent at once.
    #[ts(type = "number")]
    pub tokens: u64,
    /// The soft value: reaching it asks the agent to wrap up. Absent = 85% of `tokens`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub soft: Option<u64>,
    #[serde(default)]
    pub counts: TokenCount,
    /// Count the agents it spawns (and theirs) toward this agent's limit.
    #[serde(default = "yes")]
    pub include_subagents: bool,
}

impl TokenLimit {
    pub fn soft_value(&self) -> u64 {
        crate::SoftHard { soft: self.soft, hard: self.tokens }.soft_value()
    }
}

fn yes() -> bool {
    true
}

/// Which reported tokens count toward a limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TokenCount {
    /// Input, output, cache reads and cache writes.
    #[default]
    All,
    /// Everything except cache reads.
    New,
    Output,
}

impl TokenCount {
    /// The counted share of `[input, output, cache_read, cache_write]`.
    pub fn of(self, t: [u64; 4]) -> u64 {
        match self {
            TokenCount::All => t.iter().sum(),
            TokenCount::New => t[0] + t[1] + t[3],
            TokenCount::Output => t[1],
        }
    }
}

/// Per-provider model defaults for a team or a group. None = inherit (group, then team, then the
/// CLI's own default). An agent keeps its Claude-or-Codex kind and may override with `AgentAdvanced.model`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct ModelDefaults {
    /// For Claude agents: an alias (`opus`, `sonnet`) or a full model id.
    #[serde(default)]
    pub claude: Option<String>,
    /// For Codex agents: a model slug (e.g. `gpt-6-sol`).
    #[serde(default)]
    pub codex: Option<String>,
}

impl ModelDefaults {
    /// The default for one kind of agent; Practice agents never have one.
    pub fn for_kind(&self, kind: AgentKind) -> Option<&str> {
        match kind {
            AgentKind::Claude => self.claude.as_deref(),
            AgentKind::Codex => self.codex.as_deref(),
            AgentKind::Practice => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.claude.is_none() && self.codex.is_none()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct DraftAgent {
    /// Immutable; becomes the RigSpec member id.
    pub id: String,
    /// None = the default group (`DEFAULT_GROUP_ID`).
    pub group_id: Option<String>,
    pub name: String,
    pub kind: AgentKind,
    /// Plain-language instructions, sent as the agent's first message at start.
    #[serde(default)]
    pub job: String,
    /// Absolute path. Practice agents may leave it empty (they use the home folder).
    pub folder: Option<String>,
    pub x: i32,
    pub y: i32,
    #[serde(default)]
    pub advanced: AgentAdvanced,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct DraftGroup {
    /// Immutable; becomes the RigSpec pod id.
    pub id: String,
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// This group's model per provider; absent = inherit the team's.
    #[serde(default, skip_serializing_if = "ModelDefaults::is_empty")]
    #[ts(as = "Option<ModelDefaults>", optional)]
    pub models: ModelDefaults,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ConnectionKind {
    /// `delegates_to`: the giver starts before the receiver.
    HandsWorkTo,
    /// `escalates_to`: design intent, not enforced.
    ReportsTo,
    /// `can_observe`: design intent, not enforced.
    Watches,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct DraftConnection {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: ConnectionKind,
}

/// The project's Input box on the Design board: the goal, links and notes the agents it points
/// to get with their Job when the team starts. (A workflow project uses its workflow's Input.)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct TeamInput {
    pub text: String,
    pub x: i32,
    pub y: i32,
    /// The agents (ids) it hands to.
    #[serde(default)]
    pub to: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct TeamDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub workflow: Option<crate::Workflow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input: Option<TeamInput>,
    /// Default working folder for agents without their own override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub folder: Option<String>,
    /// Immutable; becomes the rig name.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub groups: Vec<DraftGroup>,
    #[serde(default)]
    pub agents: Vec<DraftAgent>,
    #[serde(default)]
    pub connections: Vec<DraftConnection>,
    /// The team's model per provider; absent = the CLI's own default.
    #[serde(default, skip_serializing_if = "ModelDefaults::is_empty")]
    #[ts(as = "Option<ModelDefaults>", optional)]
    pub models: ModelDefaults,
    /// Messages sent to an agent every N minutes while the team runs (Advanced).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Schedule>>", optional)]
    pub schedules: Vec<Schedule>,
    /// Limits on the agents this team's agents spawn (Advanced). Absent = no limits.
    #[serde(default, skip_serializing_if = "TeamLimits::is_empty")]
    #[ts(as = "Option<TeamLimits>", optional)]
    pub limits: TeamLimits,
    /// Which of the person's own Claude Code setup Claude agents load (Advanced).
    #[serde(default, skip_serializing_if = "crate::ClaudeSetup::is_default")]
    #[ts(as = "Option<crate::ClaudeSetup>", optional)]
    pub claude_setup: crate::ClaudeSetup,
    /// Rules between this team's agents and their tools (Advanced). On by default.
    #[serde(default, skip_serializing_if = "crate::Protection::is_default")]
    #[ts(as = "Option<crate::Protection>", optional)]
    pub protection: crate::Protection,
    /// The team's safety rules (Advanced → Protections): tool calls they match are refused.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::SafetyRule>>", optional)]
    pub safety: Vec<crate::SafetyRule>,
    /// Components this team adds to Sculptured's built-in ones (from Community, or written by hand).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::Component>>", optional)]
    pub components: Vec<crate::Component>,
    /// What one run may use (Advanced). Absent = no budget.
    #[serde(default, skip_serializing_if = "crate::RunBudget::is_empty")]
    #[ts(as = "Option<crate::RunBudget>", optional)]
    pub budget: crate::RunBudget,
    /// People keep a decision log that agents read with read_decisions (Advanced).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub decision_log: bool,
    /// Server-assigned save counter for optimistic concurrency.
    #[ts(type = "number")]
    pub revision: u64,
}

impl TeamDraft {
    /// A component by id: one of the team's, or a built-in one.
    pub fn component(&self, id: &str) -> Option<crate::Component> {
        self.components.iter().find(|c| c.id == id).cloned().or_else(|| crate::component::builtins().into_iter().find(|c| c.id == id))
    }

    /// The components an agent is given, with those they require first, each once.
    pub fn agent_components(&self, a: &DraftAgent) -> Vec<crate::Component> {
        let mut out: Vec<crate::Component> = Vec::new();
        fn add(d: &TeamDraft, id: &str, out: &mut Vec<crate::Component>, depth: u8) {
            if depth > 4 || out.iter().any(|c| c.id == id) {
                return;
            }
            let Some(c) = d.component(id) else { return };
            for r in &c.requires {
                add(d, r, out, depth + 1);
            }
            if !out.iter().any(|o| o.id == c.id) {
                out.push(c);
            }
        }
        for id in &a.advanced.integrations {
            add(self, id, &mut out, 0);
        }
        out
    }

    /// The agent's own skills, then those its skills components add; a name it already has is kept
    /// as its own.
    pub fn agent_skills(&self, a: &DraftAgent) -> Vec<crate::Skill> {
        let mut out = a.advanced.skills.clone();
        for s in self.agent_components(a).into_iter().filter(|c| c.kind == crate::ComponentKind::Skills).flat_map(|c| c.skills) {
            if !out.iter().any(|o| o.name == s.name) {
                out.push(s);
            }
        }
        out
    }

    pub fn effective_folder<'a>(&'a self, a: &'a DraftAgent) -> Option<&'a str> {
        a.folder.as_deref().map(str::trim).filter(|s| !s.is_empty())
            .or_else(|| self.folder.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    }

/// The agent's Job, with the project's Input after it when the Input points to the agent
    /// (only the Input when it has no Job). Empty when it has neither.
    pub fn job_with_input(&self, a: &DraftAgent) -> String {
        let job = a.job.trim();
        let input = self.input.as_ref().filter(|i| i.to.contains(&a.id)).map(|i| i.text.trim()).filter(|t| !t.is_empty());
        match input {
            Some(text) if job.is_empty() => text.to_string(),
            Some(text) => format!("{job}\n\n--- Input ---\n{text}"),
            None => job.to_string(),
        }
    }

    pub fn agent(&self, id: &str) -> Option<&DraftAgent> {
        self.agents.iter().find(|a| a.id == id)
    }

    /// The pod id an agent compiles into.
    pub fn pod_of<'a>(&self, a: &'a DraftAgent) -> &'a str {
        a.group_id.as_deref().unwrap_or(DEFAULT_GROUP_ID)
    }

    /// The model an agent runs with: its own, else its group's for its kind, else the team's.
    /// None = the CLI's own default. Practice agents inherit nothing.
    pub fn effective_model(&self, a: &DraftAgent) -> Option<String> {
        let clean = |m: Option<&str>| m.map(str::trim).filter(|m| !m.is_empty()).map(str::to_string);
        let group = self.groups.iter().find(|g| Some(g.id.as_str()) == a.group_id.as_deref());
        clean(a.advanced.model.as_deref())
            .or_else(|| clean(group.and_then(|g| g.models.for_kind(a.kind))))
            .or_else(|| clean(self.models.for_kind(a.kind)))
    }

    /// `pod.member@rig` for an agent, from ids only.
    pub fn seat_id(&self, agent_id: &str) -> Option<String> {
        let a = self.agent(agent_id)?;
        Some(format!("{}.{}@{}", self.pod_of(a), a.id, self.id))
    }

    /// The design's agent that runs as `seat` (`pod.member@rig`).
    pub fn agent_for_seat(&self, seat: &str) -> Option<&DraftAgent> {
        self.agents.iter().find(|a| self.seat_id(&a.id).as_deref() == Some(seat))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ElementKind {
    Team,
    Group,
    Agent,
    Connection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct ElementRef {
    pub kind: ElementKind,
    pub id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum IssueSeverity {
    /// Blocks Start team.
    Error,
    /// Shown, does not block.
    Warning,
}

/// One validation finding, pinned to a draft element, in plain words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct DraftIssue {
    pub element: ElementRef,
    pub severity: IssueSeverity,
    /// Stable machine code, e.g. `folder-missing`.
    pub code: String,
    /// Plain-language sentence naming the element.
    pub message: String,
    /// Technical message, shown only under Advanced.
    pub detail: Option<String>,
    /// RigSpec path the detail refers to, shown only under Advanced.
    pub spec_path: Option<String>,
}

/// A message sent to `agent` every `every_minutes` while the team runs; skipped when the agent is busy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Schedule { pub id: String, pub agent: String, pub every_minutes: u32, pub message: String }
