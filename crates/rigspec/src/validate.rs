use crate::{Diagnostic, EdgeKind, EdgeSpec, MemberSpec, PodSpec, RestorePolicy, RigSpec, Runtime};
use std::collections::HashSet;

const BUILTIN_POLICIES: [&str; 5] = ["locked", "standard", "open", "yolo", "auto"];

fn safe_relative(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && p.split('/').all(|s| !s.is_empty() && s != "..")
}

fn check_policy(path: &str, p: &str, out: &mut Vec<Diagnostic>) {
    if let Some(name) = p.strip_prefix("builtin:") {
        if !BUILTIN_POLICIES.contains(&name) {
            out.push(crate::diag::error(
                path,
                format!("unknown built-in policy {p:?}"),
            ));
        }
    } else if BUILTIN_POLICIES.contains(&p) {
        out.push(crate::diag::error(path, format!("use 'builtin:{p}'")));
    } else if p != "none" && !safe_relative(p) {
        out.push(crate::diag::error(
            path,
            "policy path must be a safe relative path",
        ));
    }
}

/// A `not_enforced` diagnostic for each declared field, at `base.field` (or `field` at the top).
fn declared(base: &str, fields: &[(&str, bool)], out: &mut Vec<Diagnostic>) {
    out.extend(crate::diag::present_paths(base, fields).map(crate::diag::not_enforced));
}

#[must_use]
pub fn validate(spec: &RigSpec) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    check_rig(spec, &mut out);
    let mut pod_ids = HashSet::new();
    let mut seats = HashSet::new();
    let indexed_pods = spec.pods.iter().enumerate();
    for (pi, pod) in indexed_pods {
        check_pod(
            &format!("pods[{pi}]"),
            pod,
            &mut pod_ids,
            &mut seats,
            &mut out,
        );
    }
    let indexed_edges = spec.edges.iter().enumerate();
    for (ei, e) in indexed_edges {
        check_cross_edge(&format!("edges[{ei}]"), e, &seats, &mut out);
    }
    out
}

fn check_rig(spec: &RigSpec, out: &mut Vec<Diagnostic>) {
    if spec.version.trim().is_empty() {
        out.push(crate::diag::error(
            "version",
            "version must be a non-empty string",
        ));
    }
    if !model::valid_segment(&spec.name) {
        out.push(crate::diag::error(
            "name",
            "rig name must use letters, digits, '_' or '-'",
        ));
    }
    if spec.pods.is_empty() {
        out.push(crate::diag::error("pods", "at least one pod is required"));
    }
    if let Some(c) = &spec.culture_file {
        if !safe_relative(c) {
            out.push(crate::diag::error(
                "culture_file",
                "culture_file must be a safe relative path",
            ));
        }
        out.push(crate::diag::not_enforced("culture_file"));
    }
    if let Some(p) = &spec.permission_policy {
        check_policy("permission_policy", p, out);
    }
    let fields = [
        ("managed_blocks", spec.managed_blocks.is_some()),
        ("workspace", spec.workspace.is_some()),
        ("docs", spec.docs.is_some()),
        ("startup", spec.startup.is_some()),
        ("services", spec.services.is_some()),
    ];
    declared("", &fields, out);
}

fn check_pod(
    pp: &str,
    pod: &PodSpec,
    pod_ids: &mut HashSet<String>,
    seats: &mut HashSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    if !model::valid_segment(&pod.id) {
        out.push(crate::diag::error(
            format!("{pp}.id"),
            "pod id must use letters, digits, '_' or '-'",
        ));
    }
    if !pod_ids.insert(pod.id.clone()) {
        out.push(crate::diag::error(
            format!("{pp}.id"),
            format!("duplicate pod id {:?}", pod.id),
        ));
    }
    if pod.label.trim().is_empty() {
        out.push(crate::diag::error(
            format!("{pp}.label"),
            "pod label is required",
        ));
    }
    declared(
        pp,
        &[
            ("continuity_policy", pod.continuity_policy.is_some()),
            ("startup", pod.startup.is_some()),
        ],
        out,
    );
    let mut members = HashSet::new();
    let indexed_members = pod.members.iter().enumerate();
    for (mi, m) in indexed_members {
        check_member(&format!("{pp}.members[{mi}]"), pod, m, &mut members, out);
        seats.insert(format!("{}.{}", pod.id, m.id));
    }
    let indexed_edges = pod.edges.iter().enumerate();
    for (ei, e) in indexed_edges {
        check_pod_edge(&format!("{pp}.edges[{ei}]"), pod, e, &members, out);
    }
}

