//! Where a run's tokens went: MAIS over the session logs of fake-claude agents (no model is called).
mod common;
use axum::http::StatusCode;
use common::fake_bin;

use common::m2::{Served, rid};
use drivers::claude::ClaudeStreamDriver;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

async fn team(s: &Served, dir: &std::path::Path, job: &str) -> String {
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Report" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([{ "id": "a", "group_id": null, "name": "Alpha", "kind": "claude", "job": job, "folder": dir.display().to_string(), "x": 40, "y": 40 }]);
    draft["connections"] = json!([]);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

/// The report once it holds `calls` model calls.
async fn report_with(s: &Served, id: &str, calls: u64) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let (status, body) =
            crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
        if status == StatusCode::OK && body["report"]["run"]["calls"].as_u64() >= Some(calls) {
            return body;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no report with {calls} calls: {status} {body}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_runs_report_names_its_agents_and_counts_every_call() {
    let s = crate::common::m2::serve().await;
    // SAFETY: this test binary has one test; the variable is set before any agent starts.
    unsafe {
        std::env::set_var(
            "FAKE_CLAUDE_PROJECTS",
            s.home.path().join(".claude/projects/-work"),
        )
    };
    let _runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), "spend:3:1000:10").await;
    let (status, body) =
        crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no run yet: {body}");
    crate::common::m2::start_team(&s, &id).await;
    let body = report_with(&s, &id, 4).await;
    let r = &body["report"];
    assert_eq!(r["format"], "mais.report/v1");
    assert_eq!(
        r["agents"][0]["name"], "Alpha",
        "the session is linked to its team member"
    );
    assert_eq!(
        r["run"]["usage"]["cache_read"], 4000,
        "three spend calls and the last reply each read 1000"
    );
    assert_eq!(
        body["start_seq"].as_i64(),
        s.st.store.lock().unwrap().latest_run_start(&id).unwrap()
    );
    let kept: i64 =
        s.st.store
            .lock()
            .unwrap()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM run_reports WHERE rig=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(kept, 1, "the report is kept for the run");
    assert_eq!(body["running"], true);
    // A budget's hard stop ends every agent without a Stop of the team (its desired state stays running).
    s.st.store
        .lock()
        .unwrap()
        .connection()
        .execute("UPDATE seats SET lifecycle='stopped' WHERE rig=?1", [&id])
        .unwrap();
    let (_, done) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
    assert_eq!(
        done["running"], false,
        "no agent is left, though nobody pressed Stop: {done}"
    );
    let (_, again) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
    assert_eq!(
        again["at_ms"], done["at_ms"],
        "a finished run's report is kept, not made again"
    );
}
