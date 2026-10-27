//! Validation of scheduled agent messages, triggers (addresses other systems call) and result checks.
use model::{DraftIssue, ElementKind, ElementRef, IssueSeverity, TeamDraft};
use std::collections::HashSet;

/// A team's schedules (at most 8, unique ids, existing agents, every 5–1440 minutes, messages up to 4 KB),
/// its triggers and its agents' result checks.
pub(in crate::draft) fn check_schedules(d: &TeamDraft) -> Vec<DraftIssue> {
    schedule_errors(d)
        .into_iter()
        .map(|m| ("schedule-invalid", m))
        .chain(
            trigger_errors(d)
                .into_iter()
                .map(|m| ("trigger-invalid", m)),
        )
        .chain(
            result_check_errors(d)
                .into_iter()
                .map(|m| ("result-checks-invalid", m)),
        )
        .map(|(code, message)| DraftIssue {
            element: ElementRef {
                kind: ElementKind::Team,
                id: d.id.clone(),
            },
            severity: IssueSeverity::Error,
            code: code.into(),
            message,
            detail: None,
            spec_path: None,
        })
        .collect()
}

fn schedule_errors(d: &TeamDraft) -> Vec<String> {
    let mut errors = Vec::new();
    let ids: HashSet<_> = d.schedules.iter().map(|s| &s.id).collect();
    if d.schedules.len() > 8 {
        errors.push("Keep to 8 schedules.".to_string());
    }
    if ids.len() != d.schedules.len() {
        errors.push("Give each schedule its own name.".to_string());
    }
    for s in &d.schedules {
        if !model::valid_segment(&s.id)
            || d.agent(&s.agent).is_none()
            || !(5..=1440).contains(&s.every_minutes)
            || s.message.trim().is_empty()
            || s.message.len() > 4096
        {
            errors.push(format!("Schedule {} needs an existing agent, every 5–1440 minutes, and a message of 1–4096 bytes.", s.id));
        }
    }
    errors
}

/// A team's triggers: at most 8, unique ids, existing agents, messages up to 4 KB.
fn trigger_errors(d: &TeamDraft) -> Vec<String> {
    let mut errors = Vec::new();
    let ids: HashSet<_> = d.triggers.iter().map(|t| &t.id).collect();
    if d.triggers.len() > 8 {
        errors.push("Keep to 8 triggers.".to_string());
    }
    if ids.len() != d.triggers.len() {
        errors.push("Give each trigger its own name.".to_string());
    }
    for t in &d.triggers {
        if !model::valid_segment(&t.id)
            || d.agent(&t.agent).is_none()
            || t.message.trim().is_empty()
            || t.message.len() > 4096
        {
            errors.push(format!(
                "Trigger {} needs an existing agent and a message of 1–4096 bytes.",
                t.id
            ));
        }
    }
    errors
}

/// Each agent's result checks: at most 16, unique ids, a field to check.
fn result_check_errors(d: &TeamDraft) -> Vec<String> {
    d.agents
        .iter()
        .filter(|a| {
            let c = &a.advanced.result_checks;
            let ids: HashSet<_> = c.iter().map(|x| &x.id).collect();
            c.len() > 16 || ids.len() != c.len() || c.iter().any(|x| x.field.trim().is_empty())
        })
        .map(|a| {
            format!(
                "{}'s result checks need a field each, their own names, and at most 16.",
                a.name
            )
        })
        .collect()
}
