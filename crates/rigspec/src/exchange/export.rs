//! Draft → RigSpec YAML plus the layout comment.
use super::META_PREFIX;
use super::meta::{AgentMeta, GroupMeta, Meta};
use model::{AgentKind, DraftAgent, TeamDraft};

/// True when the agent has a folder of its own, or needs none (Practice agents run in
/// the home folder, which `draft_spec` writes as `.`).
fn has_folder(d: &TeamDraft, a: &DraftAgent) -> bool {
    a.kind == AgentKind::Practice || d.effective_folder(a).is_some()
}

fn meta_of(d: &TeamDraft) -> Meta {
    Meta {
        workflow: d.workflow.clone(),
        folder: d.folder.clone(),
        name: d.name.clone(),
        agents: d
            .agents
            .iter()
            .map(|a| {
                (
                    format!("{}.{}", d.pod_of(a), a.id),
                    AgentMeta {
                        x: a.x,
                        y: a.y,
                        job: a.job.clone(),
                        inherits_folder: a.folder.as_deref().is_none_or(|f| f.trim().is_empty()),
                    },
                )
            })
            .collect(),
        groups: d
            .groups
            .iter()
            .map(|g| GroupMeta {
                id: g.id.clone(),
                label: g.label.clone(),
                x: g.x,
                y: g.y,
                w: g.w,
                h: g.h,
            })
            .collect(),
    }
}

#[must_use]
pub fn export_yaml(d: &TeamDraft) -> String {
    let mut spec = crate::draft::draft_spec(d);
    for pod in &mut spec.pods {
        for m in &mut pod.members {
            // The person's own model connection never travels in a team file.
            m.connection = None;
            // `draft_spec` writes `.` for a missing folder; for Claude or Codex that would
            // be a made-up working folder, so the export leaves it blank.
            if d.agents
                .iter()
                .any(|a| a.id == m.id && d.pod_of(a) == pod.id && !has_folder(d, a))
            {
                m.cwd = String::new();
            }
        }
    }
    let json = crate::exchange::meta::ascii_json(
        &serde_json::to_string(&meta_of(d)).expect("layout metadata serializes"),
    );
    format!("{}{META_PREFIX}{json}\n", crate::draft::spec_yaml(&spec))
}
