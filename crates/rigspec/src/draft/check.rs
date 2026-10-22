//! Checks that need the draft's structure, in plain words. Pure: no filesystem access.
use super::names::display_name;
use super::{GENERIC_AGENT, GENERIC_GROUP, GENERIC_TEAM};
use ElementKind::{Agent, Connection, Group, Team};
use IssueSeverity::Error;
use model::{
    AgentKind, ConnectionKind, DraftAgent, DraftConnection, DraftIssue, ElementKind, ElementRef,
    IssueSeverity, TeamDraft,
};
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) fn issue(
    kind: ElementKind,
    id: &str,
    severity: IssueSeverity,
    code: &str,
    message: impl Into<String>,
) -> DraftIssue {
    DraftIssue {
        element: ElementRef {
            kind,
            id: id.to_string(),
        },
        severity,
        code: code.to_string(),
        message: message.into(),
        detail: None,
        spec_path: None,
    }
}

fn with_detail(mut i: DraftIssue, detail: String) -> DraftIssue {
    i.detail = Some(detail);
    i
}

/// Checks that need the draft's structure, in plain words. Pure: no filesystem access.
#[must_use]
pub fn check_draft(d: &TeamDraft) -> Vec<DraftIssue> {
    let mut out = Vec::new();
    check_team(d, &mut out);
    check_groups(d, &mut out);
    check_agents(d, &mut out);
    check_connections(d, &mut out);
    out.extend(loop_issues(d));
    out.extend(super::budget::budget_issues(d));
    out
}

fn check_team(d: &TeamDraft, out: &mut Vec<DraftIssue>) {
    if d.name.trim().is_empty() {
        out.push(issue(
            Team,
            &d.id,
            Error,
            "team-name-missing",
            "Give the team a name",
        ));
    }
    if d.workflow.is_none()
        && d.input
            .as_ref()
            .is_some_and(|i| !i.to.iter().any(|a| d.agent(a).is_some()))
    {
        let message = "The Input isn't connected to an agent, so no agent gets it; drag from the Input to an agent";
        out.push(issue(
            Team,
            &d.id,
            IssueSeverity::Warning,
            "input-unconnected",
            message,
        ));
    }
    if !model::valid_segment(&d.id) {
        let i = issue(Team, &d.id, Error, "team-id-invalid", GENERIC_TEAM);
        out.push(with_detail(
            i,
            format!("team id {:?} must use letters, digits, '_' or '-'", d.id),
        ));
    }
    if d.agents.is_empty() {
        out.push(issue(
            Team,
            &d.id,
            Error,
            "team-empty",
            "Add at least one agent to start the team",
        ));
    }
    let flattened_safety = d.safety.iter().flat_map(model::SafetyRule::problems);
    for problem in flattened_safety {
        out.push(issue(Team, &d.id, Error, "safety-rule-invalid", problem));
    }
    let builtin: Vec<String> = model::component::builtins()
        .into_iter()
        .map(|c| c.id)
        .collect();
    for c in &d.components {
        let matching_c = c.problems();
        for problem in matching_c {
            out.push(issue(Team, &d.id, Error, "component-invalid", problem));
        }
        if builtin.contains(&c.id) {
            out.push(issue(
                Team,
                &d.id,
                Error,
                "component-invalid",
                format!(
                    "{:?} is a built-in component's id; give yours another one.",
                    c.id
                ),
            ));
        }
    }
}

fn check_groups(d: &TeamDraft, out: &mut Vec<DraftIssue>) {
    let mut group_ids = HashSet::new();
    for g in &d.groups {
        if !model::valid_segment(&g.id) || !group_ids.insert(g.id.as_str()) {
            let i = issue(Group, &g.id, Error, "group-id-invalid", GENERIC_GROUP);
            out.push(with_detail(
                i,
                format!(
                    "group id {:?} is not a valid segment or is used twice",
                    g.id
                ),
            ));
        }
        if g.label.trim().is_empty() {
            out.push(issue(
                Group,
                &g.id,
                Error,
                "group-name-missing",
                "A group needs a name",
            ));
        }
    }
}

