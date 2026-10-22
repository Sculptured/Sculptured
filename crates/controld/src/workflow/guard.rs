//! The guard: a project check (`Workflow.guard`) Sculptured runs every N minutes while the team runs, in
//! the project folder. While its last result failed, no new work goes out (assignments, checks, merges);
//! work already under way finishes. A passing result, or a person's "Resume anyway", lifts the pause.
use super::jobs::Kind;
use crate::state::AppState;
use localops::LocalOp;
use localops::checks::CheckOutcome;
use model::{GuardState, WorkflowRun};
use std::time::Instant;

pub struct Job {
    pub id: String,
    pub command: String,
    pub cwd: String,
    pub timeout_s: u32,
}

/// Whether the guard holds up new work.
pub fn paused(run: &WorkflowRun) -> bool {
    run.guard
        .as_ref()
        .is_some_and(|g| !g.passed && !g.resumed_by_person)
}

/// The guard's next run, if it is due now and none is running.
pub fn due(st: &AppState, run: &WorkflowRun, project: &str) -> Option<Job> {
    let g = run.config.guard.as_ref()?;
    // Once every item and the assembly are done there is nothing left to protect.
    if run.assembly.status == model::TaskStatus::Complete {
        return None;
    }
    let every_ms = i64::from(g.every_minutes) * 60_000;
    if st.workflow_jobs.busy(&run.rig, Kind::Guard)
        || run
            .guard
            .as_ref()
            .is_some_and(|s| model::now_ms() - s.at_ms < every_ms)
    {
        return None;
    }
    let id = format!("guard-{}-{}", run.start_seq, model::now_ms());
    st.workflow_jobs
        .claim(&id, &run.rig, run.start_seq, Kind::Guard);
    Some(Job {
        id,
        command: g.command.clone(),
        cwd: project.to_string(),
        timeout_s: g.timeout_s.unwrap_or(model::DEFAULT_CHECK_TIMEOUT_S),
    })
}

pub fn spawn(st: AppState, rig: String, start: i64, job: Job) {
    let jobs = st.workflow_jobs.clone();
    jobs.clone().spawn(&job.id.clone(), async move {
        let began = Instant::now();
        let op = LocalOp::RunCheck {
            cwd: job.cwd.clone(),
            item: None,
            command: job.command.clone(),
            timeout_s: job.timeout_s,
        };
        let ran = crate::ops::run(&st, crate::ops::Target::Team(&rig), op).await;
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        if !jobs.finish(&job.id) {
            return;
        }
        let (passed, evidence) = match ran.map(serde_json::from_value::<CheckOutcome>) {
            Ok(Ok(out)) => (
                out.passed,
                super::stage_checks::evidence(&job.command, &out, began),
            ),
            Ok(Err(e)) => (false, format!("Unreadable guard result: {e}")),
            Err(e) => (false, format!("Could not run the guard: {}", e.message)),
        };
        let Ok(Some(mut run)) = store.workflow_run(&rig, start) else {
            return;
        };
        run.guard = Some(GuardState {
            passed,
            at_ms: model::now_ms(),
            evidence,
            resumed_by_person: false,
        });
        if let Err(e) = store.save_workflow(&run, None) {
            tracing::error!(error = %e, %rig, "cannot record the guard");
        }
    });
}
