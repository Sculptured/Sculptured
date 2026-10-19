use super::*;
use crate::{state::AppState, store::{RigSource, Store}};
use model::*;
use serde_json::json;

// Keep scheduler mechanics independent of additions to the production template.
pub(super) fn review_fixture()->TeamDraft {
 let mut d=rigspec::textbook::template("book","Book",true);
 d.agents.retain(|a|a.group_id.as_deref()!=Some("webgpu-engine"));
 d.groups.retain(|g|g.id!="webgpu-engine");
 let w=d.workflow.as_mut().unwrap();w.entry="reference".into();w.max_steps=12;
 w.stages.retain(|s|!s.id.starts_with("engine"));
 for s in &mut w.stages{s.routes.retain(|_,to|to.as_deref()!=Some("engine"));}
 d
}

pub(super) fn setup(store: Store) -> AppState {
    setup_draft(store, review_fixture())
}

pub(super) fn setup_draft(store: Store, mut d: TeamDraft) -> AppState {
    d.groups.clear(); for a in &mut d.agents { a.group_id = None; }
    let mut store = store;
    let rig = rigspec::from_draft(&d).unwrap().rig;
    store.insert_draft(&d).unwrap();
    store.upsert_rig(&rig, &RigSource::Team { id: d.id.clone(), revision: 0 }, Desired::Running).unwrap();
    let st = AppState::new(store, "test".into(), vec![]);
    st.local_event(Actor::Human("local".into()), Subject::Rig("book".into()), Provenance::Operator, EventBody::RigBooted { spec_hash: rig.spec_hash, seat_count: 6 }).unwrap();
    for a in &d.agents { event(&st, &a.id, EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Running, reason: None }); }
    st
}

#[test]
fn topic_dependencies_reject_unknown_self_duplicate_cycles_and_inconsistent_modes() {
    let topic = |id: &str, mode: TopicMode, deps: Vec<&str>| DispatchTask { id:id.into(), title:id.into(), context:"source".into(), mode, depends_on:deps.into_iter().map(str::to_string).collect() };
    let a = topic("a", TopicMode::Independent, vec![]);
    for deps in [vec!["missing"], vec!["b"], vec!["a","a"], vec![]] {
        assert!(topics::validate(&[a.clone(),topic("b",TopicMode::Dependent,deps)]).is_err());
    }
    assert!(topics::validate(&[a.clone(),topic("b",TopicMode::Independent,vec!["a"])]).is_err());
    assert!(topics::validate(&[topic("a",TopicMode::Dependent,vec!["b"]),topic("b",TopicMode::Dependent,vec!["a"])]).is_err());
    assert!(topics::validate(&[a,topic("b",TopicMode::Dependent,vec!["a"])]).is_ok());
}

#[tokio::test]
async fn independent_topics_use_separate_workers_and_dependents_wait_for_acceptance() {
    let mut d = review_fixture();
    let mut extra = d.agents.iter().find(|a| a.id == "reference").unwrap().clone(); extra.id = "reference-two".into(); extra.name = "Reference two".into();
    d.agents.push(extra); d.workflow.as_mut().unwrap().stages[0].workers.push("reference-two".into());
    let st = setup_draft(Store::open_in_memory().unwrap(),d);
    call(&st,"team.dispatcher@book","dispatch_work",&json!({"batch_id":"parallel","summary":"source","tasks":[
        {"id":"a","title":"A","context":"a","mode":"independent","depends_on":[]},
        {"id":"b","title":"B","context":"b","mode":"independent","depends_on":[]},
        {"id":"c","title":"C","context":"c","mode":"dependent","depends_on":["a"]}
    ]})).unwrap();
    scheduler::process(&st,"book").await.unwrap();
    let first = current(&st);
    assert_eq!(first.tasks[0].assigned_agent.as_deref(),Some("reference"));
    assert_eq!(first.tasks[1].assigned_agent.as_deref(),Some("reference-two"));
    assert!(first.tasks[2].assignment.is_none());
    let wrong = call(&st,"team.reference@book","complete_work",&json!({"assignment":first.tasks[1].assignment,"outcome":"ready","result":"wrong worker"}));
    assert!(wrong.is_err());
    finish(&st,0,"ready").await; scheduler::process(&st,"book").await.unwrap();
    assert!(current(&st).tasks[2].assignment.is_none(),"a stage result is not topic acceptance");
    for outcome in ["ready","pass","accept"] { finish(&st,0,outcome).await; }
    scheduler::process(&st,"book").await.unwrap();
    assert!(current(&st).tasks[2].assignment.is_some());
    assert!(current(&st).tasks[1].history.is_empty(),"B need not finish before C starts");
    assert!(current(&st).assembly.assignment.is_none());
}

fn event(st: &AppState, agent: &str, body: EventBody) {
    st.local_event(Actor::System("fixture".into()), Subject::Seat(format!("team.{agent}@book")), Provenance::ProtocolObserved, body).unwrap();
}

pub(super) fn dispatch(st: &AppState, n: usize) -> Result<serde_json::Value, String> {
    call(st, "team.dispatcher@book", "dispatch_work", &json!({"batch_id":"test", "summary":"Source and goals", "tasks":(0..n).map(|i| json!({"id":format!("item-{i}"),"title":format!("Topic {i}"),"context":"Source section 1; work only in modules/item/"})).collect::<Vec<_>>()}))
}

pub(super) fn current(st: &AppState) -> WorkflowRun { st.store.lock().unwrap().latest_workflow("book").unwrap().unwrap() }

fn task_at(r: &WorkflowRun, index: usize) -> &WorkflowTask { r.tasks.get(index).unwrap_or(&r.assembly) }

/// Finishes whichever item is assigned to an agent next (background work decides the order).
async fn finish_next(st: &AppState, outcome: &str) {
    for _ in 0..150 {
        scheduler::process(st, "book").await.unwrap();
        let r = current(st);
        if let Some(i) = r.tasks.iter().position(|t| t.assigned_agent.is_some() && t.assignment.is_some()) { return finish(st, i, outcome).await; }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("no item was assigned");
}

async fn finish(st: &AppState, index: usize, outcome: &str) {
    // Background work (an item's copy) may come first: wait until the item is assigned.
    for _ in 0..150 {
        scheduler::process(st, "book").await.unwrap();
        let r = current(st);
        if task_at(&r, index).assignment.is_some() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let r = current(st);
    let t = if index < r.tasks.len() { &r.tasks[index] } else { &r.assembly };
    let agent = engine::agent_for(&r.config, t).unwrap();
    let assignment = t.assignment.clone().unwrap_or_else(|| panic!("never assigned: {:?} {:?} copy={:?} steps={} all={:?}", t.status, t.problem, t.copy, t.history.len(), r.tasks.iter().map(|x| (&x.stage, x.status, x.copy.is_some())).collect::<Vec<_>>()));
    event(st, agent, EventBody::MessageConsumed { message_id: assignment.clone(), turn_id: assignment.clone() });
    event(st, agent, EventBody::TurnStarted { turn_id: assignment.clone() });
    call(st, &format!("team.{agent}@book"), "complete_work", &json!({"assignment":assignment,"outcome":outcome,"result":"modules/item/result.tsx revision abc; fixture checks passed"})).unwrap();
    event(st, agent, EventBody::TurnEnded { turn_id: assignment, status: TurnStatus::Completed });
}

#[test]
fn dispatch_is_bounded_idempotent_and_scoped_to_the_configured_agent() {
    let st = setup(Store::open_in_memory().unwrap());
    assert!(dispatch(&st, 0).is_err()); assert!(dispatch(&st, 25).is_err());
    dispatch(&st, 2).unwrap(); dispatch(&st, 2).unwrap();
    assert_eq!(current(&st).tasks.len(), 2);
    assert!(dispatch(&st, 3).is_err());
    assert!(call(&st, "team.visualizer@book", "dispatch_work", &json!({"batch_id":"x","summary":"x","tasks":[]})).unwrap_err().contains("dispatcher"));
}

#[tokio::test]
async fn review_loops_keep_context_and_assembly_waits_for_every_item() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 2).unwrap();
    finish(&st, 0, "ready").await; finish(&st, 0, "ready").await; finish(&st, 0, "revise").await;
    assert_eq!(current(&st).tasks[0].stage, "visualize");
    assert_eq!(current(&st).tasks[0].input.context, "Source section 1; work only in modules/item/");
    finish(&st, 0, "ready").await; finish(&st, 0, "pass").await; finish(&st, 0, "accept").await;
    assert_eq!(current(&st).tasks[0].history.len(), 6);
    assert!(current(&st).assembly.assignment.is_none());
    for outcome in ["ready", "ready", "pass", "accept"] { finish(&st, 1, outcome).await; }
    finish(&st, 2, "done").await;
    assert_eq!(current(&st).assembly.status, TaskStatus::Complete);
}

