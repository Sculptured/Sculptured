//! The team a session suggests, from facts in it only: one agent per role (the main agent, and each
//! distinct kind of subagent its parent started), parent → child `hands-work-to` arrows, each agent's
//! own task as its job, the session's folder and the model each used most.
use super::{Ev, Thread};
use crate::portable::session::Source;
use crate::portable::team::PortableTeam;
use model::{AgentAdvanced, AgentKind, ConnectionKind, DraftAgent, DraftConnection, TeamDraft};
use std::collections::HashMap;

/// One agent of the team: the threads that share its role.
struct Role<'a> {
    id: String,
    parent: Option<String>,
    threads: Vec<&'a Thread>,
}

pub(super) fn rebuild(threads: &[Thread], source: Source) -> PortableTeam {
    let roles = roles(threads);
    let names: HashMap<&str, String> = roles.iter().map(|r| (r.id.as_str(), name_of(r))).collect();
    let at = layout(&roles);
    // An agent whose file was not uploaded works where the session did.
    let session_folder = threads
        .iter()
        .find(|t| t.parent.is_none())
        .and_then(|t| t.cwd.clone());
    let agents = roles
        .iter()
        .map(|r| {
            agent(
                r,
                &roles,
                &names,
                at[r.id.as_str()],
                session_folder.as_deref(),
            )
        })
        .collect();
    let pairs = roles
        .iter()
        .filter_map(|r| Some((r.parent.clone()?, r.id.clone())));
    let connections = pairs
        .enumerate()
        .map(|(n, (from, to))| DraftConnection {
            id: format!("c-{}", n + 1),
            from,
            to,
            kind: ConnectionKind::HandsWorkTo,
        })
        .collect();
    let fallback = if source == Source::Codex {
        "Imported Codex session"
    } else {
        "Imported Claude Code session"
    };
    let name = threads
        .iter()
        .find(|t| t.parent.is_none())
        .and_then(|t| t.name.clone())
        .unwrap_or_else(|| fallback.into());
    let team = TeamDraft {
        claude_setup: Default::default(),
        protection: Default::default(),
        safety: vec![],
        components: vec![],
        budget: Default::default(),
        input: None,
        id: "imported-session".into(),
        name,
        revision: 0,
        folder: None,
        models: Default::default(),
        workflow: None,
        schedules: vec![],
        mandate: Default::default(),
        decision_log: false,
        limits: Default::default(),
        groups: vec![],
        agents,
        connections,
    };
    PortableTeam {
        team,
        settings: Default::default(),
    }
}

/// Parents before their children, whatever order the files came in.
fn in_parent_order(threads: &[Thread]) -> Vec<&Thread> {
    let (mut out, mut rest): (Vec<&Thread>, Vec<&Thread>) =
        threads.iter().partition(|t| t.parent.is_none());
    while !rest.is_empty() {
        let (ready, wait): (Vec<&Thread>, Vec<&Thread>) = rest.into_iter().partition(|t| {
            out.iter()
                .any(|o| Some(o.key.as_str()) == t.parent.as_deref())
        });
        if ready.is_empty() {
            out.extend(wait);
            break;
        }
        out.extend(ready);
        rest = wait;
    }
    out
}

/// A named subagent type (Explore, Plan, a custom agent) is a role; a general-purpose or untyped one is
/// its own role per distinct description.
fn role_key(t: &Thread) -> String {
    match t
        .agent_type
        .as_deref()
        .filter(|a| !a.is_empty() && *a != "general-purpose")
    {
        Some(a) => format!("type:{a}"),
        None => format!(
            "desc:{}",
            t.name
                .as_deref()
                .unwrap_or(&t.key)
                .chars()
                .filter(|c| c.is_alphabetic())
                .collect::<String>()
                .to_lowercase()
        ),
    }
}

