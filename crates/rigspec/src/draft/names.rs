//! Ids and words drawn from a draft: slugs, connection verbs, spec field values, agent names.
use model::{AgentKind, ConnectionKind, DraftAgent, ResumeBehaviour, TeamDraft};

const MAX_SLUG: usize = 48;

/// Lowercase ASCII letters and digits, other runs become one `-`. May be empty.
#[must_use]
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    let matching_name = name.trim().chars();
    for c in matching_name {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    out.truncate(MAX_SLUG);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// `slug(name)` (or `fallback`), suffixed `-2`, `-3`, … until `taken` says it is free.
///
/// # Panics
/// Panics if `taken` rejects the base and every suffix representable by `u32`.
pub fn unique_slug(name: &str, fallback: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = match slug(name) {
        s if s.is_empty() => fallback.to_string(),
        s => s,
    };
    if !taken(&base) {
        return base;
    }
    (2u32..)
        .map(|n| format!("{base}-{n}"))
        .find(|c| !taken(c))
        .expect("an unbounded range always finds a free id")
}

#[must_use]
pub fn verb(kind: ConnectionKind) -> &'static str {
    match kind {
        ConnectionKind::HandsWorkTo => "hands work to",
        ConnectionKind::ReportsTo => "reports to",
        ConnectionKind::Watches => "watches",
    }
}

pub(super) fn runtime_of(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "claude-code",
        AgentKind::Codex => "codex",
        AgentKind::Opencode => "opencode",
        AgentKind::Practice => "stub",
    }
}

pub(super) fn edge_kind_of(kind: ConnectionKind) -> &'static str {
    match kind {
        ConnectionKind::HandsWorkTo => "delegates_to",
        ConnectionKind::ReportsTo => "escalates_to",
        ConnectionKind::Watches => "can_observe",
    }
}

pub(super) fn restore_of(r: ResumeBehaviour) -> &'static str {
    match r {
        ResumeBehaviour::ResumeIfPossible => "resume_if_possible",
        ResumeBehaviour::RelaunchFresh => "relaunch_fresh",
        ResumeBehaviour::CheckpointOnly => "checkpoint_only",
    }
}

/// An agent's name for sentences: its trimmed name, else "This agent". `teams::folder_issues` uses it too.
#[must_use]
pub fn display_name(a: &DraftAgent) -> String {
    match a.name.trim() {
        "" => "This agent".to_string(),
        n => n.to_string(),
    }
}

pub(super) fn name_of(d: &TeamDraft, id: &str) -> String {
    d.agent(id)
        .map(display_name)
        .unwrap_or_else(|| "An agent".to_string())
}
