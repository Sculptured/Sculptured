//! A team's workflow both ways, as one of MADL's `workflows` (MEP 0007): its Input is a MADL input
//! that goes to the dispatcher, its guard is `policies.guards`, and its board positions are in the
//! layout.
use super::export::free_id;
use madl::model as m;
use model::{BoardPosition, CopiesMode, Escalation, Guard, Join, StageKind, TeamDraft, Workflow, WorkflowStage};

/// The workflow's input id, in a document that has one.
pub fn input_of(doc: &m::Document) -> Option<&str> {
    doc.workflows.first()?.input.as_deref()
}

pub fn out(team: &TeamDraft, w: &Workflow, doc: &mut m::Document) {
    let id = free_id(team, "workflow");
    let input = free_id(team, &format!("{id}-input"));
    // A dispatch command makes the batch itself; the input then goes to the entry stage's agent.
    let to = if w.dispatcher.is_empty() { w.stages.iter().find(|s| s.id == w.entry).map(|s| s.agent.clone()).filter(|a| !a.is_empty()) } else { Some(w.dispatcher.clone()) };
    doc.inputs.push(m::Input { id: input.clone(), text: w.input.clone(), to: to.into_iter().collect() });
    let layout = doc.layout.get_or_insert_with(Default::default);
    if let Some(p) = &w.input_position {
        layout.nodes.insert(input.clone(), point(p));
    }
    let positions: std::collections::BTreeMap<String, m::Point> = w.stages.iter().filter_map(|s| Some((s.id.clone(), point(s.position.as_ref()?)))).collect();
    if !positions.is_empty() {
        layout.stages.insert(id.clone(), positions);
    }
    doc.workflows.push(m::Workflow {
        id,
        name: None,
        input: Some(input),
        dispatcher: Some(w.dispatcher.clone()).filter(|d| !d.is_empty()),
        dispatch: w.dispatch.as_ref().map(|d| m::Dispatch { command: d.command.clone(), timeout_s: d.timeout_s }),
        entry: w.entry.clone(),
        collector: w.collector.clone(),
        max_items: Some(w.max_tasks),
        max_steps: Some(w.max_steps),
        copies: (w.copies == CopiesMode::PerItem).then_some(m::CopyMode::PerItem),
        stages: w.stages.iter().map(stage_out).collect(),
        extensions: Default::default(),
    });
}

fn point(p: &BoardPosition) -> m::Point {
    m::Point { x: p.x.into(), y: p.y.into() }
}

fn stage_out(s: &WorkflowStage) -> m::Stage {
    let kind = match s.kind {
        StageKind::Agent => None,
        StageKind::Check => Some(m::StageKind::Check),
        StageKind::Human => Some(m::StageKind::Human),
        StageKind::Merge => Some(m::StageKind::Merge),
    };
    let persona = |p: &model::ReviewPersona| serde_json::to_value(p).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    m::Stage {
        id: s.id.clone(),
        name: Some(s.name.clone()).filter(|n| !n.is_empty()),
        kind,
        agent: Some(s.agent.clone()).filter(|a| !a.is_empty()),
        workers: s.workers.clone(),
        reviewers: s.reviewers.clone(),
        personas: s.personas.iter().map(|(a, p)| (a.clone(), persona(p))).collect(),
        join: (s.join == Join::Any).then_some(m::Join::Any),
        priority: s.priority.clone(),
        routes: s.routes.clone(),
        command: s.command.clone(),
        timeout_s: s.timeout_s,
        prompt: s.prompt.clone(),
        into: s.into.clone(),
        max_rounds: s.max_rounds,
        on_limit: s.on_limit.clone(),
        escalate: s.escalate.as_ref().map(|e| m::StageEscalation { after_rounds: e.after_rounds, to: e.agent.clone() }),
    }
}

