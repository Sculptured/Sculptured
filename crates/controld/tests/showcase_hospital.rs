//! The coding-hospital showcase team (`docs/showcase/coding-hospital.madl.json`) is a valid MADL
//! document that Sculptured reads with nothing dropped: it passes every draft check, and the import check finds no
//! issue once its project folder exists. Nothing here starts the team or calls a model.
mod common;
use axum::http::StatusCode;

use controld::portable;
use model::{CopiesMode, StageKind};
use serde_json::{Value, json};

fn file() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/showcase/coding-hospital.madl.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).expect("the team file")).expect("JSON")
}

#[test]
fn the_hospital_team_file_is_valid_madl_and_passes_the_draft_checks() {
    let parsed = portable::madl::read(&file().to_string()).expect("a valid MADL document");
    assert!(parsed.dropped.is_empty(), "{:?}", parsed.dropped);
    let parsed = parsed.file;
    let build = rigspec::from_draft(&parsed.team)
        .unwrap_or_else(|issues| panic!("draft errors: {issues:#?}"));
    assert!(build.warnings.is_empty(), "{:#?}", build.warnings);

    let d = &parsed.team;
    let w = d.workflow.as_ref().unwrap();
    let ids: Vec<_> = w.stages.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "admit",
            "workup",
            "plan-review",
            "treatment",
            "tests",
            "code-review",
            "sign-off",
            "merge",
            "escalation"
        ]
    );
    let kind = |id: &str| w.stages.iter().find(|s| s.id == id).unwrap();
    assert_eq!(
        [
            kind("admit").kind,
            kind("tests").kind,
            kind("merge").kind,
            kind("escalation").kind
        ],
        [
            StageKind::Human,
            StageKind::Check,
            StageKind::Merge,
            StageKind::Human
        ]
    );
    let t = kind("treatment");
    assert_eq!(
        (
            t.max_rounds,
            t.on_limit.as_deref(),
            t.escalate
                .as_ref()
                .map(|e| (e.after_rounds, e.agent.as_str()))
        ),
        (Some(4), Some("escalation"), Some((2, "senior-fellow")))
    );
    assert_eq!(
        kind("code-review").reviewers,
        ["correctness-reviewer", "safety-reviewer"]
    );
    assert_eq!(
        (w.dispatcher.as_str(), w.collector.as_str(), w.copies),
        ("triage-nurse", "discharge-nurse", CopiesMode::PerItem)
    );
    assert_eq!(
        w.guard
            .as_ref()
            .map(|g| (g.command.as_str(), g.every_minutes)),
        Some(("npm test", 15))
    );
    assert_eq!(
        d.schedules
            .iter()
            .map(|s| (s.agent.as_str(), s.every_minutes))
            .collect::<Vec<_>>(),
        [("charge-nurse", 30)]
    );
    assert!(d.decision_log);
    assert!(
        d.agents
            .iter()
            .all(|a| a.advanced.model.is_some() && a.advanced.token_limit.is_some()),
        "every agent names its model and a token limit"
    );
}

#[tokio::test]
async fn importing_the_hospital_team_file_finds_no_issue_once_its_folder_exists() {
    let s = crate::common::m2::serve().await;
    let mut f = file();
    let folder = s.home.path().join("sculptured-hospital");
    std::fs::create_dir_all(&folder).unwrap();
    f["defaults"]["preferences"]["workspace"] = json!({ "folder": folder.display().to_string() });
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams/check-madl",
        json!({ "text": f.to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["issues"], json!([]), "{v}");
}