fn check_agents(d: &TeamDraft, out: &mut Vec<DraftIssue>) {
    let mut agent_ids = HashSet::new();
    let mut names: HashMap<String, usize> = HashMap::new();
    for a in &d.agents {
        if !model::valid_segment(&a.id) || !agent_ids.insert(a.id.as_str()) {
            let i = issue(Agent, &a.id, Error, "agent-id-invalid", GENERIC_AGENT);
            out.push(with_detail(
                i,
                format!(
                    "agent id {:?} is not a valid segment or is used twice",
                    a.id
                ),
            ));
        }
        check_agent_name(a, &mut names, out);
        check_agent_place(d, a, out);
        let matching_integrations = a
            .advanced
            .integrations
            .iter()
            .filter(|id| d.component(id).is_none());
        for id in matching_integrations {
            out.push(issue(Agent, &a.id, Error, "component-unknown", format!("{} uses the component {id:?}, which this team does not have: add it under Advanced → Components, or remove it from the agent.", display_name(a))));
        }
        for problem in model::skill::skills_problems(&a.advanced.skills) {
            out.push(issue(
                Agent,
                &a.id,
                Error,
                "skill-invalid",
                format!("{}: {problem}", crate::draft::names::display_name(a)),
            ));
        }
    }
}

/// A name is required and unique (ignoring case and outer spaces); `names` counts those seen.
fn check_agent_name(a: &DraftAgent, names: &mut HashMap<String, usize>, out: &mut Vec<DraftIssue>) {
    let key = a.name.trim().to_lowercase();
    if key.is_empty() {
        out.push(issue(
            Agent,
            &a.id,
            Error,
            "agent-name-missing",
            "An agent needs a name",
        ));
        return;
    }
    let seen = names.entry(key).or_insert(0);
    *seen += 1;
    if *seen > 1 {
        let who = crate::draft::names::display_name(a);
        out.push(issue(
            Agent,
            &a.id,
            Error,
            "agent-name-duplicate",
            format!("Two agents are called {who}; give each its own name"),
        ));
    }
}

/// A full path on the computer that runs the agent: `/…` on Linux and macOS; `C:\…`, `C:/…` or a
/// network share `\\server\…` on Windows (a connected Windows computer's folders).
pub(crate) fn is_full_path(f: &str) -> bool {
    let b = f.as_bytes();
    f.starts_with('/')
        || f.starts_with(r"\\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// The agent's group exists, and it has a full-path folder unless it is a Practice agent.
fn check_agent_place(d: &TeamDraft, a: &DraftAgent, out: &mut Vec<DraftIssue>) {
    let who = crate::draft::names::display_name(a);
    if let Some(g) = &a.group_id
        && !d.groups.iter().any(|x| &x.id == g)
    {
        out.push(issue(
            Agent,
            &a.id,
            Error,
            "agent-group-missing",
            format!("{who} is in a group that no longer exists"),
        ));
    }
    match d.effective_folder(a) {
        None if a.kind != AgentKind::Practice => {
            out.push(issue(
                Agent,
                &a.id,
                Error,
                "folder-missing",
                format!("{who} needs a folder to work in"),
            ));
        }
        Some(f) if !is_full_path(f) => {
            let message = format!(
                "{who}'s folder must be a full path, such as /home/you/project or C:\\Users\\you\\project"
            );
            out.push(issue(Agent, &a.id, Error, "folder-not-absolute", message));
        }
        _ => {}
    }
}

fn check_connections(d: &TeamDraft, out: &mut Vec<DraftIssue>) {
    let mut pairs = HashSet::new();
    for c in &d.connections {
        check_connection(d, c, &mut pairs, out);
    }
}

/// Both ends exist and differ, and the same (from, to, kind) is drawn once; `pairs` holds those seen.
fn check_connection<'a>(
    d: &TeamDraft,
    c: &'a DraftConnection,
    pairs: &mut HashSet<(&'a str, &'a str, ConnectionKind)>,
    out: &mut Vec<DraftIssue>,
) {
    let (Some(f), Some(t)) = (d.agent(&c.from), d.agent(&c.to)) else {
        let message = "This connection points to an agent that is no longer in the team";
        out.push(issue(
            Connection,
            &c.id,
            Error,
            "connection-dangling",
            message,
        ));
        return;
    };
    if f.id == t.id {
        out.push(issue(
            Connection,
            &c.id,
            Error,
            "connection-self",
            format!(
                "{}'s connection loops back to itself",
                crate::draft::names::display_name(f)
            ),
        ));
        return;
    }
    if !pairs.insert((c.from.as_str(), c.to.as_str(), c.kind)) {
        let message = format!(
            "{} already {} {}; remove the extra connection",
            crate::draft::names::display_name(f),
            crate::draft::names::verb(c.kind),
            crate::draft::names::display_name(t)
        );
        out.push(issue(
            Connection,
            &c.id,
            Error,
            "connection-duplicate",
            message,
        ));
    }
}

/// Shortest hands-work-to path from `start` to `goal`, as agent ids (both ends included).
fn shortest_path<'a>(
    next: &HashMap<&'a str, Vec<&'a str>>,
    start: &'a str,
    goal: &'a str,
) -> Option<Vec<&'a str>> {
    let mut prev: HashMap<&'a str, &'a str> = HashMap::new();
    let mut seen: HashSet<&'a str> = HashSet::from([start]);
    let mut queue: VecDeque<&'a str> = VecDeque::from([start]);
    while let Some(n) = queue.pop_front() {
        if n == goal {
            return Some(path_back(&prev, goal));
        }
        let matching_next = next.get(n).map(Vec::as_slice).unwrap_or(&[]);
        for &m in matching_next {
            if seen.insert(m) {
                prev.insert(m, n);
                queue.push_back(m);
            }
        }
    }
    None
}

