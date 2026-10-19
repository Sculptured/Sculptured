use super::{engine::agent_for, running};
use model::{TaskStatus, TeamDraft, WorkflowRun};
use crate::{error::{respond, ApiError}, extract::{Json, Path}, state::AppState};
use axum::{extract::State, response::Response, routing::{get, post}, Router};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/workflow", get(status))
        .route("/api/teams/{id}/workflow/retry", post(retry))
        .route("/api/teams/{id}/workflow/topic", post(topic))
        .route("/api/teams/{id}/workflow/decide", post(decide))
        .route("/api/teams/{id}/workflow/guard/resume", post(resume))
        .route("/api/teams/{id}/workflow/dispatch/retry", post(retry_dispatch))
}

async fn status(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let store = st.store.lock().unwrap();
    respond((|| -> Result<Value, ApiError> {
        let d = store.draft(&id)?.ok_or_else(|| ApiError::not_found("Project not found"))?;
        let start = running(&store, &id).ok().map(|(_, start)| start);
        let run = store.latest_workflow(&id)?;
        // Tokens are extra evidence: if they can't be read, the workflow still shows.
        let usage = run.as_ref().map(|r| store.assignment_usage(&assignments(r), r.start_seq).unwrap_or_else(|e| {
            tracing::warn!(error = %e, team = %id, "cannot read workflow token usage");
            Default::default()
        })).unwrap_or_default();
        // The dispatch command's result for the running start (else the shown batch's start).
        let at = start.or(run.as_ref().map(|r| r.start_seq));
        let dispatch = at.map(|s| store.dispatch_record(&id, s)).transpose()?.flatten();
        let dispatching = st.workflow_jobs.busy(&id, super::jobs::Kind::Dispatch);
        Ok(json!({ "config": d.workflow, "run": run, "active": run.as_ref().is_some_and(|r| Some(r.start_seq) == start), "usage": usage, "dispatch": dispatch, "dispatching": dispatching, "start_seq": start }))
    })())
}

/// Every assignment a run has sent: each step's (and its panel's), and the ones in progress.
pub(super) fn assignments(run: &WorkflowRun) -> Vec<String> {
    run.all_tasks().flat_map(|t| {
        let steps = t.history.iter().flat_map(|h| std::iter::once(&h.assignment).chain(h.panel.iter().filter_map(|r| r.assignment.as_ref())));
        steps.chain(&t.assignment).chain(t.reviews.iter().filter_map(|r| r.assignment.as_ref())).cloned().collect::<Vec<_>>()
    }).collect()
}

#[derive(Deserialize)]
struct Retry { task: String, start_seq: i64 }

async fn retry(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<Retry>) -> Response {
    respond(retry_task(&st, &id, &req.task, req.start_seq))
}

