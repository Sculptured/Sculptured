//! A workflow whose batch comes from a dispatch command Sculptured runs (no dispatcher agent).

use super::{dispatch, scheduler};
use crate::state::AppState;
use crate::store::Store;
use model::{BriefingLevel, DispatchRecord, TaskStatus, TeamDraft, WorkflowDispatch};
use serde_json::json;

/// The review fixture with its batch from `command`, run in a folder holding `tasks.json`.
fn with_command(command: &str, tasks: serde_json::Value) -> (TeamDraft, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("tasks.json"), tasks.to_string()).unwrap();
    let mut d = crate::workflow::tests::review_fixture();
    d.folder = Some(dir.path().display().to_string());
    let w = d.workflow.as_mut().unwrap();
    w.dispatcher = String::new();
    w.dispatch = Some(WorkflowDispatch {
        command: command.into(),
        timeout_s: Some(5),
    });
    (d, dir)
}

fn batch(n: usize) -> serde_json::Value {
    json!({"summary": "Shared brief", "tasks": (0..n).map(|i| json!({"id": format!("t{i}"), "title": format!("Topic {i}"), "context": "Section 1"})).collect::<Vec<_>>()})
}

fn start(st: &AppState) -> i64 {
    crate::workflow::running(&st.store.lock().unwrap(), "book")
        .unwrap()
        .1
}

fn record(st: &AppState) -> Option<DispatchRecord> {
    let at = start(st);
    st.store
        .lock()
        .unwrap()
        .dispatch_record("book", at)
        .unwrap()
}

fn batch_now(st: &AppState) -> Option<model::WorkflowRun> {
    let at = start(st);
    st.store.lock().unwrap().workflow_run("book", at).unwrap()
}

