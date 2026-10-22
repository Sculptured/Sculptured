//! Compiler diagnostics → the draft element they concern, in plain words.
use super::build::SpecIndex;
use super::{GENERIC_AGENT, GENERIC_CONNECTION, GENERIC_GROUP, GENERIC_TEAM};
use crate::{Diagnostic, Severity};
use model::{DraftIssue, ElementKind, ElementRef, IssueSeverity, TeamDraft};

fn bracket<'p>(path: &'p str, prefix: &str) -> Option<(usize, &'p str)> {
    let rest = path.strip_prefix(prefix)?.strip_prefix('[')?;
    let (n, rest) = rest.split_once(']')?;
    Some((n.parse().ok()?, rest))
}

fn at(kind: ElementKind, id: &str) -> ElementRef {
    ElementRef {
        kind,
        id: id.to_string(),
    }
}

/// `pods[p]…` paths: a member, a pod-local edge, or the pod itself.
fn locate_in_pod(index: &SpecIndex, p: usize, rest: &str) -> Option<(ElementRef, String)> {
    if let Some((m, field)) = bracket(rest, ".members")
        && let Some(id) = index.members.get(p).and_then(|v| v.get(m))
    {
        return Some((at(ElementKind::Agent, id), field.to_string()));
    }
    if let Some((e, field)) = bracket(rest, ".edges")
        && let Some(id) = index.pod_edges.get(p).and_then(|v| v.get(e))
    {
        return Some((at(ElementKind::Connection, id), field.to_string()));
    }
    index
        .pods
        .get(p)
        .map(|id| (at(ElementKind::Group, id), rest.to_string()))
}

fn locate(d: &TeamDraft, index: &SpecIndex, path: &str) -> (ElementRef, String) {
    if let Some((p, rest)) = bracket(path, "pods")
        && let Some(found) = locate_in_pod(index, p, rest)
    {
        return found;
    }
    if let Some((e, field)) = bracket(path, "edges")
        && let Some(id) = index.edges.get(e)
    {
        return (at(ElementKind::Connection, id), field.to_string());
    }
    (at(ElementKind::Team, &d.id), path.to_string())
}

pub(super) fn map_with(d: &TeamDraft, index: &SpecIndex, diag: &Diagnostic) -> DraftIssue {
    let (element, field) = locate(d, index, &diag.path);
    let severity = if diag.severity == Severity::Error {
        IssueSeverity::Error
    } else {
        IssueSeverity::Warning
    };
    let who = d
        .agent(&element.id)
        .map(crate::draft::names::display_name)
        .unwrap_or_else(|| "This agent".to_string());
    let (code, message) = match (element.kind, field.as_str()) {
        (ElementKind::Agent, ".cwd") => {
            ("folder-missing", format!("{who} needs a folder to work in"))
        }
        (ElementKind::Agent, _) => ("agent-config", GENERIC_AGENT.to_string()),
        (ElementKind::Connection, _) => ("connection-config", GENERIC_CONNECTION.to_string()),
        (ElementKind::Group, _) => ("group-config", GENERIC_GROUP.to_string()),
        (ElementKind::Team, _) => ("team-config", GENERIC_TEAM.to_string()),
    };
    DraftIssue {
        element,
        severity,
        code: code.to_string(),
        message,
        detail: Some(diag.message.clone()),
        spec_path: Some(diag.path.clone()),
    }
}
