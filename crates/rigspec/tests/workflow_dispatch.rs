//! A workflow's batch comes from a dispatcher agent or a dispatch command: exactly one of them.
use model::{TeamDraft, Workflow, WorkflowDispatch};
use rigspec::validate_draft;

fn textbook() -> TeamDraft {
    let mut d = rigspec::textbook::template("book", "Book", true);
    d.folder = Some("/tmp".into());
    d
}

fn errors(change: impl FnOnce(&mut TeamDraft, &mut Workflow)) -> Vec<String> {
    let mut d = textbook();
    let mut w = d.workflow.take().unwrap();
    change(&mut d, &mut w);
    d.workflow = Some(w);
    validate_draft(&d).into_iter().filter(|i| i.code == "workflow-invalid").map(|i| i.message).collect()
}

fn command(text: &str) -> Option<WorkflowDispatch> {
    Some(WorkflowDispatch { command: text.into(), timeout_s: None })
}

#[test]
fn a_dispatcher_agent_alone_is_still_valid() {
    assert!(errors(|_, _| {}).is_empty());
    assert!(!errors(|_, w| w.dispatcher = String::new()).is_empty(), "no dispatcher and no command");
}

#[test]
fn a_dispatch_command_replaces_the_dispatcher_agent() {
    assert!(errors(|_, w| { w.dispatcher = String::new(); w.dispatch = command("cat tasks.json"); }).is_empty());
    let both = errors(|_, w| w.dispatch = command("cat tasks.json"));
    assert!(both.iter().any(|e| e.contains("not both")), "{both:?}");
}

#[test]
fn a_dispatch_command_needs_text_a_time_limit_in_range_a_folder_and_an_assembly_agent() {
    let only = |change: fn(&mut TeamDraft, &mut Workflow)| errors(move |d, w| { w.dispatcher = String::new(); w.dispatch = command("cat tasks.json"); change(d, w); });
    assert!(only(|_, w| w.dispatch.as_mut().unwrap().command = "  ".into()).iter().any(|e| e.contains("1–2000 bytes")));
    assert!(only(|_, w| w.dispatch.as_mut().unwrap().command = "x".repeat(2001)).iter().any(|e| e.contains("1–2000 bytes")));
    assert!(only(|_, w| w.dispatch.as_mut().unwrap().timeout_s = Some(0)).iter().any(|e| e.contains("1–3600")));
    assert!(only(|_, w| w.dispatch.as_mut().unwrap().timeout_s = Some(3600)).is_empty());
    assert!(!only(|d, _| d.folder = None).iter().any(|e| e.contains("project folder")), "a missing folder is its own issue (project-folder-needed)");
    assert!(only(|_, w| w.collector = "missing".into()).iter().any(|e| e.contains("assembly")));
}

#[test]
fn the_dispatch_command_round_trips_through_the_team_file_and_older_files_still_load() {
    let mut d = textbook();
    let w = d.workflow.as_mut().unwrap();
    w.dispatcher = String::new();
    w.dispatch = Some(WorkflowDispatch { command: "cat tasks.json".into(), timeout_s: Some(60) });
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["workflow"]["dispatch"], serde_json::json!({"command": "cat tasks.json", "timeout_s": 60}));
    assert_eq!(serde_json::from_value::<TeamDraft>(json).unwrap(), d);
    let mut old = serde_json::to_value(textbook().workflow.unwrap()).unwrap();
    assert!(old.get("dispatch").is_none(), "no dispatch command: written as before");
    old.as_object_mut().unwrap().remove("dispatcher");
    assert_eq!(serde_json::from_value::<Workflow>(old).unwrap().dispatcher, "", "a missing dispatcher reads as none");
}

#[test]
fn a_missing_project_folder_is_one_issue_naming_everything_that_runs_there() {
    let mut d = textbook();
    d.folder = None;
    let w = d.workflow.as_mut().unwrap();
    w.copies = model::CopiesMode::PerItem;
    w.guard = Some(model::Guard { command: "npm test".into(), every_minutes: 15, timeout_s: None });
    w.dispatcher = String::new();
    w.dispatch = command("cat tasks.json");
    let issues = validate_draft(&d);
    let folder: Vec<_> = issues.iter().filter(|i| i.code == "project-folder-needed").collect();
    assert_eq!(folder.len(), 1, "{issues:#?}");
    let m = &folder[0].message;
    assert!(m.starts_with("Choose the project folder") && m.contains("a copy per item") && m.contains("the guard") && m.contains("the dispatch command"), "{m}");
    assert!(!issues.iter().any(|i| i.code == "workflow-invalid" && i.message.contains("project folder")), "{issues:#?}");
}
