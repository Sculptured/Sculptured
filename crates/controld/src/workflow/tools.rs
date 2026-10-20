use super::{engine, running};
use crate::state::AppState;
use model::DispatchTask;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Dispatch { pub batch_id: String, pub summary: String, pub tasks: Vec<DispatchTask> }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete { assignment: String, outcome: String, result: String }

pub fn call(st: &AppState, seat: &str, tool: &str, args: &Value) -> Result<Value, String> {
    let rig = crate::seat_ids::rig_of(seat).ok_or("Unknown agent.")?;
    let mut store = st.store.lock().unwrap();
    let (d, start) = running(&store, rig)?;
    let agent = d.agent_for_seat(seat).ok_or("Agent is not in this project.")?;
    match tool {
        "dispatch_work" => {
            let req: Dispatch = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            let w = d.workflow.as_ref().unwrap();
            if w.dispatch.is_some() { return Err("This workflow's batch comes from its dispatch command, which Sculptured runs; no agent dispatches it.".into()); }
            if w.dispatcher != agent.id { return Err("Only the configured dispatcher can create a batch.".into()); }
            dispatch(&mut store, &d, start, req)?;
        }
        "complete_work" => {
            let req: Complete = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            let mut run = store.workflow_run(rig, start).map_err(|e| e.to_string())?.ok_or("No batch has been dispatched.")?;
            let task = run.all_tasks().find(|t| t.assignment.as_deref() == Some(req.assignment.as_str())).map(|t| (t.input.id.clone(), t.input.title.clone()));
            engine::complete(&mut run, &agent.id, &req.assignment, &req.outcome, &req.result)?;
            store.save_workflow(&run, None).map_err(|e| e.to_string())?;
            // The reply is read into the worker's context: one line, not the whole batch.
            let (id, title) = task.unwrap_or_else(|| (req.assignment.clone(), String::new()));
            let now = run.all_tasks().find(|t| t.input.id == id).map(|t| format!("; it is now {} at stage {}", serde_json::to_value(t.status).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(), t.stage)).unwrap_or_default();
            return Ok(json!({ "text": format!("Recorded outcome {:?} for task {id} ({title}){now}. Your next assignment, if any, arrives as a message; workflow_status reads the whole batch.", req.outcome) }));
        }
        "workflow_status" => {},
        _ => return Err("Unknown workflow tool.".into()),
    }
    let run = store.workflow_run(rig, start).map_err(|e| e.to_string())?;
    let summary = run.map(|r| json!({"batch_id":r.batch_id,"summary":r.summary,"tasks":r.all_tasks().map(|t| json!({"id":t.input.id,"title":t.input.title,"mode":t.input.mode,"depends_on":t.input.depends_on,"waiting_for":super::topics::waiting(&r,t),"stage":t.stage,"status":t.status,"assigned_agent":t.assigned_agent,"assignment":t.assignment,"steps":t.history.len(),"problem":t.problem,"latest_report":t.history.last()})).collect::<Vec<_>>() }));
    Ok(json!({ "text": serde_json::to_string(&summary).map_err(|e| e.to_string())? }))
}

/// A batch checked and made into its run: the one way in, for dispatch_work and the dispatch command alike.
pub(super) fn batch(d: &model::TeamDraft, start: i64, req: Dispatch) -> Result<model::WorkflowRun, String> {
    if !model::valid_segment(&req.batch_id) || req.summary.trim().is_empty() || req.summary.len() > 12000 { return Err("Provide a simple batch_id and a summary of 1–12000 bytes.".into()); }
    engine::new_run(&d.id, start, req.batch_id, req.summary, d.workflow.clone().unwrap(), req.tasks)
}

fn dispatch(store: &mut crate::store::Store, d: &model::TeamDraft, start: i64, req: Dispatch) -> Result<(), String> {
    let run = batch(d, start, req)?;
    if let Some(old) = store.workflow_run(&d.id, start).map_err(|e| e.to_string())? {
        let same = old.batch_id == run.batch_id && old.summary == run.summary && old.tasks.iter().map(|t| &t.input).eq(run.tasks.iter().map(|t| &t.input));
        return if same { Ok(()) } else { Err("This start already has a batch. Stop and start the project to dispatch a new batch.".into()) };
    }
    store.save_workflow(&run, None).map_err(|e| e.to_string())?;
    Ok(())
}
