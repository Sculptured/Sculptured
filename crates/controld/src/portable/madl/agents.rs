//! Agents, for [`super::import::from_madl`].
use super::import::{effective_runtime, ext_of, kind_of, Cx};
use madl::model as m;
use madl::resolve::AgentSettings;
use model::*;

impl Cx<'_> {
    pub(super) fn agent(&mut self, i: usize, a: &m::Agent, r: &AgentSettings) -> Option<DraftAgent> {
        let at = format!("/agents/{i}");
        let runtime = effective_runtime(r);
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
            advanced: self.advanced(&at, a, r),
        })
    }

    fn group_of(&mut self, at: &str, a: &m::Agent) -> Option<String> {
        if a.groups.len() > 1 {
            self.drop(format!("{at}/groups"), format!("`{}` was in {} groups; it was put in `{}`, its first", a.id, a.groups.len(), a.groups[0]));
        }
        let hidden = ext_of(&a.extensions).get("group_id").and_then(|g| g.as_str()).map(str::to_string);
        a.groups.first().cloned().or(hidden)
    }

    fn position(&self, id: &str, i: usize) -> (i32, i32) {
        match self.doc.layout.as_ref().and_then(|l| l.nodes.get(id)) {
            Some(p) => (p.x as i32, p.y as i32),
            None => (40 + 260 * (i % 4) as i32, 40 + 180 * (i / 4) as i32),
        }
    }

    fn advanced(&mut self, at: &str, a: &m::Agent, r: &AgentSettings) -> AgentAdvanced {
        let ext = ext_of(&a.extensions);
        let counts = ext.get("token_counts").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let (token_limit, turn_limit, minute_limit) = super::limits::agent_in(r.settings.restrictions.budget.as_ref(), counts);
        if a.checks.len() > 1 {
            self.drop(format!("{at}/checks/1"), "Sculptured runs one check per agent; the first was kept");
        }
        self.unkept_agent_parts(at, a);
        AgentAdvanced { turn_limit, minute_limit,
            model: self.model(at, r),
            permission: ext.get("permission").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
            resume: match r.settings.preferences.resume {
                Some(m::Resume::RelaunchFresh) => ResumeBehaviour::RelaunchFresh,
                Some(m::Resume::CheckpointOnly) => ResumeBehaviour::CheckpointOnly,
                Some(m::Resume::ResumeIfPossible) | None => ResumeBehaviour::ResumeIfPossible,
            },
            token_limit,
            check: a.checks.first().map(|c| c.command.clone()),
            data_access: a.data_access.as_ref().map(|d| model::DataAccess {
                reads: d.reads.clone(), writes: d.writes.clone(), side_effects: d.side_effects.clone(), rationale: d.rationale.clone(),
                shared: ext.get("shared_values").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
            }),
            authority: a.authority.as_ref().map(|x| model::Authority { domains: x.domains.clone(), scope: x.scope.clone(), rationale: x.rationale.clone() }),
            skills: self.skills(at, a),
            integrations: ext.get("integrations").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
            briefing: ext.get("briefing").and_then(|v| serde_json::from_value(v.clone()).ok()),
        }
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

    fn model(&mut self, at: &str, r: &AgentSettings) -> Option<String> {
        match r.settings.preferences.model.as_ref()? {
            m::ModelRef::Id(id) => Some(id.clone()),
            m::ModelRef::WithFallback { id, fallback } => {
                if !fallback.is_empty() {
                    self.drop(format!("{at}/model"), "Sculptured uses one model per agent; the fallbacks were not kept");
                }
                Some(id.clone())
            }
        }
        .map(|id| id.split_once('/').map_or(id.clone(), |(_, model)| model.to_string()))
    }

    fn unkept_agent_parts(&mut self, at: &str, a: &m::Agent) {
        let parts = [
            ("tools", !a.tools.is_empty()),
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

    /// An agent whose model came from its group or team keeps inheriting it.
    pub(super) fn models(&mut self, team: &mut TeamDraft) {
        for i in 0..team.agents.len() {
            let inherited = self.doc.agents.iter().find(|a| a.id == team.agents[i].id).is_some_and(|a| ext_of(&a.extensions).get("model_inherited") == Some(&serde_json::Value::Bool(true)));
            if !inherited {
                continue;
            }
            let own = team.agents[i].advanced.model.take();
            if team.effective_model(&team.agents[i]) != own {
                team.agents[i].advanced.model = own;
            }
        }
    }
}
