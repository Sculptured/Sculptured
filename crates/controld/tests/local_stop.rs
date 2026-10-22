//! The runner keeps each agent's process on record while it runs, for `sculptured stop`.
mod common;
use common::fake_bin;

use common::m2::rid;
use drivers::claude::ClaudeStreamDriver;
use std::sync::Arc;

#[tokio::test]
async fn a_running_agent_is_on_record_and_its_record_goes_when_it_stops() {
    let s = crate::common::m2::serve().await;
    let runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        serde_json::json!({ "request_id": rid(), "template": "blank", "name": "Rec" }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let mut d = rec["draft"].clone();
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    d["agents"] = serde_json::json!([{ "id": "a", "group_id": null, "name": "Alpha", "kind": "claude", "job": "hello", "folder": dir.path().display().to_string(), "x": 40, "y": 40 }]);
    d["connections"] = serde_json::json!([]);
    let team = d["id"].as_str().unwrap().to_string();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{team}"),
        serde_json::json!({ "request_id": rid(), "revision": d["revision"], "draft": d }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{body}");
    crate::common::m2::start_team(&s, &team).await;
    let a = crate::common::m2::seat(&team, "a");
    common::wait_for("the agent on record", || {
        runner
            .recorded_processes()
            .iter()
            .any(|(s, pid, ident)| s == &a && *pid > 0 && ident.is_some())
    })
    .await;
    crate::common::m2::stop_team(&s, &team).await;
    common::wait_for("its record gone", || {
        !runner.recorded_processes().iter().any(|(s, ..)| s == &a)
    })
    .await;
}
