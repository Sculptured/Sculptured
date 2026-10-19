//! A team file as a MADL document. What MADL's core can say is said there (agents, groups,
//! connections, the Input, models, budgets, checks, schedules, positions); what only Sculptured
//! uses rides in the `com.usesculptured` extension so a round trip loses nothing.
use super::NS;
use crate::portable::team::TeamFile;
use madl::model as m;
use model::{AgentKind, ConnectionKind, DraftAgent, ResumeBehaviour, TeamDraft};
use serde_json::{json, Map, Value};

/// The Input's id in MADL: `input`, or `input-1`… when an agent or group already uses it.
pub fn input_id(team: &TeamDraft) -> String {
    let taken = |id: &str| team.agents.iter().any(|a| a.id == id) || team.groups.iter().any(|g| g.id == id);
    let mut id = "input".to_string();
    let mut n = 0;
    while taken(&id) {
        n += 1;
        id = format!("input-{n}");
    }
    id
}

pub fn to_madl(file: &TeamFile) -> m::Document {
    let team = &file.team;
    let mut doc = m::Document {
        madl: "draft".into(),
        system: m::System { id: team.id.clone(), name: team.name.clone(), description: None, budget: super::limits::run_out(&team.budget) },
        defaults: defaults(file),
        groups: team.groups.iter().map(group).collect(),
        agents: team.agents.iter().map(|a| agent(team, a, file.settings.own_copies)).collect(),
        connections: team.connections.iter().map(connection).collect(),
        layout: Some(layout(team)),
        extensions: team_extension(file),
        ..Default::default()
    };
    if let Some(input) = &team.input {
        doc.inputs.push(m::Input { id: input_id(team), text: input.text.clone(), to: input.to.clone() });
    }
    doc.policies = policies(team);
    if let Some(w) = &team.workflow {
        super::workflow::render(team, w, &mut doc);
    }
    doc
}

fn defaults(file: &TeamFile) -> Option<m::Settings> {
    let questions = file.settings.question_timeout_min.map(|min| m::Questions { timeout: Some(format!("{min}m")), on_timeout: None });
    let depth = file.team.limits.max_child_levels;
    let preferences = questions.map(|q| m::Preferences { questions: Some(q), ..Default::default() });
    let spawn = depth.map(|d| m::Spawn { max_depth: Some(d), ..Default::default() });
    let restrictions = spawn.map(|s| m::Restrictions { spawn: Some(s), ..Default::default() });
    (preferences.is_some() || restrictions.is_some()).then_some(m::Settings { preferences, restrictions, applies_to: None })
}

fn group(g: &model::DraftGroup) -> m::Group {
    let mut extensions = m::Extensions::new();
    if !g.models.is_empty() {
        extensions.insert(NS.into(), json!({ "models": g.models }));
    }
    m::Group { id: g.id.clone(), name: Some(g.label.clone()), purpose: None, parent: None, settings: None, extensions }
}

pub fn runtime(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "claude-code",
        AgentKind::Codex => "codex",
        AgentKind::Practice => "x-sculptured-practice",
    }
}

fn agent(team: &TeamDraft, a: &DraftAgent, own_copies: bool) -> m::Agent {
    let in_group = a.group_id.as_ref().filter(|g| team.groups.iter().any(|d| &d.id == *g));
    let own = own_copies && a.kind != AgentKind::Practice;
    let workspace = (a.folder.is_some() || own).then(|| m::Workspace { folder: a.folder.clone(), copy: own.then_some(m::CopyMode::Own) });
    m::Agent {
        id: a.id.clone(),
        name: Some(a.name.clone()),
        instructions: Some(a.job.clone()).filter(|j| !j.trim().is_empty()),
        runtime: Some(runtime(a.kind).into()),
        model: team.effective_model(a).map(|id| m::ModelRef::Id(with_provider(a.kind, id))),
        resume: resume(a.advanced.resume),
        groups: in_group.into_iter().cloned().collect(),
        workspace,
        checks: a.advanced.check.iter().map(|c| m::Check { command: c.clone(), after: Some(m::CheckWhen::EachTurn), timeout_s: None }).collect(),
        settings: budget(a),
        data_access: a.advanced.data_access.as_ref().map(|d| m::DataAccess {
            reads: d.reads.clone(), writes: d.writes.clone(), side_effects: d.side_effects.clone(), rationale: d.rationale.clone(),
        }),
        authority: a.advanced.authority.as_ref().map(|x| m::Authority { domains: x.domains.clone(), scope: x.scope.clone(), rationale: x.rationale.clone() }),
        // Embedded, so the file carries the agent's skills wherever it goes.
        skills: a.advanced.skills.iter().map(|s| m::SkillRef::Embedded(m::EmbeddedSkill { name: s.name.clone(), description: s.description.clone(), body: s.body.clone() })).collect(),
        extensions: agent_extension(a, in_group.is_none()),
        ..Default::default()
    }
}

/// MADL names models `provider/model`; Sculptured stores the model alone.
fn with_provider(kind: AgentKind, id: String) -> String {
    let provider = match kind {
        AgentKind::Codex => "openai",
        AgentKind::Claude | AgentKind::Practice => "anthropic",
    };
    if id.contains('/') { id } else { format!("{provider}/{id}") }
}

