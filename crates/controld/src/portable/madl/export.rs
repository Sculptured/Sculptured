//! A team as a MADL document, entirely in MADL's core: every part of a team's design has a place
//! there (MEPs 0006–0008), so nothing rides in an extension.
use super::care;
use crate::portable::team::PortableTeam;
use madl::model as m;
use model::{AgentKind, ConnectionKind, DraftAgent, ResumeBehaviour, TeamDraft};

/// Where MADL's JSON Schema is published, written as each document's `$schema`.
pub const MADL_SCHEMA_URL: &str = "https://raw.githubusercontent.com/Alexander-Aghili/Multi-Agent-Descriptor-Language/main/schema/draft/schema.json";

/// The id an input, channel or workflow takes: `wanted`, or `wanted-1`… when something in the team
/// already uses it.
pub fn free_id(team: &TeamDraft, wanted: &str) -> String {
    let taken = |id: &str| {
        team.agents.iter().any(|a| a.id == id)
            || team.groups.iter().any(|g| g.id == id)
            || team.components.iter().any(|c| c.id == id)
    };
    let mut id = wanted.to_string();
    let mut n = 0;
    while taken(&id) {
        n += 1;
        id = format!("{wanted}-{n}");
    }
    id
}

/// The Input's id in MADL.
pub fn input_id(team: &TeamDraft) -> String {
    free_id(team, "input")
}

pub fn to_madl(file: &PortableTeam) -> m::Document {
    let team = &file.team;
    let mut doc = m::Document {
        schema: Some(MADL_SCHEMA_URL.into()),
        madl: "draft".into(),
        system: m::System {
            id: team.id.clone(),
            name: team.name.clone(),
            description: None,
            budget: super::limits::run_out(&team.budget, team.limits.max_agents),
        },
        defaults: care::defaults_out(file),
        groups: groups(team),
        agents: team.agents.iter().map(agent).collect(),
        tools: super::tools::tools_out(team),
        connections: team.connections.iter().map(connection).collect(),
        layout: Some(layout(team)),
        ..Default::default()
    };
    if let Some(input) = &team.input {
        doc.inputs.push(m::Input {
            id: input_id(team),
            text: input.text.clone(),
            to: input.to.clone(),
        });
    }
    if team.decision_log {
        doc.channels.push(m::Channel {
            id: free_id(team, "decisions"),
            medium: "decision-log".into(),
            ..Default::default()
        });
    }
    doc.policies = policies(team);
    if let Some(w) = &team.workflow {
        super::workflow::out(team, w, &mut doc);
    }
    doc
}

/// The drawn groups, then any group an agent names that is not drawn (it has no box in the layout).
fn groups(team: &TeamDraft) -> Vec<m::Group> {
    let mut out: Vec<m::Group> = team
        .groups
        .iter()
        .map(|g| m::Group {
            id: g.id.clone(),
            name: Some(g.label.clone()),
            settings: care::models_settings(&g.models),
            ..Default::default()
        })
        .collect();
    let matching_agents = team.agents.iter().filter_map(|a| a.group_id.as_ref());
    for id in matching_agents {
        if !out.iter().any(|g| &g.id == id) {
            out.push(m::Group {
                id: id.clone(),
                ..Default::default()
            });
        }
    }
    out
}

pub fn runtime(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "claude-code",
        AgentKind::Codex => "codex",
        AgentKind::Opencode => super::care::OPENCODE,
        AgentKind::Practice => "x-sculptured-practice",
    }
}