#[tokio::test]
async fn a_replayed_scheduler_does_not_resend_an_unconfirmed_assignment() {
    let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("db.sqlite");
    let st = setup(Store::open(&path).unwrap()); dispatch(&st, 2).unwrap();
    scheduler::process(&st, "book").await.unwrap();
    let assignment = current(&st).tasks[0].assignment.clone();
    let restarted = AppState::new(Store::open(&path).unwrap(), "test".into(), vec![]);
    scheduler::process(&restarted, "book").await.unwrap();
    assert_eq!(current(&restarted).tasks[0].assignment, assignment);
    let count: i64 = restarted.store.lock().unwrap().connection().query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 1, "one assignment per shared worker; never silently resend");
}

#[tokio::test]
async fn stopped_projects_do_not_dispatch_or_accept_late_completions() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 1).unwrap();
    st.store.lock().unwrap().set_desired("book", Desired::Stopped).unwrap();
    st.local_event(Actor::Human("local".into()), Subject::Rig("book".into()), Provenance::Operator, EventBody::RigDesiredChanged { desired: Desired::Stopped }).unwrap();
    scheduler::process(&st, "book").await.unwrap();
    assert!(current(&st).tasks[0].assignment.is_none());
    assert!(call(&st, "team.reference@book", "complete_work", &json!({"assignment":"late","outcome":"ready","result":"x"})).unwrap_err().contains("stopped"));
}

#[tokio::test]
async fn completions_reject_wrong_agents_and_conflicting_retries() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 1).unwrap(); scheduler::process(&st, "book").await.unwrap();
    let id = current(&st).tasks[0].assignment.clone().unwrap();
    let args = json!({"assignment":id,"outcome":"ready","result":"source verified; modules/item/references.json"});
    assert!(call(&st, "team.visualizer@book", "complete_work", &args).is_err());
    call(&st, "team.reference@book", "complete_work", &args).unwrap();
    call(&st, "team.reference@book", "complete_work", &args).unwrap();
    assert_eq!(current(&st).tasks[0].history.len(), 1);
    let mut other = args; other["result"] = json!("different");
    assert!(call(&st, "team.reference@book", "complete_work", &other).is_err());
}

#[tokio::test]
async fn turn_end_without_a_report_blocks_but_consumption_alone_does_not() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 1).unwrap(); scheduler::process(&st, "book").await.unwrap();
    let id = current(&st).tasks[0].assignment.clone().unwrap();
    event(&st, "reference", EventBody::MessageConsumed { message_id: id.clone(), turn_id: id.clone() });
    scheduler::process(&st, "book").await.unwrap(); assert_eq!(current(&st).tasks[0].status, TaskStatus::Assigned);
    event(&st, "reference", EventBody::TurnEnded { turn_id: id, status: TurnStatus::Completed });
    scheduler::process(&st, "book").await.unwrap(); assert_eq!(current(&st).tasks[0].status, TaskStatus::Blocked);
    assert!(current(&st).assembly.assignment.is_none());
}

#[test]
fn graph_validation_rejects_dangling_routes_and_unbounded_cycles() {
    let mut d = review_fixture();
    assert!(rigspec::from_draft(&d).is_ok());
    d.workflow.as_mut().unwrap().stages[0].routes.insert("broken".into(), Some("missing".into()));
    assert!(rigspec::from_draft(&d).is_err());
    d.workflow.as_mut().unwrap().stages[0].routes = [("loop".into(), Some("reference".into()))].into();
    assert!(rigspec::from_draft(&d).is_err());
}

#[tokio::test]
async fn revision_loops_stop_at_the_configured_limit() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await;
    for _ in 0..5 { finish(&st, 0, "ready").await; finish(&st, 0, "revise").await; }
    finish(&st, 0, "ready").await;
    assert_eq!(current(&st).tasks[0].status, TaskStatus::Blocked);
    assert!(current(&st).tasks[0].problem.as_ref().unwrap().contains("limit"));
}

#[test]
fn workflow_configuration_and_input_survive_export_import() {
    let mut d = review_fixture();
    d.workflow.as_mut().unwrap().input = "Links, questions and ideas".into();
    let imported = rigspec::exchange::import_yaml(&rigspec::exchange::export_yaml(&d), |_| "book".into()).unwrap();
    assert_eq!(imported.draft.workflow, d.workflow);
}

pub(super) fn kinds_fixture() -> TeamDraft {
    let mut d = review_fixture();
    d.folder = Some("/tmp".into());
    let w = d.workflow.as_mut().unwrap();
    // reference --ready--> tests (check) --passed--> chief (human) --ship--> done
    w.stages[0].routes = [("ready".into(), Some("tests".into()))].into();
    w.stages.push(WorkflowStage { id: "tests".into(), name: "Tests".into(), kind: StageKind::Check, command: Some("true".into()),
        routes: [("passed".into(), Some("chief".into())), ("failed".into(), Some("reference".into()))].into(), ..Default::default() });
    w.stages.push(WorkflowStage { id: "chief".into(), name: "Sign-off".into(), kind: StageKind::Human, prompt: Some("Ship it?".into()),
        routes: [("ship".into(), None), ("rework".into(), Some("reference".into()))].into(), ..Default::default() });
    d
}

#[test]
fn check_and_human_stages_validate_their_own_fields() {
    let d = kinds_fixture();
    assert!(rigspec::from_draft(&d).is_ok(), "{:?}", rigspec::from_draft(&d).err());
    let bad = |f: &dyn Fn(&mut Workflow)| { let mut d = kinds_fixture(); f(d.workflow.as_mut().unwrap()); rigspec::from_draft(&d).is_err() };
    let at = |w: &Workflow, id: &str| w.stages.iter().position(|s| s.id == id).unwrap();
    assert!(bad(&|w| { let i = at(w, "tests"); w.stages[i].command = None; }), "check needs a command");
    assert!(bad(&|w| { let i = at(w, "tests"); w.stages[i].agent = "reference".into(); }), "check has no agent");
    assert!(bad(&|w| { let i = at(w, "tests"); w.stages[i].routes.remove("failed"); }), "check routes both results");
    assert!(bad(&|w| { let i = at(w, "tests"); w.stages[i].routes.insert("maybe".into(), None); }), "check routes only passed/failed");
    assert!(bad(&|w| { let i = at(w, "tests"); w.stages[i].timeout_s = Some(3601); }), "timeout ≤ 3600");
    assert!(bad(&|w| { let i = at(w, "chief"); w.stages[i].prompt = None; }), "human needs a prompt");
    assert!(bad(&|w| { let i = at(w, "chief"); w.stages[i].prompt = Some("x".repeat(2049)); }), "prompt ≤ 2 KB");
    assert!(bad(&|w| { let i = at(w, "chief"); w.stages[i].routes.remove("rework"); }), "human needs 2–6 outcomes");
    assert!(bad(&|w| { let i = at(w, "chief"); w.stages[i].command = Some("true".into()); }), "human has no command");
    assert!(bad(&|w| { w.stages[0].prompt = Some("x".into()); }), "agent stages take no prompt");
    assert!(bad(&|w| { w.stages[0].max_rounds = Some(21); }), "max_rounds ≤ 20");
    assert!(bad(&|w| { w.stages[0].on_limit = Some("missing".into()); w.stages[0].max_rounds = Some(2); }), "on_limit names a stage");
    assert!(bad(&|w| { w.stages[0].on_limit = Some("chief".into()); }), "on_limit needs max_rounds");
    let mut d = kinds_fixture(); d.folder = None;
    assert!(rigspec::from_draft(&d).is_err(), "check stages need the team's project folder");
}

#[test]
fn runs_and_team_files_written_before_stage_kinds_still_load() {
    let old_stage = json!({"id":"a","name":"A","agent":"x","routes":{"done":null}});
    let s: WorkflowStage = serde_json::from_value(old_stage).unwrap();
    assert_eq!(s.kind, StageKind::Agent); assert_eq!(s.max_rounds, None);
    let old_step = json!({"assignment":"1","stage":"a","agent":"x","outcome":"done","result":"r","at_ms":1});
    let step: WorkflowStep = serde_json::from_value(old_step).unwrap();
    assert_eq!(step.by, StepBy::Agent); assert_eq!(step.evidence, None);
    let old_task = json!({"input":{"id":"t","title":"T","context":"c"},"stage":"a","status":"queued","assignment":null,"history":[],"problem":null});
    assert!(serde_json::from_value::<WorkflowTask>(old_task).unwrap().waiting.is_none());
    let out = serde_json::to_value(&s).unwrap();
    assert!(out.get("kind").is_none() && out.get("command").is_none(), "agent stages export as before");
}