fn resume(r: ResumeBehaviour) -> Option<m::Resume> {
    match r {
        ResumeBehaviour::ResumeIfPossible => None,
        ResumeBehaviour::RelaunchFresh => Some(m::Resume::RelaunchFresh),
        ResumeBehaviour::CheckpointOnly => Some(m::Resume::CheckpointOnly),
    }
}

fn budget(a: &DraftAgent) -> Option<m::Settings> {
    let budget = super::limits::agent_out(&a.advanced)?;
    let restrictions = m::Restrictions { budget: Some(budget), ..Default::default() };
    Some(m::Settings { restrictions: Some(restrictions), ..Default::default() })
}

/// What only Sculptured knows about an agent: whether its model was its own choice, its
/// permission posture, how its token limit counts, a group that is not drawn.
fn agent_extension(a: &DraftAgent, hidden_group: bool) -> m::Extensions {
    let mut ext = Map::new();
    if a.advanced.model.is_none() {
        ext.insert("model_inherited".into(), Value::Bool(true));
    }
    if a.advanced.permission != model::PermissionPosture::default() {
        ext.insert("permission".into(), json!(a.advanced.permission));
    }
    if let Some(limit) = a.advanced.token_limit.filter(|l| l.counts != model::TokenCount::default()) {
        ext.insert("token_counts".into(), json!(limit.counts));
    }
    if let Some(g) = a.group_id.as_ref().filter(|_| hidden_group) {
        ext.insert("group_id".into(), json!(g));
    }
    if !a.advanced.integrations.is_empty() {
        ext.insert("integrations".into(), json!(a.advanced.integrations));
    }
    if let Some(b) = a.advanced.briefing {
        ext.insert("briefing".into(), json!(b));
    }
    if let Some(shared) = a.advanced.data_access.as_ref().map(|d| &d.shared).filter(|s| !s.is_empty()) {
        ext.insert("shared_values".into(), json!(shared));
    }
    wrap(ext)
}

pub fn wrap(ext: Map<String, Value>) -> m::Extensions {
    let mut out = m::Extensions::new();
    if !ext.is_empty() {
        out.insert(NS.into(), Value::Object(ext));
    }
    out
}

fn connection(c: &model::DraftConnection) -> m::Connection {
    let kind = match c.kind {
        ConnectionKind::HandsWorkTo => m::ConnectionKind::Handoff,
        ConnectionKind::ReportsTo => m::ConnectionKind::Report,
        ConnectionKind::Watches => m::ConnectionKind::Watch,
    };
    super::workflow::plain_connection(Some(c.id.clone()), &c.from, &c.to, kind)
}

fn layout(team: &TeamDraft) -> m::Layout {
    let point = |x: i32, y: i32| m::Point { x: x.into(), y: y.into() };
    let mut nodes: std::collections::BTreeMap<String, m::Point> = team.agents.iter().map(|a| (a.id.clone(), point(a.x, a.y))).collect();
    if let Some(i) = &team.input {
        nodes.insert(input_id(team), point(i.x, i.y));
    }
    let groups = team.groups.iter().map(|g| (g.id.clone(), m::Rect { x: g.x.into(), y: g.y.into(), w: g.w.into(), h: g.h.into() })).collect();
    m::Layout { units: Some("px".into()), nodes, groups }
}

fn policies(team: &TeamDraft) -> Option<m::Policies> {
    let schedules: Vec<m::Schedule> = team.schedules.iter().map(|s| m::Schedule { id: s.id.clone(), agent: s.agent.clone(), every: format!("{}m", s.every_minutes), message: s.message.clone() }).collect();
    (!schedules.is_empty()).then(|| m::Policies { schedules, ..Default::default() })
}

fn team_extension(file: &TeamFile) -> m::Extensions {
    let team = &file.team;
    let mut ext = Map::new();
    let s = &file.settings;
    if let Some(p) = s.posture {
        ext.insert("posture".into(), json!(p));
    }
    if s.own_copies {
        ext.insert("own_copies".into(), Value::Bool(true));
    }
    if !team.models.is_empty() {
        ext.insert("models".into(), json!(team.models));
    }
    if let Some(n) = team.limits.max_agents {
        ext.insert("max_agents".into(), json!(n));
    }
    if team.decision_log {
        ext.insert("decision_log".into(), Value::Bool(true));
    }
    if !team.protection.is_default() {
        ext.insert("protection".into(), json!(team.protection));
    }
    if !team.safety.is_empty() {
        ext.insert("safety".into(), json!(team.safety));
    }
    if !team.components.is_empty() {
        ext.insert("components".into(), json!(team.components));
    }
    if !team.claude_setup.is_default() {
        ext.insert("claude_setup".into(), json!(team.claude_setup));
    }
    // MADL's plan shares say how much a run may add, not that it pauses: kept as Sculptured's own.
    let pauses: Vec<_> = team.budget.plan.iter().filter_map(|p| Some(json!({ "provider": p.provider, "window": p.window, "pause": p.pause? }))).collect();
    if !pauses.is_empty() {
        ext.insert("plan_pause".into(), json!(pauses));
    }
    if let Some(f) = &team.folder {
        ext.insert("folder".into(), json!(f));
    }
    if let Some(w) = &team.workflow {
        ext.insert("workflow".into(), json!(w));
    }
    wrap(ext)
}
