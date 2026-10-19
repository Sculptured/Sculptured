//! A workflow's stages drawn in MADL's core: check, person and merge stages become steps, and
//! routes become handoffs with the outcome as their condition. Everything drawn here is tagged
//! `com.usesculptured.workflow`; the workflow itself travels whole in the team's extension, and
//! import rebuilds it from there.
use super::NS;
use madl::model as m;
use model::{StageKind, TeamDraft, Workflow, WorkflowStage};
use serde_json::json;

pub fn plain_connection(id: Option<String>, from: &str, to: &str, kind: m::ConnectionKind) -> m::Connection {
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

/// The tag on everything drawn from the workflow.
pub fn tag() -> m::Extensions {
    let mut ext = m::Extensions::new();
    ext.insert(NS.into(), json!({ "workflow": true }));
    ext
}

pub fn is_tagged(ext: &m::Extensions) -> bool {
    ext.get(NS).and_then(|v| v.get("workflow")).and_then(|v| v.as_bool()) == Some(true)
}

pub fn render(team: &TeamDraft, w: &Workflow, doc: &mut m::Document) {
    let nodes: Vec<Vec<String>> = w.stages.iter().map(|s| stage_nodes(team, s, doc)).collect();
    let at = |id: &str| w.stages.iter().position(|s| s.id == id);
    // A dispatch command has no agent to draw it from; its batch still enters at the entry stage.
    if let Some(entry) = at(&w.entry).filter(|_| !w.dispatcher.is_empty()) {
        for n in &nodes[entry] {
            push(doc, &w.dispatcher, n, m::ConnectionKind::Delegate, None, None);
        }
    }
    for (i, s) in w.stages.iter().enumerate() {
        for (outcome, next) in &s.routes {
            let Some(j) = next.as_deref().and_then(at) else { continue };
            let when = Some(format!("outcome == '{}'", outcome.replace('\'', "\\'")));
            for from in &nodes[i] {
                for to in &nodes[j] {
                    push(doc, from, to, m::ConnectionKind::Handoff, when.clone(), w.stages[j].max_rounds);
                }
            }
        }
    }
    if team.input.is_none() && !w.input.trim().is_empty() && !w.dispatcher.is_empty() {
        doc.inputs.push(m::Input { id: free_id(doc, "workflow-input"), text: w.input.clone(), to: vec![w.dispatcher.clone()] });
    }
}

fn push(doc: &mut m::Document, from: &str, to: &str, kind: m::ConnectionKind, when: Option<String>, max_rounds: Option<u32>) {
    if from == to {
        return;
    }
    let mut c = plain_connection(None, from, to, kind);
    c.when = when;
    c.max_rounds = max_rounds;
    c.extensions = tag();
    doc.connections.push(c);
}

/// The MADL nodes a stage stands for: its agent, its reviewers, or a step of its own.
fn stage_nodes(team: &TeamDraft, s: &WorkflowStage, doc: &mut m::Document) -> Vec<String> {
    if s.kind == StageKind::Agent {
        let agents = if s.reviewers.is_empty() { vec![s.agent.clone()] } else { s.reviewers.clone() };
        return agents.into_iter().filter(|a| team.agent(a).is_some()).collect();
    }
    let id = free_id(doc, &s.id);
    let mut step = m::Step {
        id: id.clone(),
        kind: m::StepKind::Merge,
        name: Some(s.name.clone()).filter(|n| !n.is_empty()),
        command: None,
        timeout_s: s.timeout_s,
        prompt: None,
        outcomes: Vec::new(),
        into: None,
        over: None,
        copies: None,
        join: None,
        extensions: tag(),
    };
    match s.kind {
        StageKind::Check => (step.kind, step.command) = (m::StepKind::Check, Some(s.command.clone().unwrap_or_default())),
        StageKind::Human => {
            step.kind = m::StepKind::Human;
            step.prompt = Some(s.prompt.clone().unwrap_or_else(|| s.name.clone()));
            step.outcomes = s.routes.keys().cloned().collect();
        }
        StageKind::Merge | StageKind::Agent => step.into = s.into.clone(),
    }
    doc.steps.push(step);
    vec![id]
}

/// `wanted`, or `stage-<wanted>`, `stage-<wanted>-2`… when the document already uses it.
fn free_id(doc: &m::Document, wanted: &str) -> String {
    let taken = |id: &str| {
        doc.agents.iter().any(|a| a.id == id) || doc.groups.iter().any(|g| g.id == id) || doc.steps.iter().any(|s| s.id == id) || doc.inputs.iter().any(|i| i.id == id)
    };
    if !taken(wanted) {
        return wanted.to_string();
    }
    let mut id = format!("stage-{wanted}");
    let mut n = 1;
    while taken(&id) {
        n += 1;
        id = format!("stage-{wanted}-{n}");
    }
    id
}
