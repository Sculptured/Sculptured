//! Validation of the fields and participants for each workflow stage kind.
use model::{CopiesMode, StageKind, TeamDraft, Workflow, WorkflowStage};
use std::collections::HashSet;

pub(super) fn errors(d: &TeamDraft, w: &Workflow, s: &WorkflowStage) -> Vec<String> {
    let mut errors = field_errors(s);
    errors.extend(match s.kind {
        StageKind::Agent if !s.reviewers.is_empty() => panel_errors(d, w, s),
        StageKind::Agent => agent_errors(d, w, s),
        StageKind::Check => check_errors(s),
        StageKind::Human => human_errors(s),
        StageKind::Merge => merge_errors(w, s),
    });
    errors
}

fn usable(d: &TeamDraft, w: &Workflow, id: &str) -> bool {
    d.agent(id).is_some() && id != w.dispatcher && id != w.collector
}

fn only_routes(s: &WorkflowStage, keys: &[&str]) -> bool {
    s.routes.len() == keys.len() && keys.iter().all(|key| s.routes.contains_key(*key))
}

fn no_agents(s: &WorkflowStage) -> bool {
    s.agent.is_empty() && s.workers.is_empty() && s.reviewers.is_empty()
}

fn field_errors(s: &WorkflowStage) -> Vec<String> {
    let mut e = Vec::new();
    let name = &s.name;
    if s.kind != StageKind::Check && (s.command.is_some() || s.timeout_s.is_some()) {
        e.push(format!("{name} is not a check: remove its command."));
    }
    if s.kind != StageKind::Human && s.prompt.is_some() {
        e.push(format!(
            "{name} is not a person's decision: remove its prompt."
        ));
    }
    if s.kind != StageKind::Merge && s.into.is_some() {
        e.push(format!("{name} is not a merge: remove its branch."));
    }
    if s.reviewers.is_empty() && (!s.join.is_all() || !s.priority.is_empty()) {
        e.push(format!("{name}: only a review panel has a join rule."));
    }
    e
}

fn panel_errors(d: &TeamDraft, w: &Workflow, s: &WorkflowStage) -> Vec<String> {
    let mut e = Vec::new();
    let name = &s.name;

    let distinct: HashSet<_> = s.reviewers.iter().collect();
    if !s.agent.is_empty()
        || !s.workers.is_empty()
        || !(2..=6).contains(&s.reviewers.len())
        || distinct.len() != s.reviewers.len()
        || !s.reviewers.iter().all(|id| usable(d, w, id))
    {
        e.push(format!(
            "{name} needs 2–6 different reviewers, and no single agent."
        ));
    }
    if s.priority
        .iter()
        .any(|p| p != "blocked" && !s.routes.contains_key(p))
    {
        e.push(format!(
            "{name}'s vote order names an outcome it doesn't have."
        ));
    }
    e
}

fn agent_errors(d: &TeamDraft, w: &Workflow, s: &WorkflowStage) -> Vec<String> {
    let mut e = Vec::new();
    let name = &s.name;

    let workers: HashSet<_> = std::iter::once(&s.agent).chain(s.workers.iter()).collect();
    if workers.len() != s.workers.len() + 1
        || workers.len() > 8
        || !workers.iter().all(|id| usable(d, w, id))
    {
        e.push(format!(
            "{name} needs 1–8 distinct existing workers, excluding dispatch and assembly."
        ));
    }
    e
}

fn check_errors(s: &WorkflowStage) -> Vec<String> {
    let mut e = Vec::new();
    let name = &s.name;

    if !no_agents(s) {
        e.push(format!(
            "{name} is a check: Sculptured runs it, so remove its agents."
        ));
    }
    if s.command
        .as_deref()
        .map(str::trim)
        .is_none_or(|c| c.is_empty() || c.len() > 2000)
    {
        e.push(format!("{name} needs a command of 1–2000 bytes."));
    }
    if s.timeout_s.is_some_and(|t| !(1..=3600).contains(&t)) {
        e.push(format!("{name} needs a time limit of 1–3600 seconds."));
    }
    if !only_routes(s, &["passed", "failed"]) {
        e.push(format!(
            "{name} needs exactly two routes: passed and failed."
        ));
    }
    e
}

fn human_errors(s: &WorkflowStage) -> Vec<String> {
    let mut e = Vec::new();
    let name = &s.name;

    if !no_agents(s) {
        e.push(format!("{name} is a person's decision: remove its agents."));
    }
    if s.prompt
        .as_deref()
        .map(str::trim)
        .is_none_or(|p| p.is_empty() || p.len() > 2048)
    {
        e.push(format!(
            "{name} needs a question for the person (up to 2 KB)."
        ));
    }
    if !(2..=6).contains(&s.routes.len()) {
        e.push(format!(
            "{name} needs 2–6 outcomes for the person to choose from."
        ));
    }
    e
}

fn merge_errors(w: &Workflow, s: &WorkflowStage) -> Vec<String> {
    let mut e = Vec::new();
    let name = &s.name;

    if !no_agents(s) {
        e.push(format!(
            "{name} is a merge: Sculptured does it, so remove its agents."
        ));
    }
    if !only_routes(s, &["merged", "conflict"]) {
        e.push(format!(
            "{name} needs exactly two routes: merged and conflict."
        ));
    }
    if w.copies != CopiesMode::PerItem {
        e.push(format!(
            "{name} merges an item's own branch: turn on a copy per item (Advanced)."
        ));
    }
    if s.into
        .as_deref()
        .is_some_and(|b| b.is_empty() || b.len() > 200 || b.chars().any(char::is_whitespace))
    {
        e.push(format!("{name} needs a branch name without spaces."));
    }
    e
}
