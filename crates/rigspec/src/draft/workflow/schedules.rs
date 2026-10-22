//! Validation of scheduled agent messages.
use model::{DraftIssue, ElementKind, ElementRef, IssueSeverity, TeamDraft};
use std::collections::HashSet;

/// A team's schedules: at most 8, unique ids, existing agents, every 5–1440 minutes, messages up to 4 KB.
pub(in crate::draft) fn check_schedules(d: &TeamDraft) -> Vec<DraftIssue> {
    schedule_errors(d)
        .into_iter()
        .map(|message| DraftIssue {
            element: ElementRef {
                kind: ElementKind::Team,
                id: d.id.clone(),
            },
            severity: IssueSeverity::Error,
            code: "schedule-invalid".into(),
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
