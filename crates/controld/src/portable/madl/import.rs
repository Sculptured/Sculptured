//! A MADL document as a team file. MADL can describe more than Sculptured runs (people as
//! participants, channels, bans, other runtimes); each part that can't be kept is listed with its
//! JSON Pointer, like the OpenRig import does.
use super::NS;
use crate::portable::team::{PortableSettings, TeamFile, TEAM_FORMAT, TEAM_SCHEMA_URL};
use madl::model as m;
use madl::resolve::AgentSettings;
use model::*;
use rigspec::{Diagnostic, Severity};
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Debug)]
pub struct Imported {
    pub file: TeamFile,
    /// What MADL said that Sculptured could not keep.
    pub dropped: Vec<Diagnostic>,
}

/// `doc` must already be valid and expanded.
pub fn from_madl(doc: &m::Document) -> Imported {
    let resolved = madl::resolve(doc);
    let mut cx = Cx { doc, ext: ext_of(&doc.extensions), dropped: Vec::new() };
    let (mut budget, budget_dropped) = super::limits::run_in(doc.system.budget.as_ref());
    with_pauses(&mut budget, &ext_of(&doc.extensions));
    for (path, why) in budget_dropped {
        cx.drop(path, why);
    }
    let mut team = TeamDraft { budget, safety: cx.get("safety").unwrap_or_default(), components: cx.get("components").unwrap_or_default(), claude_setup: cx.get("claude_setup").unwrap_or_default(), protection: cx.get("protection").unwrap_or_default(),
        workflow: cx.get("workflow"),
        input: None,
        folder: cx.get("folder"),
        id: doc.system.id.clone(),
        name: doc.system.name.clone(),
        groups: doc.groups.iter().enumerate().map(|(i, g)| cx.group(i, g)).collect(),
        agents: Vec::new(),
        connections: Vec::new(),
        models: cx.get("models").unwrap_or_default(),
        schedules: cx.schedules(),
        limits: TeamLimits { max_agents: cx.get("max_agents"), max_child_levels: spawn_depth(doc) },
        decision_log: cx.get("decision_log").unwrap_or(false),
        revision: 0,
    };
    team.agents = doc.agents.iter().zip(&resolved.agents).enumerate().filter_map(|(i, (a, r))| cx.agent(i, a, r)).collect();
    cx.models(&mut team);
    team.connections = doc.connections.iter().enumerate().flat_map(|(i, c)| cx.connection(i, c, &team)).collect();
    team.input = cx.input(&team);
    cx.unkept();
    let settings = cx.settings(&team);
    let file = TeamFile { schema: Some(TEAM_SCHEMA_URL.into()), format: TEAM_FORMAT.into(), team, settings };
    Imported { file, dropped: cx.dropped }
}

pub(super) fn ext_of(ext: &m::Extensions) -> serde_json::Map<String, Value> {
    ext.get(NS).and_then(Value::as_object).cloned().unwrap_or_default()
}

fn spawn_depth(doc: &m::Document) -> Option<u32> {
    doc.defaults.as_ref()?.restrictions.as_ref()?.spawn?.max_depth
}

pub(super) struct Cx<'a> {
    pub(super) doc: &'a m::Document,
    pub(super) ext: serde_json::Map<String, Value>,
    pub(super) dropped: Vec<Diagnostic>,
}

impl Cx<'_> {
    pub(super) fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.ext.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub(super) fn drop(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.dropped.push(Diagnostic { path: path.into(), severity: Severity::Warning, message: message.into() });
    }

    fn group(&mut self, i: usize, g: &m::Group) -> DraftGroup {
        if g.parent.is_some() {
            self.drop(format!("/groups/{i}/parent"), format!("Group `{}` was inside another; Sculptured's groups sit side by side", g.id));
        }
        let rect = self.doc.layout.as_ref().and_then(|l| l.groups.get(&g.id)).copied();
        let r = rect.unwrap_or(m::Rect { x: 40.0 + 560.0 * i as f64, y: 40.0, w: 520.0, h: 360.0 });
        let models = ext_of(&g.extensions).get("models").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        let label = g.name.clone().unwrap_or_else(|| g.id.clone());
        DraftGroup { id: g.id.clone(), label, x: r.x as i32, y: r.y as i32, w: r.w as i32, h: r.h as i32, models }
    }

    fn schedules(&self) -> Vec<Schedule> {
        let list = self.doc.policies.iter().flat_map(|p| p.schedules.iter());
        let minutes = |every: &str| madl_minutes(every).unwrap_or(60);
        list.map(|s| Schedule { id: s.id.clone(), agent: s.agent.clone(), every_minutes: minutes(&s.every), message: s.message.clone() }).collect()
    }

    fn settings(&self, team: &TeamDraft) -> PortableSettings {
        let timeout = self.doc.defaults.as_ref().and_then(|d| d.preferences.as_ref()?.questions.as_ref()?.timeout.clone());
        let all_own = !team.agents.is_empty() && self.doc.agents.iter().all(|a| a.workspace.as_ref().and_then(|w| w.copy) == Some(m::CopyMode::Own));
        PortableSettings {
            posture: self.get("posture"),
            own_copies: self.get("own_copies").unwrap_or(all_own),
            question_timeout_min: timeout.as_deref().and_then(madl_minutes),
        }
    }
}

/// Whole minutes in a MADL duration, at least 1.
pub fn madl_minutes(every: &str) -> Option<u32> {
    let secs = parse_duration(every)?;
    Some(u32::try_from(secs.div_ceil(60)).unwrap_or(u32::MAX).max(1))
}

/// `90s`, `30m`, `2h`, `1d`, `1h30m`.
fn parse_duration(text: &str) -> Option<u64> {
    let (mut total, mut n) = (0u64, String::new());
    for c in text.trim().chars() {
        if c.is_ascii_digit() {
            n.push(c);
            continue;
        }
        let unit = match c { 's' => 1, 'm' => 60, 'h' => 3600, 'd' => 86_400, _ => return None };
        total += n.parse::<u64>().ok()? * unit;
        n.clear();
    }
    n.is_empty().then_some(total)
}

pub(super) fn kind_of(runtime: Option<&str>) -> Option<AgentKind> {
    match runtime {
        Some("claude-code") => Some(AgentKind::Claude),
        Some("codex") => Some(AgentKind::Codex),
        Some("x-sculptured-practice") => Some(AgentKind::Practice),
        _ => None,
    }
}

pub(super) fn effective_runtime(r: &AgentSettings) -> Option<&str> {
    r.settings.preferences.runtime.as_deref()
}

#[derive(serde::Deserialize)]
struct PlanPauseExt {
    provider: String,
    window: String,
    pause: model::PlanPause,
}

/// The plan budgets that pause at their hard value, from Sculptured's extension.
fn with_pauses(budget: &mut model::RunBudget, ext: &serde_json::Map<String, serde_json::Value>) {
    let Some(list) = ext.get("plan_pause").and_then(|v| serde_json::from_value::<Vec<PlanPauseExt>>(v.clone()).ok()) else { return };
    for p in &mut budget.plan {
        p.pause = list.iter().find(|x| x.provider == p.provider && x.window == p.window).map(|x| x.pause);
    }
}