/// The path that ends at `goal`, following `prev` back to its start.
fn path_back<'a>(prev: &HashMap<&'a str, &'a str>, goal: &'a str) -> Vec<&'a str> {
    let mut path = vec![goal];
    let mut cur = goal;
    while let Some(&before) = prev.get(cur) {
        path.push(before);
        cur = before;
    }
    path.reverse();
    path
}

/// Every agent work can reach along "hands work to" from an agent nobody hands work to.
fn reached_from_starts<'a>(
    d: &'a TeamDraft,
    next: &HashMap<&'a str, Vec<&'a str>>,
) -> HashSet<&'a str> {
    let handed: HashSet<&str> = next.values().flatten().copied().collect();
    let mut seen: HashSet<&str> = d
        .agents
        .iter()
        .map(|a| a.id.as_str())
        .filter(|a| !handed.contains(a))
        .collect();
    let mut todo: Vec<&str> = seen.iter().copied().collect();
    while let Some(n) = todo.pop() {
        let matching_next = next.get(n).map(Vec::as_slice).unwrap_or(&[]);
        for &m in matching_next {
            if seen.insert(m) {
                todo.push(m);
            }
        }
    }
    seen
}

fn loop_issues(d: &TeamDraft) -> Vec<DraftIssue> {
    let hands: Vec<_> = d
        .connections
        .iter()
        .filter(|c| {
            c.kind == ConnectionKind::HandsWorkTo
                && c.from != c.to
                && d.agent(&c.from).is_some()
                && d.agent(&c.to).is_some()
        })
        .collect();
    let mut next: HashMap<&str, Vec<&str>> = HashMap::new();
    for c in &hands {
        next.entry(c.from.as_str()).or_default().push(c.to.as_str());
    }
    let started = reached_from_starts(d, &next);
    let mut out = Vec::new();
    for c in hands {
        // A loop is fine once work can reach it from an agent nobody hands work to.
        if started.contains(c.from.as_str()) {
            continue;
        }
        let Some(back) = shortest_path(&next, c.to.as_str(), c.from.as_str()) else {
            continue;
        };
        let ring: Vec<String> = std::iter::once(c.from.as_str())
            .chain(back)
            .map(|id| crate::draft::names::name_of(d, id))
            .collect();
        let message = format!(
            "{} → {} is part of a loop ({}) that no other agent hands work into, so the work has no starting point; connect an agent outside the loop to it, or remove one connection",
            crate::draft::names::name_of(d, &c.from),
            crate::draft::names::name_of(d, &c.to),
            ring.join(" → ")
        );
        out.push(issue(Connection, &c.id, Error, "connection-loop", message));
    }
    out
}
