//! Checks of an agent's folder that need the filesystem.
use crate::folders::{self, Home};
use crate::state::AppState;
use model::{AgentKind, DraftAgent, DraftIssue, ElementKind, ElementRef, IssueSeverity, TeamDraft};

/// Hosted: a team's folders and agents are on the user's computers, so without those connected it
/// can't start: the team's computer, and each agent's own. Local `serve` runs everything on this
/// machine and never asks.
pub(crate) fn computer_issues(st: &AppState, d: &TeamDraft) -> Vec<DraftIssue> {
    if !matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. }) {
        return Vec::new();
    }
    let own = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .team_settings(&d.id)
        .map(|t| t.agent_hosts)
        .unwrap_or_default();
    let mine = |a: &DraftAgent| own.get(&a.id).filter(|h| !h.is_empty());
    let mut issues: Vec<DraftIssue> = d.agents.iter().filter_map(|a| {
        let h = mine(a).filter(|h| !st.hosts.is_connected(h))?;
        Some(DraftIssue {
            element: ElementRef { kind: ElementKind::Agent, id: a.id.clone() },
            severity: IssueSeverity::Error,
            code: "computer-not-connected".to_string(),
            message: format!("{h} is not connected, and {} runs there. Open Computers to reconnect it, or choose another computer for this agent.", a.name),
            detail: None,
            spec_path: None,
        })
    }).collect();
    if d.agents.iter().any(|a| mine(a).is_none()) {
        issues.extend(team_computer_issue(st, d));
    }
    issues
}

fn team_computer_issue(st: &AppState, d: &TeamDraft) -> Option<DraftIssue> {
    let host = crate::launch::team_host(st, &d.id);
    if st.hosts.is_connected(&host) {
        return None;
    }
    let chosen = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .team_settings(&d.id)
        .ok()
        .and_then(|t| t.host)
        .filter(|h| !h.is_empty());
    let message = match (chosen, st.hosts.connected().len()) {
        (Some(h), _) => format!("{h} is not connected. Open Computers to reconnect it, or choose another computer for this team."),
        (None, 0) => "Connect your computer: your folders and agents run there, not on the server. Open Computers and choose Connect a computer.".to_string(),
        (None, _) => "Choose which connected computer runs this team (Run on).".to_string(),
    };
    Some(DraftIssue {
        element: ElementRef {
            kind: ElementKind::Team,
            id: d.id.clone(),
        },
        severity: IssueSeverity::Error,
        code: "computer-not-connected".to_string(),
        message,
        detail: None,
        spec_path: None,
    })
}

/// Checks that need the filesystem, so they live here and not in `rigspec`. An agent's
/// folder must sit inside the home folder, checked with the folder picker's own rule
/// (`folders::resolve_inside`): a place outside home gets the same words whether or not
/// it exists. A Practice agent without a folder runs in the home folder (`.` in its spec).
pub(crate) fn folder_issues(st: &AppState, d: &TeamDraft) -> Vec<DraftIssue> {
    // Hosted: folders are on the user's computer, whose runner enforces its own allowed folders
    // (`hostd::policy`); the server's home folder says nothing about them.
    if matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. }) {
        return Vec::new();
    }
    let home = folders::home(st).ok();
    d.agents
        .iter()
        .filter_map(|a| folder_issue(home.as_ref(), a, d.effective_folder(a)))
        .collect()
}

fn agent_issue(a: &DraftAgent, code: &str, message: String, detail: Option<String>) -> DraftIssue {
    DraftIssue {
        element: ElementRef {
            kind: ElementKind::Agent,
            id: a.id.clone(),
        },
        severity: IssueSeverity::Error,
        code: code.to_string(),
        message,
        detail,
        spec_path: None,
    }
}

/// macOS keeps Desktop, Documents and Downloads from background programs until they're allowed.
fn macos_hint() -> Option<String> {
    cfg!(target_os = "macos").then(|| "In System Settings → Privacy & Security, allow Sculptured under Files and Folders or Full Disk Access.".into())
}

/// A blank or relative folder is `rigspec`'s issue (folder-missing, folder-not-absolute).
fn folder_issue(home: Option<&Home>, a: &DraftAgent, folder: Option<&str>) -> Option<DraftIssue> {
    let who = rigspec::draft::display_name(a);
    match (folder.filter(|f| f.starts_with('/')), home) {
        (Some(_), None) => Some(agent_issue(
            a,
            "home-unknown",
            format!("Your home folder is unknown, so {who}'s folder can't be checked"),
            None,
        )),
        (Some(f), Some(home)) => {
            let e = folders::resolve_inside(home, std::path::Path::new(f)).err()?;
            Some(match e.code.as_ref() {
                "outside_home" => agent_issue(
                    a,
                    "folder-outside-home",
                    format!("{who}'s folder is outside your home folder"),
                    None,
                ),
                "folder_missing" => agent_issue(
                    a,
                    "folder-not-found",
                    format!("{who}'s folder no longer exists: {f}"),
                    None,
                ),
                "not_a_folder" => agent_issue(
                    a,
                    "folder-not-a-folder",
                    format!("{who}'s folder is a file, not a folder: {f}"),
                    None,
                ),
                "folder_unreadable" => agent_issue(
                    a,
                    "folder-unreadable",
                    format!("Sculptured can't read {who}'s folder: {f}"),
                    macos_hint(),
                ),
                // `.`/`..` components, NUL bytes, link loops, names that are too long.
                _ => agent_issue(
                    a,
                    "folder-bad-path",
                    format!("{who}'s folder path can't be used: {f}"),
                    Some(e.message),
                ),
            })
        }
        (None, None) if a.kind == AgentKind::Practice && folder.is_none() => Some(agent_issue(
            a,
            "home-unknown",
            format!("Your home folder is unknown, so {who} needs a folder"),
            None,
        )),
        (None, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthPolicy;
    use crate::store::Store;
    use crate::views::TeamSettings;
    use std::sync::Arc;

    #[test]
    fn hosted_an_agent_on_an_offline_computer_is_flagged_on_the_agent() {
        let st = AppState::new(Store::open_in_memory().unwrap(), "t".into(), vec![]).with_auth(
            AuthPolicy::Gateway {
                secret: Arc::new(b"s".to_vec()),
            },
        );
        st.hosts.test_mark_connected("laptop");
        let d = rigspec::textbook::template("book", "Book", true);
        let (agent, name) = (d.agents[0].id.clone(), d.agents[0].name.clone());
        let settings = TeamSettings {
            host: Some("laptop".into()),
            agent_hosts: [(agent.clone(), "hpc-login".to_string())].into(),
            ..Default::default()
        };
        st.store
            .lock()
            .unwrap()
            .save_team_settings(&d.id, &settings)
            .unwrap();
        let issues = computer_issues(&st, &d);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(
            (issues[0].element.kind, issues[0].element.id.as_str()),
            (ElementKind::Agent, agent.as_str())
        );
        assert!(
            issues[0].message.contains("hpc-login") && issues[0].message.contains(&name),
            "{}",
            issues[0].message
        );
        st.hosts.test_mark_connected("hpc-login");
        assert!(computer_issues(&st, &d).is_empty());
    }
}