fn human_fixture() -> TeamDraft { // reference --ready--> chief (human); no check stage
    let mut d = kinds_fixture();
    let w = d.workflow.as_mut().unwrap();
    w.stages[0].routes = [("ready".into(), Some("chief".into()))].into();
    w.stages.retain(|s| s.id == "reference" || s.id == "chief");
    d
}

#[tokio::test]
async fn an_item_waits_at_a_human_stage_until_a_person_decides_once() {
    let st = setup_draft(Store::open_in_memory().unwrap(), human_fixture()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!((t.status.as_str(), t.stage.as_str()), ("waiting", "chief"));
    assert_eq!(t.waiting.as_ref().unwrap().stage, "chief");
    let start = current(&st).start_seq;
    assert!(routes::decide_task(&st, "book", "item-0", "nope", "", start, false).is_err(), "unknown outcome");
    assert!(routes::decide_task(&st, "book", "item-0", "ship", &"x".repeat(4097), start, false).is_err(), "note ≤ 4 KB");
    assert!(routes::decide_task(&st, "book", "item-0", "ship", "", start - 1, false).is_err(), "earlier run");
    routes::decide_task(&st, "book", "item-0", "ship", "Looks right", start, false).unwrap();
    assert!(routes::decide_task(&st, "book", "item-0", "ship", "", start, false).unwrap_err().message.contains("not waiting"), "second decision refused");
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Complete);
    assert_eq!(t.history.len(), 2);
    let last = t.history.last().unwrap();
    assert_eq!((last.by, last.outcome.as_str(), last.result.as_str()), (StepBy::Person, "ship", "Looks right"));
    assert!(t.waiting.is_none());
}

#[tokio::test]
async fn deciding_is_refused_once_the_team_stops() {
    let st = setup_draft(Store::open_in_memory().unwrap(), human_fixture()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let start = current(&st).start_seq;
    st.store.lock().unwrap().set_desired("book", Desired::Stopped).unwrap();
    assert!(routes::decide_task(&st, "book", "item-0", "ship", "", start, false).unwrap_err().message.contains("stopped"));
}

#[tokio::test]
async fn waiting_items_appear_in_the_inbox_with_their_prompt_and_outcomes() {
    let st = setup_draft(Store::open_in_memory().unwrap(), human_fixture()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let items = crate::questions::inbox(&st).unwrap();
    let d = items.iter().find(|i| i.kind == crate::views::InboxKind::Decision).expect("decision item").decision.as_ref().unwrap();
    assert_eq!((d.task.as_str(), d.prompt.as_str()), ("item-0", "Ship it?"));
    assert_eq!(d.outcomes, vec!["rework".to_string(), "ship".into()]);
    assert_eq!(d.recent.last().unwrap().outcome, "ready");
}

#[tokio::test]
async fn a_round_limit_takes_the_on_limit_route_or_blocks() {
    let mut d = human_fixture();
    let w = d.workflow.as_mut().unwrap();
    // chief --rework--> reference: reference may be entered at most twice, then the item goes back to chief.
    w.stages[0].max_rounds = Some(2); w.stages[0].on_limit = Some("chief".into());
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    let start = current(&st).start_seq;
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    routes::decide_task(&st, "book", "item-0", "rework", "again", start, false).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    routes::decide_task(&st, "book", "item-0", "rework", "again", start, false).unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.stage, "chief", "third entry to reference is past the limit");
    assert!(t.history.last().unwrap().evidence.as_deref().unwrap().contains("Round limit (2) reached"));
    // Without on_limit the item is blocked instead.
    let mut d = human_fixture(); d.workflow.as_mut().unwrap().stages[0].max_rounds = Some(1);
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    let start = current(&st).start_seq;
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    routes::decide_task(&st, "book", "item-0", "rework", "", start, false).unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Blocked); assert!(t.problem.as_deref().unwrap().contains("Round limit (1)"));
}

#[tokio::test]
async fn agent_steps_are_labelled_agent_reported() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await;
    assert_eq!(current(&st).tasks[0].history[0].by, StepBy::Agent);
}

fn kinds_state() -> AppState { setup_draft(Store::open_in_memory().unwrap(), kinds_fixture()) }

/// Runs the scheduler until item 0 leaves the check stage (checks run as background jobs).
async fn settle_checks(st: &AppState) {
    for _ in 0..150 {
        scheduler::process(st, "book").await.unwrap();
        let t = current(st).tasks[0].clone();
        if t.stage != "tests" { return; }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the check never finished");
}

#[tokio::test]
async fn a_passing_check_routes_passed_with_verified_evidence() {
    let st = kinds_state(); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; settle_checks(&st).await;
    let t = &current(&st).tasks[0];
    let step = t.history.last().unwrap();
    assert_eq!((step.stage.as_str(), step.outcome.as_str(), step.by), ("tests", "passed", StepBy::Check));
    let evidence = step.evidence.as_deref().unwrap();
    assert!(evidence.contains("$ true") && evidence.contains("exit 0"), "{evidence}");
    scheduler::process(&st, "book").await.unwrap();
    assert_eq!(current(&st).tasks[0].status, TaskStatus::Waiting, "passed → the human sign-off");
}

#[tokio::test]
async fn a_failing_or_hung_check_routes_failed() {
    let mut d = kinds_fixture();
    let w = d.workflow.as_mut().unwrap(); let i = w.stages.iter().position(|s| s.id == "tests").unwrap();
    w.stages[i].command = Some("echo boom; sleep 5".into()); w.stages[i].timeout_s = Some(1);
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; settle_checks(&st).await;
    let step = current(&st).tasks[0].history.last().cloned().unwrap();
    assert_eq!((step.outcome.as_str(), step.by), ("failed", StepBy::Check));
    assert!(step.evidence.unwrap().contains("timed out"));
    assert_eq!(current(&st).tasks[0].stage, "reference", "failed → back to the agent");
}

#[tokio::test]
async fn a_check_lost_to_a_restart_runs_again() {
    let st = kinds_state(); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await;
    // A restart: the item is assigned at the check stage, but no job runs for it in this process.
    { let mut store = st.store.lock().unwrap(); let mut run = store.latest_workflow("book").unwrap().unwrap();
      let t = &mut run.tasks[0]; t.stage = "tests".into(); t.status = TaskStatus::Assigned; t.assignment = Some("lost".into()); store.save_workflow(&run, None).unwrap(); }
    settle_checks(&st).await;
    assert_eq!(current(&st).tasks[0].history.last().unwrap().outcome, "passed");
}

#[tokio::test]
async fn a_dependent_topic_waits_for_its_prerequisite_even_at_a_check_or_human_stage() {
    let mut d = kinds_fixture(); d.workflow.as_mut().unwrap().entry = "tests".into();
    let st = setup_draft(Store::open_in_memory().unwrap(), d);
    call(&st, "team.dispatcher@book", "dispatch_work", &json!({"batch_id":"deps","summary":"s","tasks":[
        {"id":"a","title":"A","context":"a","mode":"independent","depends_on":[]},
        {"id":"b","title":"B","context":"b","mode":"dependent","depends_on":["a"]}]})).unwrap();
    settle_checks(&st).await; scheduler::process(&st, "book").await.unwrap();
    let r = current(&st);
    assert_eq!(r.tasks[0].status, TaskStatus::Waiting, "A passed its check and waits for sign-off");
    assert_eq!((r.tasks[1].status.as_str(), r.tasks[1].history.len()), ("queued", 0), "B's check must not run before A is accepted");
}

#[tokio::test]
async fn stopping_the_team_stops_a_running_check_and_says_so() {
    let mut d = kinds_fixture();
    let w = d.workflow.as_mut().unwrap(); let i = w.stages.iter().position(|s| s.id == "tests").unwrap();
    w.stages[i].command = Some("sleep 30".into());
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    assert_eq!((current(&st).tasks[0].stage.as_str(), current(&st).tasks[0].status.as_str()), ("tests", "assigned"));
    st.store.lock().unwrap().set_desired("book", Desired::Stopped).unwrap();
    scheduler::process(&st, "book").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Blocked);
    let step = t.history.last().unwrap();
    assert_eq!((step.by, step.outcome.as_str()), (StepBy::Sculptured, "blocked"));
    assert!(t.problem.as_deref().unwrap().contains("stopped"), "{:?}", t.problem);
}

#[tokio::test]
async fn a_check_that_cannot_run_blocks_instead_of_reporting_a_verified_failure() {
    let mut d = kinds_fixture(); d.folder = Some("/nonexistent/sculptured-test-folder".into());
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await;
    for _ in 0..100 { scheduler::process(&st, "book").await.unwrap(); if current(&st).tasks[0].status == TaskStatus::Blocked { break; } tokio::time::sleep(std::time::Duration::from_millis(20)).await; }
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Blocked);
    assert_eq!(t.history.last().unwrap().by, StepBy::Sculptured, "not a verified check result");
    assert!(t.problem.as_deref().unwrap().contains("folder"));
}