/// Runs the scheduler until the dispatch command has a result.
async fn settle(st: &AppState) -> DispatchRecord {
    for _ in 0..300 {
        scheduler::process(st, "book").await.unwrap();
        if let Some(r) = record(st) {
            return r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the dispatch command never finished");
}

#[tokio::test]
async fn the_dispatch_commands_output_becomes_the_batch_with_sculptured_as_its_source() {
    let (d, _dir) = with_command("cat tasks.json", batch(3));
    assert!(
        rigspec::from_draft(&d).is_ok(),
        "{:?}",
        rigspec::from_draft(&d).err()
    );
    let st = crate::workflow::tests::setup_draft(Store::open_in_memory().unwrap(), d);
    let r = settle(&st).await;
    assert!(r.dispatched && r.problem.is_none(), "{r:?}");
    assert!(
        r.evidence.starts_with("$ cat tasks.json\nexit 0")
            && r.evidence.contains("3 items dispatched"),
        "{}",
        r.evidence
    );
    let run = batch_now(&st).expect("the batch");
    assert_eq!(
        run.batch_id,
        format!("batch-{}", start(&st)),
        "a default batch id"
    );
    assert_eq!(
        (
            run.summary.as_str(),
            run.tasks
                .iter()
                .map(|t| t.input.id.as_str())
                .collect::<Vec<_>>()
        ),
        ("Shared brief", vec!["t0", "t1", "t2"])
    );
    scheduler::process(&st, "book").await.unwrap();
    assert_eq!(
        batch_now(&st).unwrap().tasks[0].assigned_agent.as_deref(),
        Some("reference"),
        "work goes out as usual"
    );
    let refused = crate::workflow::call(
        &st,
        "team.dispatcher@book",
        "dispatch_work",
        &json!({"batch_id": "x", "summary": "x", "tasks": []}),
    )
    .unwrap_err();
    assert!(refused.contains("dispatch command"), "{refused}");
    for _ in 0..3 {
        scheduler::process(&st, "book").await.unwrap();
    }
    assert_eq!(
        record(&st).unwrap().at_ms,
        r.at_ms,
        "it runs once per start"
    );
}

#[tokio::test]
async fn a_failing_dispatch_command_dispatches_nothing_and_says_why() {
    let dependent = json!({"summary": "s", "tasks": [{"id": "a", "title": "A", "context": "c", "mode": "dependent"}]});
    let cases = [
        ("echo partial; exit 3", batch(1), "exit 3"),
        ("echo 'not json'", batch(1), "not a batch"),
        (
            "cat tasks.json",
            json!({"summary": "s", "tasks": [], "extra": 1}),
            "not a batch",
        ),
        ("cat tasks.json", dependent, "refused"),
        ("cat tasks.json", batch(25), "refused: Dispatch 1–24 tasks."),
        ("sleep 30", batch(1), "time limit"),
    ];
    for (command, tasks, why) in cases {
        let (mut d, _dir) = with_command(command, tasks);
        d.workflow
            .as_mut()
            .unwrap()
            .dispatch
            .as_mut()
            .unwrap()
            .timeout_s = Some(1);
        let st = crate::workflow::tests::setup_draft(Store::open_in_memory().unwrap(), d);
        let r = settle(&st).await;
        assert!(
            !r.dispatched
                && r.problem
                    .as_deref()
                    .is_some_and(|p| p.contains(why) && p.contains("othing was dispatched")),
            "{command}: {r:?}"
        );
        assert!(
            r.evidence.starts_with(&format!("$ {command}")),
            "{}",
            r.evidence
        );
        assert!(batch_now(&st).is_none(), "{command}: nothing dispatched");
    }
}

#[tokio::test]
async fn a_failed_dispatch_command_runs_again_only_when_a_person_asks() {
    let (d, dir) = with_command("cat tasks.json", json!({"summary": "s"}));
    let st = crate::workflow::tests::setup_draft(Store::open_in_memory().unwrap(), d);
    assert!(!settle(&st).await.dispatched);
    std::fs::write(dir.path().join("tasks.json"), batch(2).to_string()).unwrap();
    for _ in 0..3 {
        scheduler::process(&st, "book").await.unwrap();
    }
    assert!(batch_now(&st).is_none(), "not on its own");
    assert!(
        dispatch::retry(&st, "book", start(&st) - 1).is_err(),
        "an earlier run"
    );
    dispatch::retry(&st, "book", start(&st)).unwrap();
    assert!(settle(&st).await.dispatched);
    assert_eq!(batch_now(&st).unwrap().tasks.len(), 2);
    assert!(
        dispatch::retry(&st, "book", start(&st)).is_err(),
        "it dispatched: nothing to run again"
    );
}

#[tokio::test]
async fn a_dispatch_command_where_the_project_folder_is_missing_says_so() {
    let (mut d, _dir) = with_command("cat tasks.json", batch(1));
    d.folder = Some("/nonexistent/sculptured-dispatch-folder".into());
    let st = crate::workflow::tests::setup_draft(Store::open_in_memory().unwrap(), d);
    let r = settle(&st).await;
    assert!(
        !r.dispatched && r.problem.as_deref().unwrap().contains("does not exist"),
        "{r:?}"
    );
}

#[tokio::test]
async fn a_minimal_worker_gets_its_assignment_without_the_team_briefing() {
    let (mut d, _dir) = with_command("cat tasks.json", batch(1));
    let me = d.agents.iter_mut().find(|a| a.id == "reference").unwrap();
    me.kind = model::AgentKind::Claude;
    me.advanced.briefing = Some(BriefingLevel::Minimal);
    let (name, job) = (me.name.clone(), me.job.clone());
    let others: Vec<String> = d
        .agents
        .iter()
        .filter(|a| a.id != "reference")
        .map(|a| a.job.clone())
        .collect();
    let st = crate::workflow::tests::setup_draft(Store::open_in_memory().unwrap(), d);
    settle(&st).await;
    scheduler::process(&st, "book").await.unwrap();
    let id = batch_now(&st).unwrap().tasks[0]
        .assignment
        .clone()
        .expect("assigned");
    let text: String = st
        .store
        .lock()
        .unwrap()
        .connection()
        .query_row("SELECT text FROM messages WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    assert!(
        text.contains(&format!("Your role: {name} [reference]")) && text.contains(job.trim()),
        "{text}"
    );
    assert!(
        text.contains("complete_work") && text.contains(&format!("Assignment: {id}")),
        "the assignment itself is whole"
    );
    for absent in [
        "Team architecture",
        "This project uses a dynamic workflow",
        "Configured stages",
        "Work efficiently",
    ] {
        assert!(!text.contains(absent), "{absent:?} in {text}");
    }
    assert!(
        others.iter().all(|j| !text.contains(j.trim())),
        "no neighbour's job"
    );
    assert_eq!(text.matches(job.trim()).count(), 1, "the job once");
    assert_eq!(
        batch_now(&st).unwrap().tasks[0].status,
        TaskStatus::Assigned
    );
}
