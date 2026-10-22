//! Merge stages: Sculptured merges the item's branch into the project's current branch (on the team's
//! computer), one merge per team at a time. `merged` removes the item's copy (its branch is kept);
//! `conflict` changes nothing and keeps the copy; anything else blocks the item with the reason.
use super::jobs;
use crate::state::AppState;
use localops::LocalOp;
use model::StepBy;

/// `task` is the item's id in the run; `key` names its copy and branch (unique per batch).
pub struct Job {
    pub assignment: String,
    pub project: String,
    pub task: String,
    pub key: String,
    pub into: Option<String>,
}

pub fn spawn(st: AppState, rig: String, start: i64, job: Job) {
    let id = job.assignment.clone();
    let jobs = st.workflow_jobs.clone();
    jobs.clone().spawn(&id, async move {
        let op = LocalOp::ItemMerge {
            project: job.project.clone(),
            team: rig.clone(),
            task: job.key.clone(),
            into: job.into.clone(),
        };
        let done = crate::ops::run(&st, crate::ops::Target::Team(&rig), op).await;
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        if !jobs.finish(&job.assignment) {
            return;
        }
        let (outcome, result, evidence) = match &done {
            Ok(v) if v["merged"] == true => (
                "merged",
                "Merged into the project's branch.",
                v["summary"].as_str().map(str::to_string),
            ),
            Ok(v) => (
                "conflict",
                "The item's branch did not merge cleanly; nothing was merged.",
                v["summary"].as_str().map(str::to_string),
            ),
            Err(e) => ("blocked", e.message.as_str(), None),
        };
        jobs::record(
            &mut store,
            &rig,
            start,
            &job.assignment,
            outcome,
            result,
            StepBy::Sculptured,
            evidence,
        );
        if outcome == "merged"
            && let Ok(Some(mut run)) = store.workflow_run(&rig, start)
        {
            if let Some(t) = run.tasks.iter_mut().find(|t| t.input.id == job.task) {
                t.copy = None;
            }
            let _ = store.save_workflow(&run, None);
        }
    });
}
