//! The dispatch command (`Workflow.dispatch`): with no dispatcher agent, Sculptured runs the command
//! once per start, on the team's computer in the project folder, and its standard output, in the
//! `dispatch_work` shape, becomes the batch through the same checks (`tools::batch`). The result is
//! kept per start (`DispatchRecord`): that Sculptured ran the command is observed, the items are the
//! command's own output. A command that fails, hangs or prints no valid batch dispatches nothing and
//! says why; a person runs it again from the workflow (`retry`) or by starting the team again.
use super::jobs::Kind;
use super::tools::Dispatch;
use crate::state::AppState;
use crate::store::Store;
use localops::command::CommandOutput;
use localops::LocalOp;
use model::{DispatchRecord, DispatchTask, TeamDraft, WorkflowRun};
use serde::Deserialize;
use std::time::Instant;

pub struct Job { pub id: String, pub command: String, pub cwd: String, pub timeout_s: u32 }

/// What the command prints: `dispatch_work`'s arguments, with the batch id optional.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Printed { #[serde(default)] batch_id: Option<String>, summary: String, tasks: Vec<DispatchTask> }

/// How much of a computer's refusal or stderr the record keeps.
const KEEP: usize = 4096;

/// Starts the command for this start when it is due (see `due`). Call only while the start has no batch.
pub fn start(st: &AppState, store: &Store, d: &TeamDraft, start: i64) {
    if let Some(job) = due(st, store, d, start) { spawn(st.clone(), d.id.clone(), start, job); }
}

/// The command's run for this start, claimed, when it is due: configured, no result yet, none running.
/// Call only while the start has no batch.
pub fn due(st: &AppState, store: &Store, d: &TeamDraft, start: i64) -> Option<Job> {
    let c = d.workflow.as_ref()?.dispatch.as_ref()?;
    if st.workflow_jobs.busy(&d.id, Kind::Dispatch) || store.dispatch_record(&d.id, start).ok()?.is_some() { return None; }
    let id = format!("dispatch-{start}");
    st.workflow_jobs.claim(&id, &d.id, start, Kind::Dispatch);
    let timeout_s = c.timeout_s.unwrap_or(model::DEFAULT_CHECK_TIMEOUT_S);
    Some(Job { id, command: c.command.clone(), cwd: d.folder.clone().unwrap_or_default(), timeout_s })
}

pub fn spawn(st: AppState, rig: String, start: i64, job: Job) {
    let jobs = st.workflow_jobs.clone();
    jobs.clone().spawn(&job.id.clone(), async move {
        let began = Instant::now();
        let op = LocalOp::RunDispatch { cwd: job.cwd.clone(), command: job.command.clone(), timeout_s: job.timeout_s };
        let ran = crate::ops::run(&st, crate::ops::Target::Team(&rig), op).await.map_err(unrunnable)
            .and_then(|v| serde_json::from_value::<CommandOutput>(v).map_err(|e| format!("The computer's answer could not be read: {e}")));
        let mut store = st.store.lock().unwrap();
        // Stopped (or started again) meanwhile: this start takes no batch.
        if !jobs.finish(&job.id) { return; }
        let Ok(Some(d)) = store.draft(&rig) else { return };
        let (record, run) = settle(&d, start, &job.command, ran, began);
        if let Err(e) = store.save_dispatch(&rig, start, &record, run.as_ref()) { tracing::error!(error = %e, %rig, "cannot record the dispatch command"); }
    });
}

/// Why the command could not run at all, in words a person can act on.
fn unrunnable(e: crate::error::ApiError) -> String {
    if e.code == "unknown_op" {
        return "This computer's Sculptured is too old to run a dispatch command. Update Sculptured on that computer and run `sculptured connect` again.".into();
    }
    format!("The dispatch command did not run: {} Nothing was dispatched.", model::cap::head(&e.message, KEEP))
}