/// The document's first workflow, with what Sculptured could not keep (path, why).
pub fn workflow_in(doc: &m::Document) -> (Option<Workflow>, Vec<(String, String)>) {
    let mut dropped: Vec<(String, String)> = (1..doc.workflows.len()).map(|i| (format!("/workflows/{i}"), "Sculptured runs one workflow per team; the first was kept".into())).collect();
    let Some(w) = doc.workflows.first() else { return (None, dropped) };
    let at = |id: &str| doc.layout.as_ref().and_then(|l| l.nodes.get(id));
    let input = w.input.as_deref().and_then(|id| doc.inputs.iter().find(|i| i.id == id));
    let positions = doc.layout.as_ref().and_then(|l| l.stages.get(&w.id));
    let mut stages = Vec::new();
    for (i, s) in w.stages.iter().enumerate() {
        stages.push(stage_in(s, positions.and_then(|p| p.get(&s.id)), &format!("/workflows/0/stages/{i}"), &mut dropped));
    }
    let guards = doc.policies.as_ref().map(|p| p.guards.as_slice()).unwrap_or_default();
    dropped.extend((1..guards.len()).map(|i| (format!("/policies/guards/{i}"), "Sculptured's workflow has one guard; the first was kept".into())));
    let flow = Workflow {
        input_position: input.and_then(|i| at(&i.id)).map(position),
        input: input.map(|i| i.text.clone()).unwrap_or_default(),
        dispatcher: w.dispatcher.clone().unwrap_or_default(),
        dispatch: w.dispatch.as_ref().map(|d| model::WorkflowDispatch { command: d.command.clone(), timeout_s: d.timeout_s }),
        entry: w.entry.clone(),
        collector: w.collector.clone(),
        max_tasks: w.max_items.unwrap_or(24),
        max_steps: w.max_steps.unwrap_or(20),
        stages,
        guard: guards.first().map(|g| Guard { command: g.command.clone(), every_minutes: super::import::madl_minutes(&g.every).unwrap_or(10), timeout_s: g.timeout_s }),
        copies: if w.copies == Some(m::CopyMode::PerItem) { CopiesMode::PerItem } else { CopiesMode::Shared },
    };
    (Some(flow), dropped)
}

fn position(p: &m::Point) -> BoardPosition {
    BoardPosition { x: p.x as i32, y: p.y as i32 }
}

fn stage_in(s: &m::Stage, at: Option<&m::Point>, path: &str, dropped: &mut Vec<(String, String)>) -> WorkflowStage {
    let kind = match s.kind {
        None | Some(m::StageKind::Agent) => StageKind::Agent,
        Some(m::StageKind::Check) => StageKind::Check,
        Some(m::StageKind::Human) => StageKind::Human,
        Some(m::StageKind::Merge) => StageKind::Merge,
    };
    let mut personas = std::collections::BTreeMap::new();
    for (agent, p) in &s.personas {
        match serde_json::from_value(serde_json::Value::String(p.clone())) {
            Ok(persona) => drop(personas.insert(agent.clone(), persona)),
            Err(_) => dropped.push((format!("{path}/personas/{agent}"), format!("Sculptured has no reviewer angle `{p}`; the reviewer uses its own judgement"))),
        }
    }
    WorkflowStage {
        id: s.id.clone(),
        name: s.name.clone().unwrap_or_default(),
        agent: s.agent.clone().unwrap_or_default(),
        workers: s.workers.clone(),
        routes: s.routes.clone(),
        kind,
        command: s.command.clone(),
        timeout_s: s.timeout_s,
        prompt: s.prompt.clone(),
        max_rounds: s.max_rounds,
        on_limit: s.on_limit.clone(),
        reviewers: s.reviewers.clone(),
        personas,
        join: if s.join == Some(m::Join::Any) { Join::Any } else { Join::All },
        priority: s.priority.clone(),
        escalate: s.escalate.as_ref().map(|e| Escalation { after_rounds: e.after_rounds, agent: e.to.clone() }),
        position: at.map(position),
        into: s.into.clone(),
    }
}
