//! Check stages: Sculptured runs the stage's command (in the item's copy, else the project folder; on the
//! team's computer) and routes the item on its exit status. The step is labelled verified (StepBy::Check)
//! and keeps the command, exit status and the end of the output. A check that could not run blocks the
//! item with a step by Sculptured instead: that is not a verified failure.
use super::jobs;
use crate::state::AppState;
use localops::checks::CheckOutcome;
use localops::{ItemRef, LocalOp};
use model::StepBy;
use std::time::Instant;

/// `cwd` is the project folder; with `item`, the check runs in that item's copy instead.
pub struct Job {
    pub assignment: String,
    pub command: String,
    pub cwd: String,
    pub item: Option<ItemRef>,
    pub timeout_s: u32,
}

/// How much of the end of the output a step keeps.
const KEEP: usize = 4096;

pub fn spawn(st: AppState, rig: String, start: i64, job: Job) {
    let id = job.assignment.clone();
    let jobs = st.workflow_jobs.clone();
    jobs.clone().spawn(&id, async move {
        let began = Instant::now();
        let op = LocalOp::RunCheck {
            cwd: job.cwd.clone(),
            item: job.item.clone(),
            command: job.command.clone(),
            timeout_s: job.timeout_s,
        };
        let ran = crate::ops::run(&st, crate::ops::Target::Team(&rig), op)
            .await
            .and_then(|v| {
                serde_json::from_value::<CheckOutcome>(v).map_err(|e| {
                    crate::error::ApiError::internal(format!("unreadable check result: {e}"))
                })
            });
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        if !jobs.finish(&job.assignment) {
            return;
        }
        match ran {
            Err(e) => jobs::record(
                &mut store,
                &rig,
                start,
                &job.assignment,
                "blocked",
                &e.message,
                StepBy::Sculptured,
                None,
            ),
            Ok(out) => {
                let outcome = if out.passed { "passed" } else { "failed" };
                let evidence = evidence(&job.command, &out, began);
                jobs::record(
                    &mut store,
                    &rig,
                    start,
                    &job.assignment,
                    outcome,
                    &format!("Check {outcome}: {}", job.command),
                    StepBy::Check,
                    Some(evidence),
                );
            }
        }
    });
}

/// A check's evidence: the command, its exit status and time, and the end of its output.
pub(super) fn evidence(command: &str, out: &CheckOutcome, began: Instant) -> String {
    let exit = out.exit_code.map_or("none".to_string(), |c| c.to_string());
    format!(
        "$ {command}\nexit {exit} · {} ms{}\n{}",
        began.elapsed().as_millis(),
        if out.timed_out { " · timed out" } else { "" },
        model::cap::tail(&out.output, KEEP)
    )
}
