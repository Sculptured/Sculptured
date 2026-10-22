use super::{Desired, Store, Subject, TaskStatus, TeamDraft, engine};

pub(in super::super) fn running(store: &Store, rig: &str) -> Result<(TeamDraft, i64), String> {
    let d = store
        .draft(rig)
        .map_err(|e| e.to_string())?
        .ok_or("Project not found.")?;
    if d.workflow.is_none() {
        return Err("This project has no dynamic workflow.".into());
    }
    let snap = store
        .rig_snapshot(rig)
        .map_err(|e| e.to_string())?
        .ok_or("Start this project first.")?;
    if snap.desired != Desired::Running {
        return Err("This project is stopped; no more work will be dispatched.".into());
    }
    let start = store
        .latest_of_types(&Subject::Rig(rig.into()), &["rig-booted"], 1)
        .map_err(|e| e.to_string())?
        .first()
        .map(|e| e.seq)
        .ok_or("Project has no recorded start.")?;
    Ok((d, start))
}

/// Items waiting at human stages of running teams, for Needs you.
pub fn decisions(store: &Store) -> Vec<crate::views::InboxItem> {
    let mut out = Vec::new();
    let matching_store = store.list_rigs().unwrap_or_default();
    for rig in matching_store {
        let Ok((d, start)) = running(store, &rig.name) else {
            continue;
        };
        let Ok(Some(run)) = store.workflow_run(&rig.name, start) else {
            continue;
        };
        let matching_tasks = run.tasks.iter().filter(|t| t.status == TaskStatus::Waiting);
        for t in matching_tasks {
            let (Some(w), Some(s)) = (&t.waiting, engine::stage(&run.config, &t.stage)) else {
                continue;
            };
            let decision = crate::views::WaitingDecision {
                team: d.id.clone(),
                team_name: d.name.clone(),
                start_seq: start,
                task: t.input.id.clone(),
                title: t.input.title.clone(),
                stage: s.id.clone(),
                stage_name: s.name.clone(),
                prompt: s.prompt.clone().unwrap_or_default(),
                outcomes: s.routes.keys().cloned().collect(),
                since_ms: w.since_ms,
                recent: t.history.iter().rev().take(3).rev().cloned().collect(),
            };
            out.push(crate::views::InboxItem {
                id: format!("decision:{}:{}", rig.name, t.input.id),
                kind: crate::views::InboxKind::Decision,
                rig: rig.name.clone(),
                seat: None,
                opened_at_ms: w.since_ms,
                question: None,
                seat_view: None,
                decision: Some(decision),
            });
        }
    }
    out
}