#[tokio::test]
async fn retrying_a_round_limit_block_grants_one_more_round_at_the_limited_stage() {
    let mut d = human_fixture(); d.workflow.as_mut().unwrap().stages[0].max_rounds = Some(1);
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    let start = current(&st).start_seq;
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    routes::decide_task(&st, "book", "item-0", "rework", "", start, false).unwrap();
    assert_eq!((current(&st).tasks[0].status.as_str(), current(&st).tasks[0].stage.as_str()), ("blocked", "reference"));
    routes::retry_task(&st, "book", "item-0", start).unwrap();
    scheduler::process(&st, "book").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!((t.stage.as_str(), t.status.as_str(), t.assigned_agent.as_deref()), ("reference", "assigned", Some("reference")));
}

#[tokio::test]
async fn an_on_limit_stage_over_its_own_limit_blocks_instead_of_bouncing() {
    let mut d = human_fixture();
    let w = d.workflow.as_mut().unwrap();
    // reference (max 1, on_limit chief); chief (max 1): the second rework hits reference's limit, and chief is also spent.
    w.stages[0].max_rounds = Some(1); w.stages[0].on_limit = Some("chief".into());
    w.stages[1].max_rounds = Some(1);
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    let start = current(&st).start_seq;
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    routes::decide_task(&st, "book", "item-0", "rework", "", start, false).unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Blocked, "{t:?}");
    assert!(t.problem.as_deref().unwrap().contains("Sign-off"));
    // Parked at the stage the item was sent to, so Retry grants that stage one more round instead of
    // asking the spent on_limit stage again.
    assert_eq!(t.stage, "reference", "{t:?}");
    routes::retry_task(&st, "book", "item-0", start).unwrap();
    scheduler::process(&st, "book").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!((t.stage.as_str(), t.status, t.assigned_agent.as_deref()), ("reference", TaskStatus::Assigned, Some("reference")), "{t:?}");
}

#[test]
fn round_limit_routes_count_as_a_way_to_finish_and_cannot_point_at_their_own_stage() {
    let mut d = human_fixture();
    // reference only loops to itself, but its round-limit route reaches chief, which can finish.
    let w = d.workflow.as_mut().unwrap();
    w.stages[0].routes = [("again".into(), Some("reference".into()))].into();
    w.stages[0].max_rounds = Some(3); w.stages[0].on_limit = Some("chief".into());
    assert!(rigspec::from_draft(&d).is_ok(), "{:?}", rigspec::from_draft(&d).err());
    d.workflow.as_mut().unwrap().stages[0].on_limit = Some("reference".into());
    assert!(rigspec::from_draft(&d).is_err());
}

#[tokio::test]
async fn agents_see_check_results_labelled_as_verified_not_agent_reported() {
    let mut d = kinds_fixture();
    let w = d.workflow.as_mut().unwrap(); let i = w.stages.iter().position(|s| s.id == "tests").unwrap();
    w.stages[i].command = Some("echo nope; false".into());
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; settle_checks(&st).await; scheduler::process(&st, "book").await.unwrap();
    let id = current(&st).tasks[0].assignment.clone().expect("back with the agent");
    let text: String = st.store.lock().unwrap().connection().query_row("SELECT text FROM messages WHERE id=?1", [&id], |r| r.get(0)).unwrap();
    assert!(text.contains("verified") && text.contains("exit 1"), "{text}");
    assert!(!text.contains("Prior stage reports (agent-reported, not independent verification)"));
}

pub(super) fn panel_fixture() -> TeamDraft { // reference --ready--> panel (two reviewers) --approved--> done, --changes--> reference
    let mut d = review_fixture();
    let w = d.workflow.as_mut().unwrap();
    let agents: Vec<String> = w.stages.iter().skip(1).take(2).map(|s| s.agent.clone()).collect();
    w.stages[0].routes = [("ready".into(), Some("panel".into()))].into();
    w.stages.truncate(1);
    w.stages.push(WorkflowStage { id: "panel".into(), name: "Review panel".into(), reviewers: agents,
        routes: [("approved".into(), None), ("changes".into(), Some("reference".into()))].into(), ..Default::default() });
    d
}

#[test]
fn panels_merges_and_per_item_copies_validate() {
    assert!(rigspec::from_draft(&panel_fixture()).is_ok(), "{:?}", rigspec::from_draft(&panel_fixture()).err());
    let bad = |f: &dyn Fn(&mut Workflow)| { let mut d = panel_fixture(); f(d.workflow.as_mut().unwrap()); rigspec::from_draft(&d).is_err() };
    assert!(bad(&|w| { w.stages[1].reviewers.truncate(1); }), "2–6 reviewers");
    assert!(bad(&|w| { let r = w.stages[1].reviewers[0].clone(); w.stages[1].reviewers.push(r); }), "distinct reviewers");
    assert!(bad(&|w| { w.stages[1].agent = "reference".into(); }), "reviewers exclude agent");
    assert!(bad(&|w| { w.stages[1].reviewers.push("missing".into()); }), "existing reviewers");
    assert!(bad(&|w| { w.stages[0].join = Join::Any; }), "only a panel has a join rule");
    let merge = WorkflowStage { id: "merge".into(), name: "Merge".into(), kind: StageKind::Merge,
        routes: [("merged".into(), None), ("conflict".into(), Some("reference".into()))].into(), ..Default::default() };
    let m = merge.clone();
    assert!(bad(&move |w| { w.stages[1].routes.insert("approved".into(), Some("merge".into())); w.stages.push(m.clone()); }), "merge needs per-item copies");
    let mut d = panel_fixture(); d.folder = Some("/tmp".into());
    { let w = d.workflow.as_mut().unwrap(); w.copies = CopiesMode::PerItem; w.stages[1].routes.insert("approved".into(), Some("merge".into())); w.stages.push(merge.clone()); }
    assert!(rigspec::from_draft(&d).is_ok(), "{:?}", rigspec::from_draft(&d).err());
    { let w = d.workflow.as_mut().unwrap(); w.stages[2].routes.insert("maybe".into(), None); }
    assert!(rigspec::from_draft(&d).is_err(), "merge routes exactly merged and conflict");
    let mut d = panel_fixture(); d.folder = None; d.workflow.as_mut().unwrap().copies = CopiesMode::PerItem;
    assert!(rigspec::from_draft(&d).is_err(), "per-item copies need the project folder");
}

#[test]
fn runs_written_before_panels_and_copies_still_load() {
    let w: Workflow = serde_json::from_value(json!({"input":"","dispatcher":"d","entry":"a","collector":"c","max_tasks":1,"max_steps":4,"stages":[]})).unwrap();
    assert_eq!(w.copies, CopiesMode::Shared);
    assert!(serde_json::to_value(&w).unwrap().get("copies").is_none(), "shared exports as before");
    let t: WorkflowTask = serde_json::from_value(json!({"input":{"id":"t","title":"T","context":"c"},"stage":"a","status":"queued","assignment":null,"history":[],"problem":null})).unwrap();
    assert!(t.copy.is_none() && t.reviews.is_empty());
}

