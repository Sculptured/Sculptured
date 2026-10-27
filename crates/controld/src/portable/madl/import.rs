//! A MADL document as a team. MADL can describe more than Sculptured runs (people as participants,
//! channels, bans, other runtimes); each part that can't be kept is listed with its JSON Pointer,
//! like the OpenRig import does.
use super::{care, limits, workflow};
use crate::portable::team::{PortableSettings, PortableTeam};
use madl::model as m;

use model::{
    AgentKind, Component, DraftGroup, ModelDefaults, RunBudget, Schedule, TeamDraft, TeamLimits,
};
use rigspec::{Diagnostic, Severity};

#[derive(Debug)]
pub struct Imported {
    pub file: PortableTeam,
    /// What MADL said that Sculptured could not keep.
    pub dropped: Vec<Diagnostic>,
}

/// `doc` must already be valid and expanded.
#[must_use]
pub fn from_madl(doc: &m::Document) -> Imported {
    let resolved = madl::resolve(doc);
    let mut cx = Cx {
        doc,
        dropped: Vec::new(),
        components: Vec::new(),
    };
    let (budget, dropped) = limits::run_in(doc.system.budget.as_ref());
    cx.drops(dropped);
    let (workflow, dropped) = workflow::workflow_in(doc);
    cx.drops(dropped);
    cx.tools();
    let mut team = TeamDraft {
        budget,
        workflow,
        input: None,
        id: doc.system.id.clone(),
        name: doc.system.name.clone(),
        groups: doc
            .groups
            .iter()
            .enumerate()
            .filter_map(|(i, g)| cx.group(i, g))
            .collect(),
        agents: Vec::new(),
        connections: Vec::new(),
        schedules: cx.schedules(),
        limits: TeamLimits {
            max_agents: doc.system.budget.as_ref().and_then(|b| b.max_agents),
            max_child_levels: spawn_depth(doc),
        },
        mandate: Default::default(),
        decision_log: doc.channels.iter().any(|c| c.medium == "decision-log"),
        revision: 0,
        ..cx.defaults()
    };
    team.agents = doc
        .agents
        .iter()
        .zip(&resolved.agents)
        .enumerate()
        .filter_map(|(i, (a, r))| cx.agent(i, a, r))
        .collect();
    team.connections = doc
        .connections
        .iter()
        .enumerate()
        .flat_map(|(i, c)| cx.connection(i, c, &team))
        .collect();
    team.input = cx.input(&team);
    cx.unkept();
    team.components = std::mem::take(&mut cx.components);
    let settings = cx.settings();
    Imported {
        file: PortableTeam { team, settings },
        dropped: cx.dropped,
    }
}

fn spawn_depth(doc: &m::Document) -> Option<u32> {
    doc.defaults
        .as_ref()?
        .restrictions
        .as_ref()?
        .spawn?
        .max_depth
}

pub(super) struct Cx<'a> {
    pub(super) doc: &'a m::Document,
    pub(super) dropped: Vec<Diagnostic>,
    /// The team's components: the document's tools, then tools written on agents.
    pub(super) components: Vec<Component>,
}

