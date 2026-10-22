//! copies: per-item — makes an item's own copy of the project (on the team's computer) before its first
//! assignment, a few at a time, in the background so a slow repository or computer holds up no one else.
//! A copy that can't be made blocks the item with the reason.
use crate::state::AppState;
use localops::LocalOp;

/// `task` is the item's id in the run; `key` names its copy and branch (unique per batch).
pub struct Job {
    pub id: String,
    pub project: String,
    pub task: String,
    pub key: String,
}

pub fn spawn(st: AppState, rig: String, start: i64, job: Job) {
    let jobs = st.workflow_jobs.clone();
    jobs.clone().spawn(&job.id.clone(), async move {
        let op = LocalOp::ItemCopyPrepare {
            project: job.project.clone(),
            team: rig.clone(),
            task: job.key.clone(),
        };
        let made = crate::ops::run(&st, crate::ops::Target::Team(&rig), op).await;
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        if !jobs.finish(&job.id) {
            return;
        }
        let Ok(Some(mut run)) = store.workflow_run(&rig, start) else {
            return;
        };
        let changed = match made {
            Ok(path) => run
                .tasks
                .iter_mut()
                .find(|t| t.input.id == job.task)
                .map(|t| t.copy = path.as_str().map(str::to_string))
                .is_some(),
            Err(e) => super::engine::block(&mut run, &job.task, &e.message).is_ok(),
        };
        if changed && let Err(e) = store.save_workflow(&run, None) {
            tracing::error!(error = %e, %rig, "cannot record an item's copy");
        }
    });
}