/// A 409 with the route's own code, in the user's words.
fn conflict(code: &'static str) -> impl Fn(String) -> ApiError { move |reason| ApiError::conflict(code, reason) }

/// The running team's current batch, refused when the page shows an earlier run (`expected`).
fn current_run(store: &crate::store::Store, rig: &str, expected: i64, bad: &impl Fn(String) -> ApiError) -> Result<(TeamDraft, WorkflowRun), ApiError> {
    let (d, start) = running(store, rig).map_err(bad)?;
    if start != expected { return Err(bad("This is an earlier run. Reload the workflow.".into())); }
    let run = store.workflow_run(rig, start)?.ok_or_else(|| bad("No batch yet.".into()))?;
    Ok((d, run))
}

pub(super) fn retry_task(st: &AppState, rig: &str, id: &str, expected: i64) -> Result<Value, ApiError> {
    let bad = conflict("workflow_retry");
    let mut store = st.store.lock().unwrap();
    let (d, mut run) = current_run(&store, rig, expected, &bad)?;
    // Split borrows: the checks below read run.config while changing the item.
    let config = &run.config;
    let t = run.tasks.iter_mut().chain(std::iter::once(&mut run.assembly)).find(|t| t.input.id == id).ok_or_else(|| bad("Task not found.".into()))?;
    if t.status != TaskStatus::Blocked { return Err(bad("Only a blocked item can be retried.".into())); }
    if t.history.len() >= config.max_steps as usize { return Err(bad("The step limit has been reached. Review the plan before starting a new batch.".into())); }
    // Check and human stages have no agent: the scheduler re-runs or re-parks them.
    if let Some(agent) = agent_for(config, t) {
        let seat = d.seat_id(agent).ok_or_else(|| bad("Agent not found.".into()))?;
        if store.seat(&seat)?.is_none_or(|s| s.lifecycle != model::SeatLifecycle::Running || s.current_turn.is_some()) { return Err(bad("The assigned agent must be running and idle before retrying.".into())); }
        if let Some(id) = &t.assignment {
            if !delivered(&store, id)? { return Err(bad("Delivery is still uncertain; inspect the transcript and stop/restart the project instead of risking duplicate work.".into())); }
        }
    }
    // A panel keeps the votes given; only reviewers who never voted are asked again, once their delivery is settled.
    for r in t.reviews.iter_mut().filter(|r| r.outcome.is_none()) {
        if let Some(a) = &r.assignment {
            if !delivered(&store, a)? { return Err(bad("A reviewer's delivery is still uncertain; inspect its transcript before retrying.".into())); }
        }
        r.assignment = None;
    }
    t.status = TaskStatus::Queued; t.assignment = None; t.assigned_agent = None; t.problem = None;
    store.save_workflow(&run, None)?;
    Ok(json!({ "retried": id }))
}

/// Whether a sent message reached its end (taken by the agent, or failed): retrying can't duplicate it.
fn delivered(store: &crate::store::Store, message: &str) -> rusqlite::Result<bool> {
    Ok(matches!(store.message_state(message)?, Some(model::MessageState::Consumed | model::MessageState::Failed)))
}

#[derive(Deserialize)]
struct Decide { task: String, outcome: String, #[serde(default)] note: String, start_seq: i64, #[serde(default)] log: bool }

async fn decide(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<Decide>) -> Response {
    respond(decide_task(&st, &id, &req.task, &req.outcome, &req.note, req.start_seq, req.log))
}

/// A person picks the outcome of an item waiting at a human stage; with `log`, the note becomes a precedent.
pub(super) fn decide_task(st: &AppState, rig: &str, task: &str, outcome: &str, note: &str, expected: i64, log: bool) -> Result<Value, ApiError> {
    let bad = conflict("workflow_decide");
    let mut store = st.store.lock().unwrap();
    let (d, mut run) = current_run(&store, rig, expected, &bad)?;
    let at = run.tasks.iter().find(|t| t.input.id == task).map(|t| (t.input.title.clone(), super::engine::stage(&run.config, &t.stage).map(|s| s.name.clone()).unwrap_or_default()));
    super::engine::decide(&mut run, task, outcome, note).map_err(bad)?;
    store.save_workflow(&run, None)?;
    if log && d.decision_log && !note.trim().is_empty() {
        let (title, stage) = at.unwrap_or_default();
        let text = format!("{title} · {stage} → {outcome}: {}", note.trim());
        store.add_decision(rig, "a person", model::cap::head(&text, 4096), "decision")?;
    }
    Ok(json!({ "decided": task }))
}

#[derive(Deserialize)]
struct Resume { start_seq: i64 }

async fn retry_dispatch(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<Resume>) -> Response {
    respond(super::dispatch::retry(&st, &id, req.start_seq))
}

async fn resume(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<Resume>) -> Response {
    respond(resume_guard(&st, &id, req.start_seq))
}

/// A person lets work go out past a failing guard, until its next result.
pub(super) fn resume_guard(st: &AppState, rig: &str, expected: i64) -> Result<Value, ApiError> {
    let bad = conflict("workflow_guard");
    let mut store = st.store.lock().unwrap();
    let (_, mut run) = current_run(&store, rig, expected, &bad)?;
    if !super::guard::paused(&run) { return Err(bad("The guard isn't holding anything up.".into())); }
    run.guard.as_mut().unwrap().resumed_by_person = true;
    store.save_workflow(&run, None)?;
    Ok(json!({ "resumed": true }))
}

#[derive(Deserialize)]
struct TopicEdit {
    task: String, start_seq: i64, mode: model::TopicMode, depends_on: Vec<String>,
    previous_mode: model::TopicMode, previous_depends_on: Vec<String>,
}

async fn topic(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<TopicEdit>) -> Response {
    respond(edit_topic(&st, &id, req))
}

fn edit_topic(st: &AppState, rig: &str, req: TopicEdit) -> Result<Value, ApiError> {
    let bad = conflict("workflow_topic");
    let mut store = st.store.lock().unwrap();
    let (_, mut run) = current_run(&store, rig, req.start_seq, &bad)?;
    let t = run.tasks.iter_mut().find(|t| t.input.id == req.task).ok_or_else(|| bad("Topic not found.".into()))?;
    if t.assignment.is_some() || !t.history.is_empty() || t.status != TaskStatus::Queued { return Err(bad("Only topics that have never been assigned can change prerequisites.".into())); }
    if t.input.mode != req.previous_mode || t.input.depends_on != req.previous_depends_on { return Err(bad("The topic changed elsewhere. Reload before editing.".into())); }
    t.input.mode = req.mode; t.input.depends_on = req.depends_on;
    super::topics::validate(&run.tasks.iter().map(|t| t.input.clone()).collect::<Vec<_>>()).map_err(&bad)?;
    store.save_workflow(&run, None)?;
    Ok(json!(run))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{store::Store, workflow::tests::{setup, dispatch, current}};
    use model::TopicMode;

    fn edit(start_seq: i64, task: &str, prerequisite: &str) -> TopicEdit {
        TopicEdit { task: task.into(), start_seq, mode: TopicMode::Dependent, depends_on: vec![prerequisite.into()], previous_mode: TopicMode::Independent, previous_depends_on: vec![] }
    }

    #[tokio::test]
    async fn topic_changes_validate_cycles_versions_and_assignment_races() {
        let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 2).unwrap();
        let start = current(&st).start_seq;
        assert!(edit_topic(&st, "book", edit(start - 1, "item-1", "item-0")).is_err());
        edit_topic(&st, "book", edit(start, "item-1", "item-0")).unwrap();
        assert!(edit_topic(&st, "book", edit(start, "item-1", "item-0")).is_err(), "stale edit rejected");
        assert!(edit_topic(&st, "book", edit(start, "item-0", "item-1")).is_err(), "cycle rejected");
        assert_eq!(current(&st).tasks[0].input.mode, TopicMode::Independent, "failed edit leaves state untouched");
        super::super::scheduler::process(&st, "book").await.unwrap();
        assert!(current(&st).tasks[0].assignment.is_some());
        let mut req = edit(start, "item-0", "item-1"); req.mode = TopicMode::Independent; req.depends_on.clear();
        assert!(edit_topic(&st, "book", req).is_err(), "cannot change assigned topic");
    }
}
