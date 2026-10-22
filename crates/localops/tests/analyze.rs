//! `analyze-run`: a team run's report from this computer's session files (hand-written fixtures).
use localops::analyze::RunSession;
use localops::{Context, LocalOp};
use std::path::Path;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    let mapped_items = std::fs::read_dir(from).unwrap().map(Result::unwrap);
    for e in mapped_items {
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy(&e.path(), &target)
        } else {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

fn computer() -> (tempfile::TempDir, Context) {
    let home = tempfile::tempdir().unwrap();
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/analyze"),
        &home.path().join(".claude/projects/-work-c172"),
    );
    let cx = Context {
        home: home.path().into(),
        copies_root: home.path().join("copies"),
        claude: "/no".into(),
        codex: "/no".into(),
        codex_home: home.path().join(".codex"),
    };
    (home, cx)
}

const EXPORT: &str = concat!(
    r#"{"format":"sculptured.session/v1","source":"sculptured","team_id":"c172","started_ms":1792090000000,"ended_ms":null,"agents":[],"tokens":{"input":0,"output":0,"cache_read":0,"cache_write":0},"#,
    r#""team":{"format":"sculptured.team/v1","team":{"id":"c172","name":"C172 Model","groups":[{"id":"fuel","label":"Fuel Systems"}],"agents":[{"id":"dev8","name":"Developer 8","group_id":"fuel"}],"connections":[]}}}"#,
    "\n",
    r#"{"origin":"observed","event":{"seq":1,"at_ms":1792090001000,"subject":{"kind":"seat","id":"fuel.dev8@c172"},"body":{"type":"tenure-started","tenure":1,"cause":"fresh","native_session_id":"s1"}}}"#,
    "\n",
);

fn op(ids: &[&str]) -> LocalOp {
    LocalOp::AnalyzeRun {
        sessions: ids
            .iter()
            .map(|id| RunSession {
                runtime: "claude".into(),
                id: (*id).into(),
            })
            .collect(),
        export: EXPORT.into(),
    }
}

#[tokio::test]
async fn a_runs_sessions_on_this_computer_become_its_report() {
    let (_home, cx) = computer();
    let r = localops::execute(&cx, op(&["s1"])).await.unwrap();
    assert_eq!(r["format"], "mais.report/v1");
    assert_eq!(
        (
            r["agents"][0]["name"].as_str(),
            r["agents"][0]["group"].as_str()
        ),
        (Some("Developer 8"), Some("Fuel Systems"))
    );
    assert_eq!(r["run"]["agents"], 2, "its subagent too");
}

#[tokio::test]
async fn sessions_this_computer_does_not_have_are_a_plain_not_found() {
    let (_home, cx) = computer();
    let e = localops::execute(&cx, op(&["elsewhere", "../s1"]))
        .await
        .unwrap_err();
    assert_eq!((e.status, e.code.as_str()), (404, "no_logs"));
}

#[test]
fn the_operation_crosses_the_wire_as_plain_json() {
    let wire: LocalOp = serde_json::from_str(
        r#"{"op":"analyze-run","sessions":[{"runtime":"codex","id":"019a"}],"export":""}"#,
    )
    .unwrap();
    assert_eq!(
        wire,
        LocalOp::AnalyzeRun {
            sessions: vec![RunSession {
                runtime: "codex".into(),
                id: "019a".into()
            }],
            export: String::new()
        }
    );
}