/// The record of this run of the command, and the batch it made when its output is a valid one.
fn settle(d: &TeamDraft, start: i64, command: &str, ran: Result<CommandOutput, String>, began: Instant) -> (DispatchRecord, Option<WorkflowRun>) {
    let record = |dispatched: bool, evidence: String, problem: Option<String>| DispatchRecord { command: command.into(), at_ms: model::now_ms(), dispatched, evidence, problem };
    let out = match ran {
        Err(why) => return (record(false, format!("$ {command}\nnot run"), Some(why)), None),
        Ok(out) => out,
    };
    let evidence = evidence(command, &out, began);
    match batch_of(d, start, &out) {
        Ok(run) => (record(true, format!("{evidence}\n{} items dispatched", run.tasks.len()), None), Some(run)),
        Err(why) => (record(false, evidence, Some(why)), None),
    }
}

fn batch_of(d: &TeamDraft, start: i64, out: &CommandOutput) -> Result<WorkflowRun, String> {
    if out.timed_out { return Err("The dispatch command was stopped at its time limit before it finished; nothing was dispatched.".into()); }
    if out.too_large { return Err(format!("The dispatch command printed more than {} MB; nothing was dispatched.", localops::command::MAX_STDOUT >> 20)); }
    if out.exit_code != Some(0) {
        let code = out.exit_code.map_or("no exit status".to_string(), |c| format!("exit {c}"));
        return Err(format!("The dispatch command failed ({code}); nothing was dispatched."));
    }
    let p: Printed = serde_json::from_str(out.stdout.trim()).map_err(|e| format!(
        "The dispatch command's output is not a batch ({e}). It must print JSON: {{\"summary\": …, \"tasks\": [{{\"id\", \"title\", \"context\"}}]}}; nothing was dispatched."))?;
    let req = Dispatch { batch_id: p.batch_id.unwrap_or_else(|| format!("batch-{start}")), summary: p.summary, tasks: p.tasks };
    super::tools::batch(d, start, req).map_err(|e| format!("The dispatch command's batch was refused: {e} Nothing was dispatched."))
}

/// The command, its exit status and time, and the end of what it wrote to stderr.
fn evidence(command: &str, out: &CommandOutput, began: Instant) -> String {
    let exit = out.exit_code.map_or("none".to_string(), |c| c.to_string());
    let timed = if out.timed_out { " · timed out" } else { "" };
    let stderr = if out.stderr.is_empty() { String::new() } else { format!("\nstderr:\n{}", model::cap::tail(&out.stderr, KEEP)) };
    format!("$ {command}\nexit {exit} · {} ms{timed} · {} bytes of output{stderr}", began.elapsed().as_millis(), out.stdout.len())
}

/// A person asks for a failed dispatch command to run again (this start only).
pub(super) fn retry(st: &AppState, rig: &str, expected: i64) -> Result<serde_json::Value, crate::error::ApiError> {
    let bad = |m: &str| crate::error::ApiError::conflict("workflow_dispatch", m.to_string());
    let mut store = st.store.lock().unwrap();
    let (_, start) = super::running(&store, rig).map_err(|e| bad(&e))?;
    if start != expected { return Err(bad("This is an earlier run. Reload the workflow.")); }
    let failed = store.dispatch_record(rig, start)?.is_some_and(|r| !r.dispatched);
    if !failed || store.workflow_run(rig, start)?.is_some() { return Err(bad("Only a dispatch command that dispatched nothing can run again.")); }
    store.clear_dispatch(rig, start)?;
    Ok(serde_json::json!({ "retried": true }))
}

#[cfg(test)]
#[test]
fn a_computer_whose_sculptured_predates_dispatch_commands_is_told_to_update() {
    // An older runner cannot read the request (it would only know RunCheck, which keeps just the end of the output).
    let e: crate::error::ApiError = localops::LocalError::bad("unknown_op", "unknown variant `run-dispatch`").into();
    let why = unrunnable(e);
    assert!(why.contains("too old") && why.contains("Update Sculptured on that computer"), "{why}");
}
