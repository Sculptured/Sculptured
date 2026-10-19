use model::{CopiesMode, DraftIssue, ElementKind, ElementRef, IssueSeverity, StageKind, TeamDraft, Workflow, WorkflowStage};
use std::collections::HashSet;

pub(super) fn check(d: &TeamDraft) -> Vec<DraftIssue> {
    let Some(w) = &d.workflow else { return vec![] };
    errors(d, w).into_iter().map(|message| DraftIssue {
        element: ElementRef { kind: ElementKind::Team, id: d.id.clone() }, severity: IssueSeverity::Error,
        code: "workflow-invalid".into(), message, detail: None, spec_path: None,
    }).collect()
}

fn errors(d: &TeamDraft, w: &Workflow) -> Vec<String> {
    let mut errors = Vec::new();
    if w.input.len() > 64000 { errors.push("Keep the Input block under 64 KB; reference larger source files by path or link.".into()); }
    if !(1..=64).contains(&w.max_tasks) || !(1..=32).contains(&w.max_steps) {
        errors.push("Dynamic dispatch needs 1–64 tasks and 1–32 steps per task.".into());
    }
    errors.extend(dispatch_errors(d, w));
    if !d.connections.is_empty() { errors.push("Dynamic workflows use stage routes instead of ordinary connections. Remove ordinary connections first.".into()); }
    if w.copies == CopiesMode::PerItem && !has_folder(d) { errors.push("A copy per item needs the team's project folder.".into()); }
    if let Some(g) = &w.guard {
        if g.command.trim().is_empty() || g.command.len() > 2000 { errors.push("The guard needs a command of 1–2000 bytes.".into()); }
        if !(5..=1440).contains(&g.every_minutes) { errors.push("The guard runs every 5–1440 minutes.".into()); }
        if g.timeout_s.is_some_and(|t| !(1..=3600).contains(&t)) { errors.push("The guard needs a time limit of 1–3600 seconds.".into()); }
        if !has_folder(d) { errors.push("The guard runs in the team's project folder; set one in team settings.".into()); }
    }
    let ids: HashSet<_> = w.stages.iter().map(|s| &s.id).collect();
    if w.stages.is_empty() || w.stages.len() > 16 || ids.len() != w.stages.len() || !ids.contains(&w.entry) {
        errors.push("Choose an entry stage and 1–16 stages with unique IDs.".into());
    }
    for s in &w.stages {
        if !model::valid_segment(&s.id) || s.id == "assembly" { errors.push(format!("{} needs a valid stage ID.", s.name)); }
        if s.routes.is_empty() || s.routes.len() > 8 || s.routes.keys().any(|k| !model::valid_segment(k) || k == "blocked") || s.routes.values().flatten().any(|id| !ids.contains(id)) {
            errors.push(format!("{} needs 1–8 named routes to existing stages or completion; blocked is reserved.", s.name));
        }
        errors.extend(kind_errors(d, w, s));
        if let Some(e) = &s.escalate {
            let single = s.kind == StageKind::Agent && s.reviewers.is_empty();
            let agent_ok = d.agent(&e.agent).is_some() && e.agent != s.agent && e.agent != w.dispatcher && e.agent != w.collector;
            if !single || !agent_ok || !(1..=20).contains(&e.after_rounds) || s.max_rounds.is_some_and(|m| e.after_rounds >= m) {
                errors.push(format!("{} hands items over after 1–20 rounds (fewer than its round limit) to another existing agent; only a single-agent stage can.", s.name));
            }
        }
        if s.max_rounds.is_some_and(|m| !(1..=20).contains(&m)) { errors.push(format!("{} allows 1–20 rounds.", s.name)); }
        if s.on_limit.is_some() && s.max_rounds.is_none() { errors.push(format!("{} has a round-limit route but no round limit.", s.name)); }
        if s.on_limit.as_ref().is_some_and(|id| !ids.contains(id) || *id == s.id) { errors.push(format!("{}'s round-limit route must name another existing stage.", s.name)); }
        if !can_finish(w, &s.id, &mut HashSet::new()) { errors.push(format!("{} has no route to completion.", s.name)); }
    }
    errors
}

/// Who makes the batch: a dispatcher agent, or a dispatch command (never both); and the assembly agent.
fn dispatch_errors(d: &TeamDraft, w: &Workflow) -> Vec<String> {
    let Some(c) = &w.dispatch else {
        let ok = d.agent(&w.dispatcher).is_some() && d.agent(&w.collector).is_some() && w.dispatcher != w.collector;
        return if ok { vec![] } else { vec!["Choose different, existing dispatcher and assembly agents.".into()] };
    };
    let mut e = Vec::new();
    if !w.dispatcher.is_empty() { e.push("Choose a dispatcher agent or a dispatch command, not both.".into()); }
    if d.agent(&w.collector).is_none() { e.push("Choose an existing assembly agent.".into()); }
    if c.command.trim().is_empty() || c.command.len() > 2000 { e.push("The dispatch command needs 1–2000 bytes.".into()); }
    if c.timeout_s.is_some_and(|t| !(1..=3600).contains(&t)) { e.push("The dispatch command needs a time limit of 1–3600 seconds.".into()); }
    if !has_folder(d) { e.push("The dispatch command runs in the team's project folder; set one in team settings.".into()); }
    e
}