fn roles(threads: &[Thread]) -> Vec<Role<'_>> {
    let mut roles: Vec<Role> = Vec::new();
    let mut role_of: HashMap<&str, String> = HashMap::new();
    for t in in_parent_order(threads) {
        let parent = t
            .parent
            .as_deref()
            .and_then(|p| role_of.get(p))
            .cloned()
            .or_else(|| t.parent.as_ref().map(|_| "main".to_string()));
        let same = roles.iter_mut().find(|r| {
            parent.is_some() && r.parent == parent && role_key(r.threads[0]) == role_key(t)
        });
        let id = match same {
            Some(r) => {
                r.threads.push(t);
                r.id.clone()
            }
            None => {
                let id = if roles.is_empty() {
                    "main".to_string()
                } else {
                    format!("sub-{}", roles.len())
                };
                roles.push(Role {
                    id: id.clone(),
                    parent: parent.clone(),
                    threads: vec![t],
                });
                id
            }
        };
        role_of.insert(&t.key, id);
    }
    roles
}

fn name_of(r: &Role) -> String {
    let t = r.threads[0];
    let name = if r.parent.is_none() {
        t.name.clone().unwrap_or_else(|| "Main agent".into())
    } else {
        t.name
            .clone()
            .or_else(|| t.agent_type.clone())
            .unwrap_or_else(|| format!("Subagent {}", &r.id[4..]))
    };
    name.chars().take(60).collect()
}

/// Top-down tree: one row per depth, agents side by side in order.
fn layout<'a>(roles: &'a [Role]) -> HashMap<&'a str, (i32, i32)> {
    let mut depth: HashMap<&str, i32> = HashMap::new();
    let mut across: HashMap<i32, i32> = HashMap::new();
    let mut out = HashMap::new();
    for r in roles {
        let d = r
            .parent
            .as_deref()
            .and_then(|p| depth.get(p))
            .map_or(0, |d| d + 1);
        depth.insert(&r.id, d);
        let i = across.entry(d).or_insert(0);
        out.insert(r.id.as_str(), (40 + 280 * *i, 40 + 220 * d));
        *i += 1;
    }
    out
}

fn agent(
    r: &Role,
    roles: &[Role],
    names: &HashMap<&str, String>,
    (x, y): (i32, i32),
    session_folder: Option<&str>,
) -> DraftAgent {
    let t = r.threads[0];
    let kind = if t.runtime == "codex-app-server" {
        AgentKind::Codex
    } else {
        AgentKind::Claude
    };
    let advanced = AgentAdvanced {
        model: model_of(r),
        ..Default::default()
    };
    DraftAgent {
        id: r.id.clone(),
        group_id: None,
        name: names[r.id.as_str()].clone(),
        kind,
        job: job(r, roles, names),
        folder: t.cwd.clone().or_else(|| session_folder.map(str::to_string)),
        x,
        y,
        advanced,
        connection: None,
    }
}

fn first_prompt(t: &Thread) -> Option<String> {
    t.steps.iter().find_map(|s| match &s.ev {
        Ev::Prompt { text, .. } => Some(text.clone()),
        Ev::Body(..) => None,
    })
}

/// Its own words: the session's first prompt (main) or the task its parent gave it, plus how often it
/// was started and, for a parent, who its teammates are.
fn job(r: &Role, roles: &[Role], names: &HashMap<&str, String>) -> String {
    let t = r.threads[0];
    let own = if r.parent.is_none() {
        first_prompt(t)
    } else {
        t.task.clone().or_else(|| first_prompt(t))
    };
    let mut job = own.unwrap_or_default();
    if r.threads.len() > 1 {
        job.push_str(&format!(
            "\n\n(In the original session this was started {} times, with different tasks.)",
            r.threads.len()
        ));
    }
    let kids: Vec<&str> = roles
        .iter()
        .filter(|c| c.parent.as_deref() == Some(r.id.as_str()))
        .map(|c| names[c.id.as_str()].as_str())
        .collect();
    if !kids.is_empty() {
        job.push_str(&format!("\n\nYour teammates: {}. Hand them work with send_agent_message rather than starting new subagents.", kids.join(", ")));
    }
    job
}

/// The model its threads used most (the first seen wins a tie).
fn model_of(r: &Role) -> Option<String> {
    let mut count: HashMap<&str, (usize, usize)> = HashMap::new();
    let indexed_threads = r.threads.iter().flat_map(|t| &t.models).enumerate();
    for (i, m) in indexed_threads {
        count.entry(m.as_str()).or_insert((0, i)).0 += 1;
    }
    count
        .into_iter()
        .max_by_key(|(_, (n, first))| (*n, std::cmp::Reverse(*first)))
        .map(|(m, _)| m.to_string())
}