pub(super) fn git_in(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git").current_dir(dir).args(["-c", "user.name=T", "-c", "user.email=t@t"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A project repository and a copies folder, both temporary.
pub(super) struct ItemRepo { repo: tempfile::TempDir, _copies: tempfile::TempDir }
impl ItemRepo { pub(super) fn path(&self) -> &std::path::Path { self.repo.path() } }

/// kinds_fixture with a copy per item, over a fresh git repository (one commit: README).
pub(super) fn per_item_state() -> (AppState, ItemRepo) { per_item_state_with(|_| {}) }

pub(super) fn per_item_state_with(change: impl FnOnce(&mut Workflow)) -> (AppState, ItemRepo) {
    let (repo, copies) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    git_in(repo.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(repo.path().join("README"), "start\n").unwrap();
    git_in(repo.path(), &["add", "-A"]); git_in(repo.path(), &["commit", "-qm", "first"]);
    let mut d = kinds_fixture(); d.folder = Some(repo.path().display().to_string());
    let w = d.workflow.as_mut().unwrap(); w.copies = CopiesMode::PerItem;
    let i = w.stages.iter().position(|s| s.id == "tests").unwrap(); w.stages[i].command = Some("pwd".into());
    change(w);
    let mut store = Store::open_in_memory().unwrap();
    let mut s = store.settings().unwrap(); s.copies_folder = Some(copies.path().display().to_string()); store.save_settings(&s).unwrap();
    (setup_draft(store, d), ItemRepo { repo, _copies: copies })
}

#[tokio::test]
async fn an_item_gets_its_own_copy_before_its_first_assignment_and_the_message_names_it() {
    let (st, _repo) = per_item_state(); dispatch(&st, 1).unwrap();
    settle_copies(&st, 0).await; scheduler::process(&st, "book").await.unwrap();
    let t = current(&st).tasks[0].clone();
    let copy = t.copy.clone().expect("copy made");
    assert!(std::path::Path::new(&copy).join(".git").exists());
    assert!(copy.contains("/item-") && copy.ends_with("-item-0"), "{copy}");
    let text: String = st.store.lock().unwrap().connection().query_row("SELECT text FROM messages WHERE id=?1", [t.assignment.unwrap()], |r| r.get(0)).unwrap();
    assert!(text.contains(&format!("Item folder: {copy}")), "{text}");
}

#[tokio::test]
async fn an_item_whose_copy_cannot_be_made_blocks_with_the_reason() {
    let (st, repo) = per_item_state();
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap(); // no longer a repository
    dispatch(&st, 1).unwrap();
    for _ in 0..150 { scheduler::process(&st, "book").await.unwrap(); if current(&st).tasks[0].status == TaskStatus::Blocked { break; } tokio::time::sleep(std::time::Duration::from_millis(20)).await; }
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Blocked);
    assert_eq!(t.history.last().unwrap().by, StepBy::Sculptured);
    assert!(t.problem.as_deref().unwrap().contains("git"), "{:?}", t.problem);
}

#[tokio::test]
async fn a_check_stage_runs_in_the_items_copy_when_items_have_copies() {
    let (st, _repo) = per_item_state(); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; settle_checks(&st).await;
    let t = &current(&st).tasks[0];
    let evidence = t.history.last().unwrap().evidence.clone().unwrap();
    let copy = std::fs::canonicalize(t.copy.as_deref().unwrap()).unwrap();
    assert!(evidence.contains(copy.to_str().unwrap()) || evidence.contains(t.copy.as_deref().unwrap()), "{evidence}");
}

/// per_item_state() with: reference --ready--> merge (merged → done, conflict → reference).
fn merge_state() -> (AppState, ItemRepo) {
    per_item_state_with(|w| {
        w.stages.retain(|s| s.id == "reference");
        w.stages[0].routes = [("ready".into(), Some("merge".into()))].into();
        w.stages.push(WorkflowStage { id: "merge".into(), name: "Merge".into(), kind: StageKind::Merge,
            routes: [("merged".into(), None), ("conflict".into(), Some("reference".into()))].into(), ..Default::default() });
    })
}

/// Runs the scheduler until item `index` leaves `stage` (merges and checks run in the background).
async fn settle_stage(st: &AppState, index: usize, stage: &str) {
    for _ in 0..150 {
        scheduler::process(st, "book").await.unwrap();
        let t = current(st).tasks[index].clone();
        if t.stage != stage || matches!(t.status.as_str(), "complete" | "blocked") { return; }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("{stage} never finished");
}

#[tokio::test]
async fn a_merge_stage_merges_the_items_branch_and_reports_what_changed() {
    let (st, repo) = merge_state(); dispatch(&st, 1).unwrap();
    settle_copies(&st, 0).await;
    let copy = current(&st).tasks[0].copy.clone().unwrap();
    std::fs::write(std::path::Path::new(&copy).join("fix.txt"), "fixed").unwrap();
    finish(&st, 0, "ready").await; settle_stage(&st, 0, "merge").await;
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Complete);
    let step = t.history.last().unwrap();
    assert_eq!((step.outcome.as_str(), step.by), ("merged", StepBy::Sculptured));
    assert!(step.evidence.as_deref().unwrap().contains("fix.txt"));
    assert_eq!(std::fs::read_to_string(repo.path().join("fix.txt")).unwrap(), "fixed");
    assert!(t.copy.is_none(), "a merged item's copy is gone");
}

#[tokio::test]
async fn a_conflicting_merge_routes_conflict_and_leaves_the_project_untouched() {
    let (st, repo) = merge_state(); dispatch(&st, 1).unwrap();
    settle_copies(&st, 0).await;
    let copy = current(&st).tasks[0].copy.clone().unwrap();
    std::fs::write(std::path::Path::new(&copy).join("README"), "item\n").unwrap();
    std::fs::write(repo.path().join("README"), "main\n").unwrap();
    git_in(repo.path(), &["commit", "-qam", "main moves"]);
    finish(&st, 0, "ready").await; settle_stage(&st, 0, "merge").await;
    let t = &current(&st).tasks[0];
    assert_eq!((t.stage.as_str(), t.history.last().unwrap().outcome.as_str()), ("reference", "conflict"));
    assert_eq!(std::fs::read_to_string(repo.path().join("README")).unwrap(), "main\n");
}

#[tokio::test]
async fn merges_of_one_team_run_one_at_a_time() {
    let (st, _repo) = merge_state(); dispatch(&st, 2).unwrap();
    // Copies are made in the background, so either item may reach the one agent first.
    for _ in 0..2 { finish_next(&st, "ready").await; }
    for _ in 0..150 {
        scheduler::process(&st, "book").await.unwrap();
        let r = current(&st);
        assert!(r.tasks.iter().filter(|t| t.stage == "merge" && t.status == TaskStatus::Assigned).count() <= 1, "two merges at once");
        if r.tasks.iter().all(|t| t.status == TaskStatus::Complete) { return; }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("merges never finished: {:?}", current(&st).tasks.iter().map(|t| (&t.stage, &t.status, &t.problem)).collect::<Vec<_>>());
}

/// One reviewer votes on item 0 through complete_work, ending its turn as the CLI would.
async fn vote(st: &AppState, agent: &str, outcome: &str) -> Result<serde_json::Value, String> {
    let r = current(st); let t = &r.tasks[0];
    let a = t.reviews.iter().find(|x| x.agent == agent).and_then(|x| x.assignment.clone()).expect("reviewer assigned");
    event(st, agent, EventBody::MessageConsumed { message_id: a.clone(), turn_id: a.clone() });
    let res = call(st, &format!("team.{agent}@book"), "complete_work", &json!({"assignment":a,"outcome":outcome,"result":format!("{agent} says {outcome}")}));
    event(st, agent, EventBody::TurnEnded { turn_id: a, status: TurnStatus::Completed });
    res
}

#[tokio::test]
async fn a_panel_assigns_every_reviewer_and_all_must_approve() {
    let d = panel_fixture(); let reviewers = d.workflow.as_ref().unwrap().stages[1].reviewers.clone();
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    assert!(current(&st).tasks[0].reviews.iter().all(|r| r.assignment.is_some()), "both reviewers assigned at once");
    vote(&st, &reviewers[0], "approved").await.unwrap();
    assert!(vote(&st, &reviewers[0], "changes").await.is_err(), "no second vote");
    scheduler::process(&st, "book").await.unwrap();
    assert_eq!(current(&st).tasks[0].stage, "panel", "waits for every reviewer");
    vote(&st, &reviewers[1], "changes").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.stage, "reference", "one 'changes' sends it back");
    let step = t.history.last().unwrap();
    assert_eq!((step.votes.len(), step.outcome.as_str()), (2, "changes"));
    assert!(t.reviews.is_empty());
}

#[tokio::test]
async fn an_any_panel_completes_on_the_first_approval_and_records_late_votes() {
    let mut d = panel_fixture(); d.workflow.as_mut().unwrap().stages[1].join = Join::Any;
    let reviewers = d.workflow.as_ref().unwrap().stages[1].reviewers.clone();
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let slow = current(&st).tasks[0].reviews.iter().find(|r| r.agent == reviewers[0]).and_then(|r| r.assignment.clone()).unwrap();
    vote(&st, &reviewers[1], "approved").await.unwrap();
    assert_eq!(current(&st).tasks[0].status, TaskStatus::Complete);
    call(&st, &format!("team.{}@book", reviewers[0]), "complete_work", &json!({"assignment":slow,"outcome":"changes","result":"late"})).unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Complete, "a late vote is ignored");
    assert_eq!(t.history.last().unwrap().votes.get(&format!("{} (late)", reviewers[0])).map(String::as_str), Some("changes"), "but recorded");
}

#[tokio::test]
async fn a_reviewer_ending_without_a_vote_blocks_the_item() {
    let d = panel_fixture(); let reviewers = d.workflow.as_ref().unwrap().stages[1].reviewers.clone();
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let a = current(&st).tasks[0].reviews[0].assignment.clone().unwrap();
    event(&st, &reviewers[0], EventBody::MessageConsumed { message_id: a.clone(), turn_id: a.clone() });
    event(&st, &reviewers[0], EventBody::TurnEnded { turn_id: a, status: TurnStatus::Completed });
    scheduler::process(&st, "book").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Blocked);
    assert!(t.problem.as_deref().unwrap().contains("without a vote"), "{:?}", t.problem);
}

