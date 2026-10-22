use model::{
    CopiesMode, DraftIssue, ElementKind, ElementRef, IssueSeverity, StageKind, TeamDraft, Workflow,
    WorkflowStage,
};
use std::collections::HashSet;

pub(in crate::draft) fn check(d: &TeamDraft) -> Vec<DraftIssue> {
    let Some(w) = &d.workflow else { return vec![] };
    let issue = |code: &str, message: String| DraftIssue {
        element: ElementRef {
            kind: ElementKind::Team,
            id: d.id.clone(),
        },
        severity: IssueSeverity::Error,
        code: code.into(),
        message,
        detail: None,
        spec_path: None,
    };
    let mut issues: Vec<DraftIssue> = errors(d, w)
        .into_iter()
        .map(|m| issue("workflow-invalid", m))
        .collect();
    let needs = folder_users(w);
    if !has_folder(d) && !needs.is_empty() {
        issues.push(issue(
            "project-folder-needed",
            format!(
                "Choose the project folder: {} {} there.",
                list(&needs),
                if needs.len() == 1 { "runs" } else { "run" }
            ),
        ));
    }
    issues
}

/// What runs in the project folder: per-item copies, the guard, a dispatch command and check stages.
fn folder_users(w: &Workflow) -> Vec<String> {
    let mut v = Vec::new();
    if w.copies == CopiesMode::PerItem {
        v.push("a copy per item".to_string());
    }
    if w.guard.is_some() {
        v.push("the guard".into());
    }
    if w.dispatch.is_some() {
        v.push("the dispatch command".into());
    }
    v.extend(
        w.stages
            .iter()
            .filter(|s| s.kind == StageKind::Check)
            .map(|s| s.name.trim().to_string()),
    );
    v
}

/// "a", "a and b", "a, b and c".
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn errors(d: &TeamDraft, w: &Workflow) -> Vec<String> {
    let mut errors = workflow_errors(d, w);
    let ids: HashSet<_> = w.stages.iter().map(|s| &s.id).collect();
    if w.stages.is_empty()
        || w.stages.len() > 16
        || ids.len() != w.stages.len()
        || !ids.contains(&w.entry)
    {
        errors.push("Choose an entry stage and 1–16 stages with unique IDs.".into());
    }
    for s in &w.stages {
        errors.extend(route_errors(s, &ids));
        errors.extend(super::stages::errors(d, w, s));
        errors.extend(escalation_errors(d, w, s));
        errors.extend(round_errors(s, &ids));
        if !can_finish(w, &s.id, &mut HashSet::new()) {
            errors.push(format!("{} has no route to completion.", s.name));
        }
    }
    errors
}

fn workflow_errors(d: &TeamDraft, w: &Workflow) -> Vec<String> {
    let mut errors = Vec::new();
    if w.input.len() > 64000 {
        errors.push(
            "Keep the Input block under 64 KB; reference larger source files by path or link."
                .into(),
        );
    }
    if !(1..=64).contains(&w.max_tasks) || !(1..=32).contains(&w.max_steps) {
        errors.push("Dynamic dispatch needs 1–64 tasks and 1–32 steps per task.".into());
    }
    errors.extend(dispatch_errors(d, w));
    if !d.connections.is_empty() {
        errors.push("Dynamic workflows use stage routes instead of ordinary connections. Remove ordinary connections first.".into());
    }
    errors.extend(guard_errors(w));
    errors
}

fn guard_errors(w: &Workflow) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(g) = &w.guard {
        if g.command.trim().is_empty() || g.command.len() > 2000 {
            errors.push("The guard needs a command of 1–2000 bytes.".into());
        }
        if !(5..=1440).contains(&g.every_minutes) {
            errors.push("The guard runs every 5–1440 minutes.".into());
        }
        if g.timeout_s.is_some_and(|t| !(1..=3600).contains(&t)) {
            errors.push("The guard needs a time limit of 1–3600 seconds.".into());
        }
    }
    errors
}