impl Cx<'_> {
    pub(super) fn drop(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.dropped.push(Diagnostic {
            path: path.into(),
            severity: Severity::Warning,
            message: message.into(),
        });
    }

    fn drops(&mut self, list: Vec<(String, String)>) {
        for (path, why) in list {
            self.drop(path, why);
        }
    }

    fn preferences(&self) -> Option<&m::Preferences> {
        self.doc.defaults.as_ref()?.preferences.as_ref()
    }

    pub(super) fn models_of(&mut self, at: &str, s: Option<&m::Settings>) -> ModelDefaults {
        let (models, others) = care::models_in(s);
        for r in others {
            self.drop(
                format!("{at}/preferences/models/{r}"),
                format!(
                    "Sculptured runs Claude Code and Codex agents; the model for `{r}` was not kept"
                ),
            );
        }
        models
    }

    /// Team-wide settings from `defaults`; the rest of the draft is filled in by the caller.
    fn defaults(&mut self) -> TeamDraft {
        let models = self.models_of("/defaults", self.doc.defaults.as_ref());
        let p = self.preferences();
        let tools = self
            .doc
            .defaults
            .as_ref()
            .and_then(|d| d.restrictions.as_ref()?.tools.as_ref());
        TeamDraft {
            models,
            folder: p.and_then(|p| p.workspace.as_ref()?.folder.clone()),
            protection: care::protection_in(p.and_then(|p| p.context.as_ref())),
            claude_setup: care::setup_in(p.and_then(|p| p.config.as_ref())),
            safety: tools
                .map(|t| t.refuse.iter().map(care::rule_in).collect())
                .unwrap_or_default(),
            workflow: None,
            input: None,
            id: String::new(),
            name: String::new(),
            groups: Vec::new(),
            agents: Vec::new(),
            connections: Vec::new(),
            schedules: Vec::new(),
            limits: TeamLimits::default(),
            components: Vec::new(),
            budget: RunBudget::default(),
            mandate: Default::default(),
            decision_log: false,
            revision: 0,
        }
    }

    /// The document's tools as components; one exactly like a built-in one is that one.
    fn tools(&mut self) {
        let indexed_tools = self.doc.tools.iter().enumerate();
        for (i, t) in indexed_tools {
            match super::tools::tool_in(t) {
                Ok(c) if super::tools::is_builtin_as_shipped(&c) => {}
                Ok(c) => self.components.push(c),
                Err(why) => self.drop(
                    format!("/tools/{i}"),
                    format!("Tool `{}` {why}; it was not kept", t.id),
                ),
            }
        }
    }

    /// A drawn group; a group with no box in the layout is only a name its agents share.
    fn group(&mut self, i: usize, g: &m::Group) -> Option<DraftGroup> {
        if g.parent.is_some() {
            self.drop(
                format!("/groups/{i}/parent"),
                format!(
                    "Group `{}` was inside another; Sculptured's groups sit side by side",
                    g.id
                ),
            );
        }
        let rect = self
            .doc
            .layout
            .as_ref()
            .map(|l| l.groups.get(&g.id).copied());
        let r = match rect {
            Some(None) => return None,
            Some(Some(r)) => r,
            None => m::Rect {
                x: 40.0 + 560.0 * i as f64,
                y: 40.0,
                w: 520.0,
                h: 360.0,
            },
        };
        let models = self.models_of(&format!("/groups/{i}/settings"), g.settings.as_ref());
        let label = g.name.clone().unwrap_or_else(|| g.id.clone());
        Some(DraftGroup {
            id: g.id.clone(),
            label,
            x: r.x as i32,
            y: r.y as i32,
            w: r.w as i32,
            h: r.h as i32,
            models,
        })
    }

    fn schedules(&self) -> Vec<Schedule> {
        let list = self.doc.policies.iter().flat_map(|p| p.schedules.iter());
        let minutes = |every: &str| madl_minutes(every).unwrap_or(60);
        list.map(|s| Schedule {
            id: s.id.clone(),
            agent: s.agent.clone(),
            every_minutes: minutes(&s.every),
            message: s.message.clone(),
        })
        .collect()
    }

    fn settings(&mut self) -> PortableSettings {
        let p = self.preferences();
        let timeout = p.and_then(|p| p.questions.as_ref()?.timeout.clone());
        let all_own = !self.doc.agents.is_empty()
            && self
                .doc
                .agents
                .iter()
                .all(|a| a.workspace.as_ref().and_then(|w| w.copy) == Some(m::CopyMode::Own));
        let own_copies =
            all_own || p.and_then(|p| p.workspace.as_ref()?.copy) == Some(m::CopyMode::Own);
        let approvals = self
            .doc
            .defaults
            .as_ref()
            .and_then(|d| d.restrictions.as_ref()?.approvals.as_ref());
        let (posture, others) = care::posture_in(approvals);
        if !others.is_empty() {
            self.drop("/defaults/restrictions/approvals/required_for", format!("Sculptured asks a person about what the agent's runtime asks permission for; approvals for {} were not kept", others.join(", ")));
        }
        PortableSettings {
            posture,
            own_copies,
            question_timeout_min: timeout.as_deref().and_then(madl_minutes),
        }
    }
}

/// Whole minutes in a MADL duration, at least 1.
pub fn madl_minutes(every: &str) -> Option<u32> {
    let secs = madl::duration::seconds(every)?;
    Some(u32::try_from(secs.div_ceil(60)).unwrap_or(u32::MAX).max(1))
}

pub(super) fn kind_of(runtime: Option<&str>) -> Option<AgentKind> {
    match runtime {
        Some("claude-code") => Some(AgentKind::Claude),
        Some("codex") => Some(AgentKind::Codex),
        Some(super::care::OPENCODE | "opencode") => Some(AgentKind::Opencode),
        Some("x-sculptured-practice") => Some(AgentKind::Practice),
        _ => None,
    }
}