#[tokio::test]
async fn an_items_copy_and_branch_belong_to_its_batch() {
    let (st, _repo) = per_item_state(); dispatch(&st, 1).unwrap();
    settle_copies(&st, 0).await;
    let r = current(&st);
    assert!(r.tasks[0].copy.as_deref().unwrap().ends_with(&format!("item-{}-item-0", r.start_seq)), "{:?}", r.tasks[0].copy);
}

/// Runs the scheduler until item `index` has its copy (copies are made in the background).
async fn settle_copies(st: &AppState, index: usize) {
    for _ in 0..150 {
        scheduler::process(st, "book").await.unwrap();
        if current(st).tasks[index].copy.is_some() { return; }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the copy was never made");
}

#[tokio::test]
async fn copies_are_made_in_the_background_a_few_at_a_time() {
    let (st, _repo) = per_item_state(); dispatch(&st, 4).unwrap();
    scheduler::process(&st, "book").await.unwrap();
    assert!(st.workflow_jobs.count("book", jobs::Kind::Copy) <= 2, "at most two copies at once");
    for i in 0..4 { settle_copies(&st, i).await; }
}

#[tokio::test]
async fn a_check_after_a_merge_tests_the_project_not_a_new_copy() {
    let (st, repo) = per_item_state_with(|w| {
        w.stages.retain(|s| s.id == "reference" || s.id == "tests");
        w.stages[0].routes = [("ready".into(), Some("merge".into()))].into();
        let t = w.stages.iter_mut().find(|s| s.id == "tests").unwrap();
        t.routes = [("passed".into(), None), ("failed".into(), Some("reference".into()))].into();
        w.stages.push(WorkflowStage { id: "merge".into(), name: "Merge".into(), kind: StageKind::Merge,
            routes: [("merged".into(), Some("tests".into())), ("conflict".into(), Some("reference".into()))].into(), ..Default::default() });
    });
    dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; settle_stage(&st, 0, "merge").await; settle_stage(&st, 0, "tests").await;
    let t = &current(&st).tasks[0];
    assert!(t.copy.is_none(), "no copy made again after the merge");
    let evidence = t.history.last().unwrap().evidence.clone().unwrap();
    let project = std::fs::canonicalize(repo.path()).unwrap();
    assert!(evidence.contains(project.to_str().unwrap()) && !evidence.contains("item-"), "{evidence}");
}

#[tokio::test]
async fn retrying_a_blocked_panel_keeps_the_votes_already_given() {
    let d = panel_fixture(); let reviewers = d.workflow.as_ref().unwrap().stages[1].reviewers.clone();
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    vote(&st, &reviewers[0], "approved").await.unwrap();
    // The second reviewer's turn ends without a vote: blocked.
    let a = current(&st).tasks[0].reviews[1].assignment.clone().unwrap();
    event(&st, &reviewers[1], EventBody::MessageConsumed { message_id: a.clone(), turn_id: a.clone() });
    event(&st, &reviewers[1], EventBody::TurnEnded { turn_id: a, status: TurnStatus::Completed });
    scheduler::process(&st, "book").await.unwrap();
    assert_eq!(current(&st).tasks[0].status, TaskStatus::Blocked);
    routes::retry_task(&st, "book", "item-0", current(&st).start_seq).unwrap();
    scheduler::process(&st, "book").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.reviews[0].outcome.as_deref(), Some("approved"), "the first vote is kept");
    assert!(t.reviews[1].assignment.is_some() && t.reviews[1].outcome.is_none(), "only the silent reviewer is asked again");
    vote(&st, &reviewers[1], "approved").await.unwrap();
    assert_eq!(current(&st).tasks[0].status, TaskStatus::Complete);
}

#[tokio::test]
async fn a_vote_on_a_blocked_panel_is_kept_but_does_not_move_it() {
    let mut d = panel_fixture(); d.workflow.as_mut().unwrap().stages[1].join = Join::Any;
    let reviewers = d.workflow.as_ref().unwrap().stages[1].reviewers.clone();
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let a = current(&st).tasks[0].reviews[0].assignment.clone().unwrap();
    event(&st, &reviewers[0], EventBody::MessageConsumed { message_id: a.clone(), turn_id: a.clone() });
    event(&st, &reviewers[0], EventBody::TurnEnded { turn_id: a, status: TurnStatus::Completed });
    scheduler::process(&st, "book").await.unwrap();
    assert_eq!(current(&st).tasks[0].status, TaskStatus::Blocked);
    vote(&st, &reviewers[1], "approved").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!(t.status, TaskStatus::Blocked, "blocked for inspection stays blocked");
    assert_eq!(t.reviews[1].outcome.as_deref(), Some("approved"));
}

#[test]
fn panel_priority_names_real_outcomes() {
    let mut d = panel_fixture(); d.workflow.as_mut().unwrap().stages[1].priority = vec!["changes".into(), "aproved".into()];
    assert!(rigspec::from_draft(&d).is_err());
    d.workflow.as_mut().unwrap().stages[1].priority = vec!["blocked".into(), "changes".into(), "approved".into()];
    assert!(rigspec::from_draft(&d).is_ok());
}

#[tokio::test]
async fn a_steps_tokens_come_from_the_turn_that_took_its_assignment() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await;
    let a = current(&st).tasks[0].history[0].assignment.clone();
    // Usage arrives after complete_work, at the turn's end, in two reports.
    for (i, o) in [(100, 20), (50, 5)] {
        event(&st, "reference", EventBody::UsageReported { in_turn: false, turn_id: Some(a.clone()), input_tokens: i, output_tokens: o, cache_read_tokens: 7, cache_write_tokens: 0, cost: None });
    }
    event(&st, "reference", EventBody::UsageReported { in_turn: false, turn_id: Some("other".into()), input_tokens: 999, output_tokens: 999, cache_read_tokens: 0, cache_write_tokens: 0, cost: None });
    let start = current(&st).start_seq;
    let usage = st.store.lock().unwrap().assignment_usage(&[a.clone(), "never-sent".into()], start).unwrap();
    // Only this run's reports count: nothing before the run's start is read.
    assert!(st.store.lock().unwrap().assignment_usage(&[a.clone()], i64::MAX).unwrap().is_empty());
    assert_eq!(usage.get(&a), Some(&crate::views::TurnTokens { input: 150, output: 25, cache_read: 14, cache_write: 0, ..Default::default() }));
    assert!(!usage.contains_key("never-sent"), "no report → absent, never zero");
}

#[tokio::test]
async fn a_panels_assignments_count_every_reviewer() {
    let d = panel_fixture(); let reviewers = d.workflow.as_ref().unwrap().stages[1].reviewers.clone();
    let st = setup_draft(Store::open_in_memory().unwrap(), d); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let pending: Vec<String> = current(&st).tasks[0].reviews.iter().filter_map(|r| r.assignment.clone()).collect();
    for r in &reviewers { vote(&st, r, "approved").await.unwrap(); }
    let all = routes::assignments(&current(&st));
    assert!(pending.iter().all(|a| all.contains(a)), "both reviewers' assignments are counted");
}