fn route_errors(s: &WorkflowStage, ids: &HashSet<&String>) -> Vec<String> {
    let mut errors = Vec::new();
    if !model::valid_segment(&s.id) || s.id == "assembly" {
        errors.push(format!("{} needs a valid stage ID.", s.name));
    }
    if s.routes.is_empty()
        || s.routes.len() > 8
        || s.routes
            .keys()
            .any(|k| !model::valid_segment(k) || k == "blocked")
        || s.routes.values().flatten().any(|id| !ids.contains(id))
    {
        errors.push(format!(
            "{} needs 1–8 named routes to existing stages or completion; blocked is reserved.",
            s.name
        ));
    }
    errors
}

fn escalation_errors(d: &TeamDraft, w: &Workflow, s: &WorkflowStage) -> Vec<String> {
    let mut errors = Vec::new();
    if let Some(e) = &s.escalate {
        let single = s.kind == StageKind::Agent && s.reviewers.is_empty();
        let agent_ok = d.agent(&e.agent).is_some()
            && e.agent != s.agent
            && e.agent != w.dispatcher
            && e.agent != w.collector;
        if !single
            || !agent_ok
            || !(1..=20).contains(&e.after_rounds)
            || s.max_rounds.is_some_and(|m| e.after_rounds >= m)
        {
            errors.push(format!("{} hands items over after 1–20 rounds (fewer than its round limit) to another existing agent; only a single-agent stage can.", s.name));
        }
    }
    errors
}

fn round_errors(s: &WorkflowStage, ids: &HashSet<&String>) -> Vec<String> {
    let mut errors = Vec::new();
    if s.max_rounds.is_some_and(|m| !(1..=20).contains(&m)) {
        errors.push(format!("{} allows 1–20 rounds.", s.name));
    }
    if s.on_limit.is_some() && s.max_rounds.is_none() {
        errors.push(format!(
            "{} has a round-limit route but no round limit.",
            s.name
        ));
    }
    if s.on_limit
        .as_ref()
        .is_some_and(|id| !ids.contains(id) || *id == s.id)
    {
        errors.push(format!(
            "{}'s round-limit route must name another existing stage.",
            s.name
        ));
    }
    errors
}

/// Who makes the batch: a dispatcher agent, or a dispatch command (never both); and the assembly agent.
fn dispatch_errors(d: &TeamDraft, w: &Workflow) -> Vec<String> {
    let Some(c) = &w.dispatch else {
        let ok = d.agent(&w.dispatcher).is_some()
            && d.agent(&w.collector).is_some()
            && w.dispatcher != w.collector;
        return if ok {
            vec![]
        } else {
            vec!["Choose different, existing dispatcher and assembly agents.".into()]
        };
    };
    let mut e = Vec::new();
    if !w.dispatcher.is_empty() {
        e.push("Choose a dispatcher agent or a dispatch command, not both.".into());
    }
    if d.agent(&w.collector).is_none() {
        e.push("Choose an existing assembly agent.".into());
    }
    if c.command.trim().is_empty() || c.command.len() > 2000 {
        e.push("The dispatch command needs 1–2000 bytes.".into());
    }
    if c.timeout_s.is_some_and(|t| !(1..=3600).contains(&t)) {
        e.push("The dispatch command needs a time limit of 1–3600 seconds.".into());
    }
    e
}

fn has_folder(d: &TeamDraft) -> bool {
    d.folder
        .as_deref()
        .map(str::trim)
        .is_some_and(|f| !f.is_empty())
}

fn can_finish(w: &Workflow, id: &str, seen: &mut HashSet<String>) -> bool {
    if !seen.insert(id.to_string()) {
        return false;
    }
    w.stages.iter().find(|s| s.id == id).is_some_and(|s| {
        s.routes.values().any(|next| match next {
            None => true,
            Some(next) => can_finish(w, next, seen),
        }) || s
            .on_limit
            .as_ref()
            .is_some_and(|next| can_finish(w, next, seen))
    })
}
