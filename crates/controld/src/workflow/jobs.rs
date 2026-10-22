//! Work Sculptured does itself for a workflow (check stages, merges): one background job per assignment.
//! A job is claimed before it is spawned, so the next scheduler tick doesn't start it again; one lost to
//! a restart is not in this process's map, so the scheduler runs it again. Stopping a team (or starting
//! it again) stops its jobs and blocks their items with a step by Sculptured. Results are recorded under
//! the store lock, the same lock `stop_stale` holds, so a result and a stop never both land.
use super::engine;
use crate::store::Store;
use model::StepBy;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Check,
    Merge,
    Copy,
    Guard,
    Dispatch,
}

struct Running {
    rig: String,
    start: i64,
    kind: Kind,
    abort: Option<tokio::task::AbortHandle>,
}

/// This Sculptured's running workflow jobs, by assignment (`AppState.workflow_jobs`).
#[derive(Default)]
pub struct Jobs(Mutex<HashMap<String, Running>>);

impl Jobs {
    /// Whether this process is running the job for `assignment`.
    pub fn running(&self, assignment: &str) -> bool {
        self.0
            .lock()
            .expect("shared state mutex must not be poisoned")
            .contains_key(assignment)
    }

    /// Whether `rig` has a job of `kind` running.
    pub fn busy(&self, rig: &str, kind: Kind) -> bool {
        self.0
            .lock()
            .expect("shared state mutex must not be poisoned")
            .values()
            .any(|r| r.rig == rig && r.kind == kind)
    }

    /// How many jobs of `kind` `rig` has running.
    pub fn count(&self, rig: &str, kind: Kind) -> usize {
        self.0
            .lock()
            .expect("shared state mutex must not be poisoned")
            .values()
            .filter(|r| r.rig == rig && r.kind == kind)
            .count()
    }

    pub fn claim(&self, assignment: &str, rig: &str, start: i64, kind: Kind) {
        self.0
            .lock()
            .expect("shared state mutex must not be poisoned")
            .insert(
                assignment.to_string(),
                Running {
                    rig: rig.into(),
                    start,
                    kind,
                    abort: None,
                },
            );
    }

    /// Spawns the job's future and keeps its handle, so a stop can cancel it.
    pub fn spawn(
        &self,
        assignment: &str,
        work: impl std::future::Future<Output = ()> + Send + 'static,
    ) {
        let task = tokio::spawn(work);
        if let Some(r) = self
            .0
            .lock()
            .expect("shared state mutex must not be poisoned")
            .get_mut(assignment)
        {
            r.abort = Some(task.abort_handle());
        }
    }

    /// Ends the job: false when it was stopped meanwhile (its item is already recorded). Call with the store locked.
    pub fn finish(&self, assignment: &str) -> bool {
        self.0
            .lock()
            .expect("shared state mutex must not be poisoned")
            .remove(assignment)
            .is_some()
    }

    /// Stops `rig`'s jobs that belong to no running batch, and blocks their items. Call with the store locked.
    pub fn stop_stale(&self, store: &mut Store, rig: &str, current: Option<i64>) {
        let stale: Vec<(String, i64)> = {
            let mut j = self
                .0
                .lock()
                .expect("shared state mutex must not be poisoned");
            let ids: Vec<String> = j
                .iter()
                .filter(|(_, r)| r.rig == rig && Some(r.start) != current)
                .map(|(id, _)| id.clone())
                .collect();
            // Only checks and merges hold an item's assignment; a stopped copy, guard or dispatch has nothing to record.
            ids.into_iter()
                .filter_map(|id| {
                    j.remove(&id).and_then(|r| {
                        if let Some(a) = r.abort {
                            a.abort();
                        }
                        matches!(r.kind, Kind::Check | Kind::Merge).then_some((id, r.start))
                    })
                })
                .collect()
        };
        for (id, start) in stale {
            record(
                store,
                rig,
                start,
                &id,
                "blocked",
                "This was stopped with the team before it finished.",
                StepBy::Sculptured,
                None,
            );
        }
    }
}

/// Records a job's outcome on its item and saves the run.
#[allow(clippy::too_many_arguments)]
pub fn record(
    store: &mut Store,
    rig: &str,
    start: i64,
    assignment: &str,
    outcome: &str,
    result: &str,
    by: StepBy,
    evidence: Option<String>,
) {
    let Ok(Some(mut run)) = store.workflow_run(rig, start) else {
        return;
    };
    match engine::settle(&mut run, assignment, outcome, result, by, evidence) {
        Ok(()) => {
            if let Err(e) = store.save_workflow(&run, None) {
                tracing::error!(error = %e, %rig, "cannot record a workflow job");
            }
        }
        Err(e) => tracing::warn!(%e, %rig, "a workflow job finished for a stale assignment"),
    }
}