#[tokio::test]
async fn a_steps_cost_sums_the_reported_estimates_and_says_where_they_came_from() {
    let st = setup(Store::open_in_memory().unwrap()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await;
    let a = current(&st).tasks[0].history[0].assignment.clone();
    let cost = |usd| Some(Cost { usd, source: CostSource::ClaudeCode });
    for c in [cost(0.02), cost(0.03), None] {
        event(&st, "reference", EventBody::UsageReported { in_turn: false, turn_id: Some(a.clone()), input_tokens: 1, output_tokens: 1, cache_read_tokens: 0, cache_write_tokens: 0, cost: c });
    }
    let start = current(&st).start_seq; // read before locking the store: current() locks it too
    let usage = st.store.lock().unwrap().assignment_usage(&[a.clone()], start).unwrap();
    let t = &usage[&a];
    assert!((t.cost_usd.unwrap() - 0.05).abs() < 1e-9);
    assert_eq!((t.cost_source, t.cost_partial), (Some(CostSource::ClaudeCode), true), "one report had no cost");
}

#[test]
fn escalation_guard_and_schedules_validate() {
    let ok = |d: &TeamDraft| rigspec::from_draft(d).is_ok();
    let mut d = human_fixture();
    d.workflow.as_mut().unwrap().stages[0].escalate = Some(Escalation { after_rounds: 2, agent: "visualizer".into() });
    assert!(ok(&d), "{:?}", rigspec::from_draft(&d).err());
    let bad = |base: &TeamDraft, f: &dyn Fn(&mut TeamDraft)| { let mut x = base.clone(); f(&mut x); !ok(&x) };
    assert!(bad(&d, &|x| x.workflow.as_mut().unwrap().stages[0].escalate.as_mut().unwrap().after_rounds = 0), "after_rounds ≥ 1");
    assert!(bad(&d, &|x| x.workflow.as_mut().unwrap().stages[0].escalate.as_mut().unwrap().agent = "missing".into()), "existing agent");
    assert!(bad(&d, &|x| x.workflow.as_mut().unwrap().stages[0].escalate.as_mut().unwrap().agent = "reference".into()), "not the stage's own agent");
    assert!(bad(&d, &|x| x.workflow.as_mut().unwrap().stages[0].max_rounds = Some(2)), "after_rounds below max_rounds");
    assert!(bad(&d, &|x| x.workflow.as_mut().unwrap().stages[1].escalate = Some(Escalation { after_rounds: 1, agent: "visualizer".into() })), "only agent stages escalate");
    d.workflow.as_mut().unwrap().guard = Some(Guard { command: "npm test".into(), every_minutes: 15, timeout_s: None });
    assert!(ok(&d));
    assert!(bad(&d, &|x| x.workflow.as_mut().unwrap().guard.as_mut().unwrap().every_minutes = 4), "guard every 5–1440 minutes");
    assert!(bad(&d, &|x| x.workflow.as_mut().unwrap().guard.as_mut().unwrap().command.clear()), "guard needs a command");
    let s = |id: &str| Schedule { id: id.into(), agent: "visualizer".into(), every_minutes: 30, message: "Summarise the board".into() };
    d.schedules = vec![s("summary")];
    assert!(ok(&d));
    assert!(bad(&d, &|x| x.schedules.push(s("summary"))), "unique schedule ids");
    assert!(bad(&d, &|x| x.schedules[0].agent = "missing".into()), "existing agent");
    assert!(bad(&d, &|x| x.schedules[0].every_minutes = 2000), "every 5–1440 minutes");
    assert!(bad(&d, &|x| x.schedules[0].message = "x".repeat(4097)), "message ≤ 4 KB");
    assert!(bad(&d, &|x| x.schedules = (0..9).map(|i| s(&format!("s{i}"))).collect()), "at most 8");
}

#[test]
fn runs_and_teams_written_before_p4_still_load() {
    let d: TeamDraft = serde_json::from_value(json!({"id":"t","name":"T","groups":[],"agents":[],"connections":[],"revision":0})).unwrap();
    assert!(d.schedules.is_empty() && !d.decision_log);
    let out = serde_json::to_value(&d).unwrap();
    assert!(out.get("schedules").is_none() && out.get("decision_log").is_none());
    let s: WorkflowStage = serde_json::from_value(json!({"id":"a","name":"A","agent":"x","routes":{"done":null}})).unwrap();
    assert!(s.escalate.is_none());
}

fn escalating_state() -> AppState {
    let mut d = human_fixture();
    d.workflow.as_mut().unwrap().stages[0].escalate = Some(Escalation { after_rounds: 2, agent: "visualizer".into() });
    setup_draft(Store::open_in_memory().unwrap(), d)
}

/// Item 0 goes through `rounds` rounds at reference, each sent back by the person at chief.
async fn rounds_at_reference(st: &AppState, rounds: usize) {
    let start = current(st).start_seq;
    for _ in 0..rounds {
        finish(st, 0, "ready").await; scheduler::process(st, "book").await.unwrap();
        routes::decide_task(st, "book", "item-0", "rework", "", start, false).unwrap();
    }
}

#[tokio::test]
async fn an_item_goes_to_the_stronger_agent_after_the_configured_rounds() {
    let st = escalating_state(); dispatch(&st, 1).unwrap();
    rounds_at_reference(&st, 2).await;
    scheduler::process(&st, "book").await.unwrap();
    assert_eq!(current(&st).tasks[0].assigned_agent.as_deref(), Some("visualizer"), "round 3 is escalated");
    finish(&st, 0, "ready").await;
    let step = current(&st).tasks[0].history.iter().rev().find(|h| h.stage == "reference").cloned().unwrap();
    assert_eq!(step.agent, "visualizer");
    assert!(step.evidence.as_deref().unwrap_or_default().contains("escalated after 2 rounds"), "{:?}", step.evidence);
}

#[tokio::test]
async fn an_escalated_item_waits_for_its_stronger_agent() {
    let st = escalating_state(); dispatch(&st, 1).unwrap();
    rounds_at_reference(&st, 2).await;
    event(&st, "visualizer", EventBody::TurnStarted { turn_id: "busy".into() });
    scheduler::process(&st, "book").await.unwrap();
    let t = &current(&st).tasks[0];
    assert_eq!((t.status, t.assignment.is_none()), (TaskStatus::Queued, true), "never falls back to the weaker agent");
}

fn guarded_state(command: &str) -> AppState {
    let mut d = review_fixture(); d.folder = Some("/tmp".into());
    d.workflow.as_mut().unwrap().guard = Some(Guard { command: command.into(), every_minutes: 5, timeout_s: None });
    setup_draft(Store::open_in_memory().unwrap(), d)
}

/// Runs the scheduler until the guard has a result (it runs in the background).
async fn settle_guard(st: &AppState) -> GuardState {
    for _ in 0..150 {
        scheduler::process(st, "book").await.unwrap();
        if let Some(g) = current(st).guard { return g; }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the guard never ran");
}

fn set_guard(st: &AppState, f: impl FnOnce(&mut WorkflowRun)) {
    let mut store = st.store.lock().unwrap(); let mut run = store.latest_workflow("book").unwrap().unwrap();
    f(&mut run); store.save_workflow(&run, None).unwrap();
}

#[tokio::test]
async fn a_failing_guard_pauses_new_work_until_it_passes() {
    let st = guarded_state("echo broken; false"); dispatch(&st, 2).unwrap();
    let g = settle_guard(&st).await;
    assert!(!g.passed && g.evidence.contains("broken"), "{g:?}");
    // Whatever was already assigned finishes; nothing new goes out.
    let assigned_before: Vec<_> = current(&st).tasks.iter().map(|t| t.assignment.is_some()).collect();
    for (i, was) in assigned_before.iter().enumerate() { if *was { finish(&st, i, "ready").await; } }
    scheduler::process(&st, "book").await.unwrap();
    assert!(current(&st).tasks.iter().all(|t| t.status != TaskStatus::Assigned), "paused: {:?}", current(&st).tasks.iter().map(|t| t.status).collect::<Vec<_>>());
    // The project is fixed; the next guard run passes and work resumes.
    set_guard(&st, |r| { r.config.guard.as_mut().unwrap().command = "true".into(); r.guard.as_mut().unwrap().at_ms -= 6 * 60_000; });
    for _ in 0..150 { scheduler::process(&st, "book").await.unwrap(); if current(&st).guard.as_ref().is_some_and(|g| g.passed) { break; } tokio::time::sleep(std::time::Duration::from_millis(20)).await; }
    scheduler::process(&st, "book").await.unwrap();
    assert!(current(&st).tasks.iter().any(|t| t.status == TaskStatus::Assigned), "resumed");
}

#[tokio::test]
async fn a_person_can_resume_past_a_failing_guard_until_its_next_result() {
    let st = guarded_state("false"); dispatch(&st, 1).unwrap();
    // Let the guard fail before anything is assigned.
    set_guard(&st, |r| r.guard = Some(GuardState { passed: false, at_ms: model::now_ms(), evidence: "exit 1".into(), resumed_by_person: false }));
    scheduler::process(&st, "book").await.unwrap();
    assert!(current(&st).tasks[0].assignment.is_none());
    let start = current(&st).start_seq;
    routes::resume_guard(&st, "book", start).unwrap();
    assert!(routes::resume_guard(&st, "book", start).is_err(), "nothing to resume twice");
    scheduler::process(&st, "book").await.unwrap();
    assert!(current(&st).tasks[0].assignment.is_some());
}

#[tokio::test]
async fn a_merge_in_flight_finishes_while_the_guard_fails() {
    let (st, _repo) = merge_state(); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    set_guard(&st, |r| r.guard = Some(GuardState { passed: false, at_ms: model::now_ms(), evidence: "exit 1".into(), resumed_by_person: false }));
    settle_stage(&st, 0, "merge").await;
    assert_eq!(current(&st).tasks[0].history.last().unwrap().outcome, "merged");
}

fn scheduled_state() -> AppState {
    let mut d = review_fixture();
    d.schedules = vec![Schedule { id: "summary".into(), agent: "visualizer".into(), every_minutes: 30, message: "Summarise the board".into() }];
    setup_draft(Store::open_in_memory().unwrap(), d)
}

fn messages_to(st: &AppState, agent: &str) -> i64 {
    st.store.lock().unwrap().connection().query_row("SELECT COUNT(*) FROM messages WHERE seat=?1", [format!("team.{agent}@book")], |r| r.get(0)).unwrap()
}

/// The team started `minutes` ago, and the schedule last ran then.
fn backdate_schedule(st: &AppState, minutes: i64) {
    let at = model::now_ms() - minutes * 60_000;
    let store = st.store.lock().unwrap();
    store.connection().execute("UPDATE events SET json = json_set(json, '$.at_ms', ?1) WHERE json_extract(json,'$.body.type') = 'rig-booted'", [at - 1]).unwrap();
    store.connection().execute("UPDATE schedule_runs SET at_ms = ?1", [at]).unwrap();
    store.connection().execute("INSERT INTO schedule_runs(team, schedule, at_ms, outcome) VALUES ('book','summary',?1,'sent')", [at]).unwrap();
}

#[tokio::test]
async fn a_schedule_waits_one_interval_after_the_team_starts() {
    let st = scheduled_state();
    crate::schedules::tick(&st, "book").await;
    assert_eq!(messages_to(&st, "visualizer"), 0);
}

#[tokio::test]
async fn a_schedule_sends_when_due_and_skips_a_busy_agent() {
    let st = scheduled_state();
    backdate_schedule(&st, 31);
    crate::schedules::tick(&st, "book").await;
    assert_eq!(messages_to(&st, "visualizer"), 1, "due: the message was queued");
    crate::schedules::tick(&st, "book").await;
    assert_eq!(messages_to(&st, "visualizer"), 1, "not due again yet");
    backdate_schedule(&st, 31);
    event(&st, "visualizer", EventBody::TurnStarted { turn_id: "busy".into() });
    crate::schedules::tick(&st, "book").await;
    assert_eq!(messages_to(&st, "visualizer"), 1, "busy: nothing queued");
    let runs = st.store.lock().unwrap().schedule_runs("book", 10).unwrap();
    assert_eq!(runs[0].outcome, "skipped: busy");
    assert!(runs[1..].iter().any(|r| r.outcome.starts_with("not delivered") || r.outcome == "sent"), "the earlier run was sent: {runs:?}");
}

fn logged_state(on: bool) -> AppState {
    let mut d = human_fixture(); d.decision_log = on;
    setup_draft(Store::open_in_memory().unwrap(), d)
}

fn read_log(st: &AppState, args: serde_json::Value) -> String {
    crate::decisions::read_tool(st, "team.reference@book", &args).unwrap()["text"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn people_write_the_decision_log_and_agents_read_it() {
    let st = logged_state(true);
    let first = crate::decisions::add(&st, "book", "Prefer small fixes over refactors.", "person").unwrap();
    crate::decisions::add(&st, "book", "Never touch the payments code without sign-off.", "person").unwrap();
    let all = read_log(&st, json!({}));
    assert!(all.find("payments").unwrap() < all.find("small fixes").unwrap(), "newest first: {all}");
    let found = read_log(&st, json!({"query": "PAYMENTS"}));
    assert!(found.contains("payments") && !found.contains("small fixes"));
    crate::decisions::hide(&st, "book", &first).unwrap();
    assert!(!read_log(&st, json!({})).contains("small fixes"), "hidden entries aren't read");
    let kept = st.store.lock().unwrap().decisions("book", None, 50, true).unwrap();
    assert!(kept.iter().any(|e| e.id == first && e.hidden_ms.is_some()), "hiding is recorded, nothing is deleted");
    let note = crate::team_context::decision_log_note(&st.store.lock().unwrap(), "book", true);
    assert!(note.unwrap().contains("1 entr"));
}

#[tokio::test]
async fn a_turned_off_log_keeps_its_entries_but_agents_are_told_it_is_off() {
    let st = logged_state(false);
    crate::decisions::add(&st, "book", "Kept for later.", "person").unwrap();
    assert!(read_log(&st, json!({})).contains("off"));
    assert_eq!(st.store.lock().unwrap().decisions("book", None, 50, false).unwrap().len(), 1);
    assert!(crate::team_context::decision_log_note(&st.store.lock().unwrap(), "book", false).is_none());
}

#[tokio::test]
async fn a_persons_decision_can_become_a_precedent() {
    let st = logged_state(true); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let start = current(&st).start_seq;
    routes::decide_task(&st, "book", "item-0", "ship", "Ship small UI fixes without a second review.", start, true).unwrap();
    let entries = st.store.lock().unwrap().decisions("book", None, 50, false).unwrap();
    assert_eq!((entries.len(), entries[0].source.as_str()), (1, "decision"));
    assert!(entries[0].text.contains("Ship small UI fixes"));
}

#[tokio::test]
async fn a_scheduled_message_is_recorded_as_the_schedules_not_a_persons() {
    let st = scheduled_state();
    backdate_schedule(&st, 31);
    crate::schedules::tick(&st, "book").await;
    let ev = st.store.lock().unwrap().latest_of_types(&Subject::Seat("team.visualizer@book".into()), &["message-queued"], 1).unwrap();
    assert_eq!(ev[0].actor, Actor::System("schedule:summary".into()));
}

#[tokio::test]
async fn two_schedules_for_one_agent_never_queue_two_messages() {
    let mut d = review_fixture();
    d.schedules = ["a", "b"].iter().map(|id| Schedule { id: (*id).into(), agent: "visualizer".into(), every_minutes: 30, message: format!("from {id}") }).collect();
    let st = setup_draft(Store::open_in_memory().unwrap(), d);
    { let at = model::now_ms() - 31 * 60_000; let store = st.store.lock().unwrap();
      store.connection().execute("UPDATE events SET json = json_set(json, '$.at_ms', ?1) WHERE json_extract(json,'$.body.type') = 'rig-booted'", [at]).unwrap(); }
    crate::schedules::tick(&st, "book").await;
    assert_eq!(messages_to(&st, "visualizer"), 1);
    let runs = st.store.lock().unwrap().schedule_runs("book", 10).unwrap();
    assert!(runs.iter().any(|r| r.outcome == "skipped: busy"), "{runs:?}");
}

#[tokio::test]
async fn a_schedule_to_a_stopped_agent_says_so() {
    let st = scheduled_state();
    backdate_schedule(&st, 31);
    event(&st, "visualizer", EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Stopped, reason: None });
    crate::schedules::tick(&st, "book").await;
    assert_eq!(st.store.lock().unwrap().schedule_runs("book", 1).unwrap()[0].outcome, "skipped: agent not running");
}

#[tokio::test]
async fn the_guard_has_its_own_time_limit_and_rests_once_the_batch_is_done() {
    let st = guarded_state("sleep 5");
    dispatch(&st, 1).unwrap();
    set_guard(&st, |r| r.config.guard.as_mut().unwrap().timeout_s = Some(1));
    let g = settle_guard(&st).await;
    assert!(g.evidence.contains("timed out"), "{}", g.evidence);
    set_guard(&st, |r| { r.assembly.status = TaskStatus::Complete; r.guard.as_mut().unwrap().at_ms -= 60 * 60_000; });
    let before = current(&st).guard.unwrap().at_ms;
    for _ in 0..5 { scheduler::process(&st, "book").await.unwrap(); tokio::time::sleep(std::time::Duration::from_millis(20)).await; }
    assert_eq!(current(&st).guard.unwrap().at_ms, before, "no guard runs once everything is done");
}

#[tokio::test]
async fn a_logged_decision_stays_within_the_entry_limit() {
    let st = logged_state(true); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let start = current(&st).start_seq;
    routes::decide_task(&st, "book", "item-0", "ship", &"x".repeat(4096), start, true).unwrap();
    assert!(st.store.lock().unwrap().decisions("book", None, 1, false).unwrap()[0].text.len() <= 4096);
}

#[tokio::test]
async fn a_panel_reviewer_is_told_its_angle_and_a_split_vote_is_named() {
    let mut d = panel_fixture();
    let reviewers = d.workflow.as_ref().unwrap().stages[1].reviewers.clone();
    d.workflow.as_mut().unwrap().stages[1].personas.insert(reviewers[0].clone(), ReviewPersona::RedTeam);
    let st = setup_draft(Store::open_in_memory().unwrap(), d.clone()); dispatch(&st, 1).unwrap();
    finish(&st, 0, "ready").await; scheduler::process(&st, "book").await.unwrap();
    let run = current(&st);
    let text = |agent: &str| assignment::text(&d, &run, &run.tasks[0], "a", agent);
    assert!(text(&reviewers[0]).contains("Review as the red team: Look for ways to break it"), "{}", text(&reviewers[0]));
    assert!(!text(&reviewers[1]).contains("Review as the"), "the other reviewer keeps its own judgement");
    vote(&st, &reviewers[0], "changes").await.unwrap();
    vote(&st, &reviewers[1], "approved").await.unwrap();
    let step = current(&st).tasks[0].history.last().unwrap().clone();
    assert!(step.result.starts_with(&format!("Reviewers disagree: {} voted approved; {} (red team) voted changes.", reviewers[1], reviewers[0])), "{}", step.result);
}
