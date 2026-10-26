//! Draft → RigSpec, recording where each spec element came from.
use super::{AGENT_REF, RESTRICTED_POLICY, SPEC_VERSION};
use crate::{EdgeSpec, MemberSpec, PodSpec, RigSpec};
use model::{AgentKind, DEFAULT_GROUP_ID, DraftAgent, PermissionPosture, TeamDraft};

/// Where each spec element came from, so diagnostic paths map back to draft ids.
pub(super) struct SpecIndex {
    pub(super) pods: Vec<String>,
    pub(super) members: Vec<Vec<String>>,
    pub(super) pod_edges: Vec<Vec<String>>,
    pub(super) edges: Vec<String>,
}

fn member(d: &TeamDraft, a: &DraftAgent) -> MemberSpec {
    MemberSpec {
        id: a.id.clone(),
        agent_ref: AGENT_REF.into(),
        profile: "default".into(),
        runtime: crate::draft::names::runtime_of(a.kind).into(),
        cwd: d.effective_folder(a).unwrap_or(".").to_string(),
        label: Some(a.name.trim().to_string()).filter(|n| !n.is_empty()),
        model: d.effective_model(a),
        effort: None,
        permission_policy: (a.advanced.permission == PermissionPosture::Restricted)
            .then(|| RESTRICTED_POLICY.to_string()),
        role: None,
        restore_policy: Some(crate::draft::names::restore_of(a.advanced.resume).to_string()),
        compaction_strategy: None,
        mechanic: None,
        codex_config_profile: None,
        session_source: None,
        starter_ref: None,
        startup: None,
        // Model connections serve Claude Code and OpenCode agents (OpenRouter keys).
        connection: matches!(a.kind, AgentKind::Claude | AgentKind::Opencode)
            .then(|| {
                a.connection
                    .as_deref()
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(str::to_string)
            })
            .flatten(),
    }
}

/// `team` first when an agent has no group, then drawn groups that hold agents, then any other
/// group an agent names.
fn pod_ids(d: &TeamDraft) -> Vec<String> {
    let mut pod_ids: Vec<String> = Vec::new();
    if d.agents.iter().any(|a| a.group_id.is_none()) {
        pod_ids.push(DEFAULT_GROUP_ID.to_string());
    }
    for g in &d.groups {
        if !pod_ids.contains(&g.id)
            && d.agents
                .iter()
                .any(|a| a.group_id.as_deref() == Some(g.id.as_str()))
        {
            pod_ids.push(g.id.clone());
        }
    }
    for a in &d.agents {
        let p = d.pod_of(a);
        if !pod_ids.iter().any(|x| x == p) {
            pod_ids.push(p.to_string());
        }
    }
    pod_ids
}

/// The group's trimmed label, else the team's.
fn pod_label(d: &TeamDraft, pid: &str, team_label: &str) -> String {
    d.groups
        .iter()
        .find(|g| g.id == pid)
        .map(|g| g.label.trim().to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| team_label.to_string())
}

fn build_pod(d: &TeamDraft, pid: &str, team_label: &str, index: &mut SpecIndex) -> PodSpec {
    let agents: Vec<&DraftAgent> = d.agents.iter().filter(|a| d.pod_of(a) == pid).collect();
    index
        .members
        .push(agents.iter().map(|a| a.id.clone()).collect());
    let mut edges = Vec::new();
    let mut edge_ids = Vec::new();
    for c in &d.connections {
        let (Some(f), Some(t)) = (d.agent(&c.from), d.agent(&c.to)) else {
            continue;
        };
        if d.pod_of(f) == pid && d.pod_of(t) == pid {
            edges.push(EdgeSpec {
                kind: crate::draft::names::edge_kind_of(c.kind).into(),
                from: f.id.clone(),
                to: t.id.clone(),
            });
            edge_ids.push(c.id.clone());
        }
    }
    index.pod_edges.push(edge_ids);
    PodSpec {
        id: pid.to_string(),
        label: pod_label(d, pid, team_label),
        summary: None,
        continuity_policy: None,
        startup: None,
        members: agents.into_iter().map(|a| member(d, a)).collect(),
        edges,
    }
}

fn cross_edges(d: &TeamDraft, index: &mut SpecIndex) -> Vec<EdgeSpec> {
    let mut edges = Vec::new();
    for c in &d.connections {
        let (Some(f), Some(t)) = (d.agent(&c.from), d.agent(&c.to)) else {
            continue;
        };
        if d.pod_of(f) != d.pod_of(t) {
            edges.push(EdgeSpec {
                kind: crate::draft::names::edge_kind_of(c.kind).into(),
                from: format!("{}.{}", d.pod_of(f), f.id),
                to: format!("{}.{}", d.pod_of(t), t.id),
            });
            index.edges.push(c.id.clone());
        }
    }
    edges
}

pub(super) fn build(d: &TeamDraft) -> (RigSpec, SpecIndex) {
    let pod_ids = pod_ids(d);
    let team_label = match d.name.trim() {
        "" => "Team".to_string(),
        n => n.to_string(),
    };
    let mut index = SpecIndex {
        pods: pod_ids.clone(),
        members: Vec::new(),
        pod_edges: Vec::new(),
        edges: Vec::new(),
    };
    let pods = pod_ids
        .iter()
        .map(|pid| build_pod(d, pid, &team_label, &mut index))
        .collect();
    let edges = cross_edges(d, &mut index);
    let spec = RigSpec {
        version: SPEC_VERSION.into(),
        name: d.id.clone(),
        summary: None,
        culture_file: None,
        permission_policy: None,
        managed_blocks: None,
        workspace: None,
        docs: None,
        startup: None,
        services: None,
        pods,
        edges,
    };
    (spec, index)
}