/// A Practice agent inherits no model: `models` has no entry for its runtime.
fn agent(a: &DraftAgent) -> m::Agent {
    m::Agent {
        id: a.id.clone(),
        name: Some(a.name.clone()),
        instructions: Some(a.job.clone()).filter(|j| !j.trim().is_empty()),
        runtime: Some(runtime(a.kind).into()),
        model: a
            .advanced
            .model
            .clone()
            .map(|id| m::ModelRef::Id(with_provider(a.kind, id))),
        resume: resume(a.advanced.resume),
        groups: a.group_id.iter().cloned().collect(),
        workspace: a.folder.clone().map(|f| m::Workspace {
            folder: Some(f),
            copy: None,
        }),
        checks: a
            .advanced
            .check
            .iter()
            .map(|c| m::Check {
                command: c.clone(),
                after: Some(m::CheckWhen::EachTurn),
                timeout_s: None,
            })
            .collect(),
        settings: care::agent_settings_out(a),
        data_access: a.advanced.data_access.as_ref().map(|d| m::DataAccess {
            reads: d.reads.clone(),
            writes: d.writes.clone(),
            side_effects: d.side_effects.clone(),
            rationale: d.rationale.clone(),
            shared: d
                .shared
                .iter()
                .map(|s| m::SharedValue {
                    name: s.name.clone(),
                    value: s.value.clone(),
                    file: s.file.clone(),
                })
                .collect(),
        }),
        authority: a.advanced.authority.as_ref().map(|x| m::Authority {
            domains: x.domains.clone(),
            scope: x.scope.clone(),
            rationale: x.rationale.clone(),
        }),
        // Embedded, so the document carries the agent's skills wherever it goes.
        skills: a
            .advanced
            .skills
            .iter()
            .map(|s| {
                m::SkillRef::Embedded(m::EmbeddedSkill {
                    name: s.name.clone(),
                    description: s.description.clone(),
                    body: s.body.clone(),
                    source: s.source.clone(),
                })
            })
            .collect(),
        tools: a
            .advanced
            .integrations
            .iter()
            .map(|id| m::ToolRef::Ref(id.clone()))
            .collect(),
        ..Default::default()
    }
}

/// MADL names models `provider/model`; Sculptured stores the model alone.
pub fn with_provider(kind: AgentKind, id: String) -> String {
    let provider = match kind {
        AgentKind::Codex => "openai",
        AgentKind::Opencode => "openrouter",
        AgentKind::Claude | AgentKind::Practice => "anthropic",
    };
    if id.contains('/') {
        id
    } else {
        format!("{provider}/{id}")
    }
}

fn resume(r: ResumeBehaviour) -> Option<m::Resume> {
    match r {
        ResumeBehaviour::ResumeIfPossible => None,
        ResumeBehaviour::RelaunchFresh => Some(m::Resume::RelaunchFresh),
        ResumeBehaviour::CheckpointOnly => Some(m::Resume::CheckpointOnly),
    }
}

pub fn plain_connection(
    id: Option<String>,
    from: &str,
    to: &str,
    kind: m::ConnectionKind,
) -> m::Connection {
    m::Connection {
        id,
        from: from.into(),
        to: to.into(),
        kind,
        direction: None,
        via: None,
        protocol: None,
        initiates: None,
        mode: None,
        carries: Vec::new(),
        max_rounds: None,
        when: None,
        pattern: None,
        extensions: m::Extensions::new(),
    }
}

fn connection(c: &model::DraftConnection) -> m::Connection {
    let kind = match c.kind {
        ConnectionKind::HandsWorkTo => m::ConnectionKind::Handoff,
        ConnectionKind::ReportsTo => m::ConnectionKind::Report,
        ConnectionKind::Watches => m::ConnectionKind::Watch,
    };
    plain_connection(Some(c.id.clone()), &c.from, &c.to, kind)
}

fn layout(team: &TeamDraft) -> m::Layout {
    let point = |x: i32, y: i32| m::Point {
        x: x.into(),
        y: y.into(),
    };
    let mut nodes: std::collections::BTreeMap<String, m::Point> = team
        .agents
        .iter()
        .map(|a| (a.id.clone(), point(a.x, a.y)))
        .collect();
    if let Some(i) = &team.input {
        nodes.insert(input_id(team), point(i.x, i.y));
    }
    let groups = team
        .groups
        .iter()
        .map(|g| {
            (
                g.id.clone(),
                m::Rect {
                    x: g.x.into(),
                    y: g.y.into(),
                    w: g.w.into(),
                    h: g.h.into(),
                },
            )
        })
        .collect();
    m::Layout {
        units: Some("px".into()),
        nodes,
        groups,
        stages: Default::default(),
    }
}

fn policies(team: &TeamDraft) -> Option<m::Policies> {
    let schedules: Vec<m::Schedule> = team
        .schedules
        .iter()
        .map(|s| m::Schedule {
            id: s.id.clone(),
            agent: s.agent.clone(),
            every: format!("{}m", s.every_minutes),
            message: s.message.clone(),
        })
        .collect();
    let guards: Vec<m::Guard> = team
        .workflow
        .iter()
        .filter_map(|w| w.guard.as_ref())
        .map(|g| m::Guard {
            command: g.command.clone(),
            every: format!("{}m", g.every_minutes),
            timeout_s: g.timeout_s,
        })
        .collect();
    (!schedules.is_empty() || !guards.is_empty()).then(|| m::Policies {
        schedules,
        guards,
        ..Default::default()
    })
}