/// The rules for one stage's kind: its own fields present, other kinds' fields absent.
fn kind_errors(d: &TeamDraft, w: &Workflow, s: &WorkflowStage) -> Vec<String> {
    let mut e = Vec::new();
    let name = &s.name;
    let only = |keys: &[&str]| s.routes.len() == keys.len() && keys.iter().all(|k| s.routes.contains_key(*k));
    let no_agents = s.agent.is_empty() && s.workers.is_empty() && s.reviewers.is_empty();
    let usable = |id: &String| d.agent(id).is_some() && *id != w.dispatcher && *id != w.collector;
    if s.kind != StageKind::Check && (s.command.is_some() || s.timeout_s.is_some()) { e.push(format!("{name} is not a check: remove its command.")); }
    if s.kind != StageKind::Human && s.prompt.is_some() { e.push(format!("{name} is not a person's decision: remove its prompt.")); }
    if s.kind != StageKind::Merge && s.into.is_some() { e.push(format!("{name} is not a merge: remove its branch.")); }
    if s.reviewers.is_empty() && (!s.join.is_all() || !s.priority.is_empty()) { e.push(format!("{name}: only a review panel has a join rule.")); }
    match s.kind {
        StageKind::Agent if !s.reviewers.is_empty() => {
            let distinct: HashSet<_> = s.reviewers.iter().collect();
            if !s.agent.is_empty() || !s.workers.is_empty() || !(2..=6).contains(&s.reviewers.len()) || distinct.len() != s.reviewers.len() || !s.reviewers.iter().all(usable) {
                e.push(format!("{name} needs 2–6 different reviewers, and no single agent."));
            }
            if s.priority.iter().any(|p| p != "blocked" && !s.routes.contains_key(p)) { e.push(format!("{name}'s vote order names an outcome it doesn't have.")); }
        }
        StageKind::Agent => {
            let workers: HashSet<_> = std::iter::once(&s.agent).chain(s.workers.iter()).collect();
            if workers.len() != s.workers.len() + 1 || workers.len() > 8 || !workers.iter().all(|id| usable(id)) {
                e.push(format!("{name} needs 1–8 distinct existing workers, excluding dispatch and assembly."));
            }
        }
        StageKind::Check => {
            if !no_agents { e.push(format!("{name} is a check: Sculptured runs it, so remove its agents.")); }
            if s.command.as_deref().map(str::trim).is_none_or(|c| c.is_empty() || c.len() > 2000) { e.push(format!("{name} needs a command of 1–2000 bytes.")); }
            if s.timeout_s.is_some_and(|t| !(1..=3600).contains(&t)) { e.push(format!("{name} needs a time limit of 1–3600 seconds.")); }
            if !only(&["passed", "failed"]) { e.push(format!("{name} needs exactly two routes: passed and failed.")); }
            if !has_folder(d) { e.push(format!("{name} runs in the team's project folder; set one in team settings.")); }
        }
        StageKind::Human => {
            if !no_agents { e.push(format!("{name} is a person's decision: remove its agents.")); }
            if s.prompt.as_deref().map(str::trim).is_none_or(|p| p.is_empty() || p.len() > 2048) { e.push(format!("{name} needs a question for the person (up to 2 KB).")); }
            if !(2..=6).contains(&s.routes.len()) { e.push(format!("{name} needs 2–6 outcomes for the person to choose from.")); }
        }
        StageKind::Merge => {
            if !no_agents { e.push(format!("{name} is a merge: Sculptured does it, so remove its agents.")); }
            if !only(&["merged", "conflict"]) { e.push(format!("{name} needs exactly two routes: merged and conflict.")); }
            if w.copies != CopiesMode::PerItem { e.push(format!("{name} merges an item's own branch: turn on a copy per item (Advanced).")); }
            if s.into.as_deref().is_some_and(|b| b.is_empty() || b.len() > 200 || b.chars().any(char::is_whitespace)) { e.push(format!("{name} needs a branch name without spaces.")); }
        }
    }
    e
}

fn has_folder(d: &TeamDraft) -> bool { d.folder.as_deref().map(str::trim).is_some_and(|f| !f.is_empty()) }

fn can_finish(w: &Workflow, id: &str, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(id.to_string()) { return false; }
    w.stages.iter().find(|s| s.id == id).is_some_and(|s| s.routes.values().any(|next| match next {
        None => true, Some(next) => can_finish(w, next, seen),
    }) || s.on_limit.as_ref().is_some_and(|next| can_finish(w, next, seen)))
}

/// A team's schedules: at most 8, unique ids, existing agents, every 5–1440 minutes, messages up to 4 KB.
pub(super) fn check_schedules(d: &TeamDraft) -> Vec<DraftIssue> {
    let mut errors = Vec::new();
    let ids: HashSet<_> = d.schedules.iter().map(|s| &s.id).collect();
    if d.schedules.len() > 8 { errors.push("Keep to 8 schedules.".to_string()); }
    if ids.len() != d.schedules.len() { errors.push("Give each schedule its own name.".to_string()); }
    for s in &d.schedules {
        if !model::valid_segment(&s.id) || d.agent(&s.agent).is_none() || !(5..=1440).contains(&s.every_minutes) || s.message.trim().is_empty() || s.message.len() > 4096 {
            errors.push(format!("Schedule {} needs an existing agent, every 5–1440 minutes, and a message of 1–4096 bytes.", s.id));
        }
    }
    errors.into_iter().map(|message| DraftIssue {
        element: ElementRef { kind: ElementKind::Team, id: d.id.clone() }, severity: IssueSeverity::Error,
        code: "schedule-invalid".into(), message, detail: None, spec_path: None,
    }).collect()
}
