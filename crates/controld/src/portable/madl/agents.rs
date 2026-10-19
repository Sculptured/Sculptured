//! Agents, for [`super::import::from_madl`].
use super::care::{self, RUNTIME_PERMISSIONS};
use super::import::{kind_of, Cx};
use madl::model as m;
use madl::resolve::AgentSettings;
use model::*;

impl Cx<'_> {
    pub(super) fn agent(&mut self, i: usize, a: &m::Agent, r: &AgentSettings) -> Option<DraftAgent> {
        let at = format!("/agents/{i}");
        let runtime = r.settings.preferences.runtime.as_deref();
        if runtime == Some("human") {
            self.drop(&at, format!("`{}` is a person; Sculptured's board holds agents, and people answer from the app", a.id));
            return None;
        }
        let kind = kind_of(runtime).unwrap_or_else(|| {
            let was = runtime.map_or("no runtime".to_string(), |r| format!("runtime `{r}`"));
            self.drop(format!("{at}/runtime"), format!("`{}` has {was}; it was imported as a Claude Code agent", a.id));
            AgentKind::Claude
        });
        let (x, y) = self.position(&a.id, i);
        Some(DraftAgent {
            id: a.id.clone(),
            group_id: self.group_of(&at, a),
            name: a.name.clone().unwrap_or_else(|| a.id.clone()),
            kind,
            job: a.instructions.clone().unwrap_or_default(),
            folder: a.workspace.as_ref().and_then(|w| w.folder.clone()),
            x,
            y,
            advanced: self.advanced(&at, i, a, r),
        })
    }

    fn group_of(&mut self, at: &str, a: &m::Agent) -> Option<String> {
        if a.groups.len() > 1 {
            self.drop(format!("{at}/groups"), format!("`{}` was in {} groups; it was put in `{}`, its first", a.id, a.groups.len(), a.groups[0]));
        }
        a.groups.first().cloned()
    }

    fn position(&self, id: &str, i: usize) -> (i32, i32) {
        match self.doc.layout.as_ref().and_then(|l| l.nodes.get(id)) {
            Some(p) => (p.x as i32, p.y as i32),
            None => (40 + 260 * (i % 4) as i32, 40 + 180 * (i / 4) as i32),
        }
    }

    fn advanced(&mut self, at: &str, i: usize, a: &m::Agent, r: &AgentSettings) -> AgentAdvanced {
        let (token_limit, turn_limit, minute_limit) = super::limits::agent_in(r.settings.restrictions.budget.as_ref());
        if a.checks.len() > 1 {
            self.drop(format!("{at}/checks/1"), "Sculptured runs one check per agent; the first was kept");
        }
        self.unkept_agent_parts(at, a);
        AgentAdvanced {
            turn_limit,
            minute_limit,
            model: self.model(at, i, r),
            permission: self.permission(a),
            resume: match r.settings.preferences.resume {
                Some(m::Resume::RelaunchFresh) => ResumeBehaviour::RelaunchFresh,
                Some(m::Resume::CheckpointOnly) => ResumeBehaviour::CheckpointOnly,
                Some(m::Resume::ResumeIfPossible) | None => ResumeBehaviour::ResumeIfPossible,
            },
            token_limit,
            check: a.checks.first().map(|c| c.command.clone()),
            data_access: a.data_access.as_ref().map(|d| model::DataAccess {
                reads: d.reads.clone(), writes: d.writes.clone(), side_effects: d.side_effects.clone(), rationale: d.rationale.clone(),
                shared: d.shared.iter().map(|s| model::SharedValue { name: s.name.clone(), value: s.value.clone(), file: s.file.clone() }).collect(),
            }),
            authority: a.authority.as_ref().map(|x| model::Authority { domains: x.domains.clone(), scope: x.scope.clone(), rationale: x.rationale.clone() }),
            skills: self.skills(at, a),
            integrations: self.integrations(at, a),
        }
    }

    /// Restricted when the agent itself or one of its groups has a person approve what its runtime asks.
    fn permission(&self, a: &m::Agent) -> PermissionPosture {
        let asks = |s: Option<&m::Settings>| {
            let list = s.and_then(|s| s.restrictions.as_ref()?.approvals.as_ref()?.required_for.as_ref());
            list.is_some_and(|l| l.iter().any(|x| x == RUNTIME_PERMISSIONS))
        };
        let groups = self.doc.groups.iter().filter(|g| a.groups.contains(&g.id)).any(|g| asks(g.settings.as_ref()));
        if groups || asks(a.settings.as_ref()) { PermissionPosture::Restricted } else { PermissionPosture::Unrestricted }
    }

    /// The agent's own model, or one a group or the defaults set with `model`; a model that comes
    /// from `models` is inherited, as Sculptured's team and group models are.
    fn model(&mut self, at: &str, i: usize, r: &AgentSettings) -> Option<String> {
        let model = r.settings.preferences.model.as_ref()?;
        if let m::ModelRef::WithFallback { fallback, .. } = model {
            if !fallback.is_empty() {
                self.drop(format!("{at}/model"), "Sculptured uses one model per agent; the fallbacks were not kept");
            }
        }
        let source = r.settings.sources.get("preferences.model").map(String::as_str).unwrap_or("");
        let own = source.starts_with(&format!("/agents/{i}/"));
        (own || !source.contains("/preferences/models/")).then(|| care::bare(care::model_id(model)))
    }

    /// Embedded skills are kept; a skill named by path or name is not, since the file does not carry it.
    fn skills(&mut self, at: &str, a: &m::Agent) -> Vec<model::Skill> {
        let mut out = Vec::new();
        for (i, s) in a.skills.iter().enumerate() {
            match s {
                m::SkillRef::Embedded(e) => out.push(model::Skill { name: e.name.clone(), description: e.description.clone(), body: e.body.clone() }),
                m::SkillRef::Ref(r) => self.drop(format!("{at}/skills/{i}"), format!("The skill {r:?} is named, not included in the file; add it to the agent with Load from file")),
            }
        }
        out
    }

    /// The agent's components: tools by id, and tools written on the agent, which join the team's.
    fn integrations(&mut self, at: &str, a: &m::Agent) -> Vec<String> {
        let mut out = Vec::new();
        for (i, t) in a.tools.iter().enumerate() {
            let id = match t {
                m::ToolRef::Ref(id) => id.clone(),
                m::ToolRef::Def(t) => match super::tools::tool_in(t) {
                    Ok(c) if self.components.iter().any(|k| k.id == c.id && *k != c) => {
                        self.drop(format!("{at}/tools/{i}"), format!("Another tool is already called `{}`; this one was not kept", c.id));
                        continue;
                    }
                    Ok(c) => {
                        if !self.components.contains(&c) && !super::tools::is_builtin_as_shipped(&c) {
                            self.components.push(c.clone());
                        }
                        c.id
                    }
                    Err(why) => {
                        self.drop(format!("{at}/tools/{i}"), format!("Tool `{}` {why}; it was not kept", t.id));
                        continue;
                    }
                },
            };
            if self.components.iter().any(|c| c.id == id) || model::component::builtins().iter().any(|c| c.id == id) {
                out.push(id);
            } else if !self.doc.tools.iter().any(|t| t.id == id) {
                self.drop(format!("{at}/tools/{i}"), format!("Sculptured has no tool `{id}`; it was not kept"));
            }
        }
        out
    }

    fn unkept_agent_parts(&mut self, at: &str, a: &m::Agent) {
        let parts = [
            ("context", !a.context.is_empty()),
            ("outputs", !a.outputs.is_empty()),
            ("placement", a.placement.is_some()),
            ("subagents", a.subagents.is_some()),
            ("endpoint", a.endpoint.is_some()),
        ];
        for (field, set) in parts {
            if set {
                self.drop(format!("{at}/{field}"), format!("Sculptured does not use an agent's `{field}` yet; it was not kept"));
            }
        }
    }
}