fn check_member(
    mp: &str,
    pod: &PodSpec,
    m: &MemberSpec,
    members: &mut HashSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    if !model::valid_segment(&m.id) {
        out.push(crate::diag::error(
            format!("{mp}.id"),
            "member id must use letters, digits, '_' or '-'",
        ));
    }
    if !members.insert(m.id.clone()) {
        out.push(crate::diag::error(
            format!("{mp}.id"),
            format!("duplicate member id {:?} in pod {:?}", m.id, pod.id),
        ));
    }
    check_member_fields(mp, m, out);
    if let Some(p) = &m.permission_policy {
        check_policy(&format!("{mp}.permission_policy"), p, out);
    }
    let fields = [
        ("effort", m.effort.is_some()),
        ("compaction_strategy", m.compaction_strategy.is_some()),
        ("mechanic", m.mechanic.is_some()),
        ("codex_config_profile", m.codex_config_profile.is_some()),
        ("session_source", m.session_source.is_some()),
        ("starter_ref", m.starter_ref.is_some()),
        ("startup", m.startup.is_some()),
    ];
    declared(mp, &fields, out);
}

fn check_member_fields(mp: &str, m: &MemberSpec, out: &mut Vec<Diagnostic>) {
    if Runtime::parse(&m.runtime).is_none() {
        out.push(crate::diag::error(
            format!("{mp}.runtime"),
            format!(
                "unknown runtime {:?}; expected claude-code, codex, pi, omp, terminal or stub",
                m.runtime
            ),
        ));
    }
    if !["local:", "path:", "builtin:"]
        .iter()
        .any(|p| m.agent_ref.starts_with(p))
    {
        out.push(crate::diag::error(
            format!("{mp}.agent_ref"),
            "agent_ref must start with local:, path: or builtin:",
        ));
    }
    if m.profile.trim().is_empty() {
        out.push(crate::diag::error(
            format!("{mp}.profile"),
            "profile is required (use \"default\")",
        ));
    }
    if m.cwd.trim().is_empty() {
        out.push(crate::diag::error(
            format!("{mp}.cwd"),
            "cwd is required (use \".\")",
        ));
    }
    if let Some(r) = &m.restore_policy
        && RestorePolicy::parse(r).is_none()
    {
        out.push(crate::diag::error(
            format!("{mp}.restore_policy"),
            "restore_policy must be resume_if_possible, relaunch_fresh or checkpoint_only",
        ));
    }
}

fn check_edge_kind(ep: &str, e: &EdgeSpec, out: &mut Vec<Diagnostic>) {
    if EdgeKind::parse(&e.kind).is_none() {
        out.push(crate::diag::error(
            format!("{ep}.kind"),
            format!("unknown edge kind {:?}", e.kind),
        ));
    }
}

fn check_pod_edge(
    ep: &str,
    pod: &PodSpec,
    e: &EdgeSpec,
    members: &HashSet<String>,
    out: &mut Vec<Diagnostic>,
) {
    check_edge_kind(ep, e, out);
    for (end, v) in [("from", &e.from), ("to", &e.to)] {
        if v.contains('.') {
            out.push(crate::diag::error(
                format!("{ep}.{end}"),
                "pod-local edges use bare member ids (no pod prefix)",
            ));
        } else if !members.contains(v) {
            out.push(crate::diag::error(
                format!("{ep}.{end}"),
                format!("no member {v:?} in pod {:?}", pod.id),
            ));
        }
    }
}

fn check_cross_edge(ep: &str, e: &EdgeSpec, seats: &HashSet<String>, out: &mut Vec<Diagnostic>) {
    check_edge_kind(ep, e, out);
    for (end, v) in [("from", &e.from), ("to", &e.to)] {
        if !v.contains('.') {
            out.push(crate::diag::error(
                format!("{ep}.{end}"),
                "cross-pod edges use pod.member ids",
            ));
        } else if !seats.contains(v) {
            out.push(crate::diag::error(
                format!("{ep}.{end}"),
                format!("no seat {v:?} in this rig"),
            ));
        }
    }
}
