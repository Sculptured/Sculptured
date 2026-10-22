//! The team briefing an agent gets: its own job and its neighbours' in full, the rest of the team by
//! name, once per conversation (fake-claude agents; no model is called).
mod common;
use axum::http::StatusCode;
use common::fake_bin;

use common::m2::{Served, rid};
use drivers::claude::ClaudeStreamDriver;
use model::EventBody;
use serde_json::json;
use std::sync::Arc;

async fn team(s: &Served, dir: &std::path::Path) -> String {
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Brief" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    let agent = |id: &str, job: &str, x: i32| json!({ "id": id, "group_id": null, "name": id.to_uppercase(), "kind": "claude", "job": job, "folder": dir.display().to_string(), "x": x, "y": 40 });
    draft["agents"] = json!([
        agent("a", "JOB-OF-A plan the work", 40),
        agent("b", "JOB-OF-B build what A plans", 300),
        agent("c", "JOB-OF-C unrelated research", 560)
    ]);
    draft["connections"] = json!([{ "id": "ab", "from": "a", "to": "b", "kind": "hands-work-to" }]);
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

fn queued(s: &Served, seat: &str) -> Vec<String> {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::MessageQueued { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn an_agent_is_briefed_once_with_its_neighbours_jobs_and_the_rest_by_name() {
    let s = crate::common::m2::serve().await;
    let _runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::wait_for("A's job", || !queued(&s, &a).is_empty()).await;
    let first = &queued(&s, &a)[0];
    assert!(
        first.contains("Team architecture (operator configuration)")
            && first.contains("Your role: A [a]"),
        "{first}"
    );
    assert!(first.contains("JOB-OF-B"), "a neighbour's job in full");
    assert!(
        !first.contains("JOB-OF-C") && first.contains("C [c]"),
        "the rest of the team by name only: {first}"
    );
    crate::common::m2::say(&s, &a, "second message").await;
    common::wait_for("the second message", || queued(&s, &a).len() >= 2).await;
    let second = &queued(&s, &a)[1];
    assert!(
        !second.contains("Team architecture"),
        "briefed once per conversation: {second}"
    );
    assert!(second.contains("second message"));
}

#[tokio::test]
async fn a_team_without_a_workflow_launches_without_the_workflow_tools() {
    let s = crate::common::m2::serve().await;
    let _runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    crate::common::m2::start_team(&s, &id).await;
    let snap =
        s.st.store
            .lock()
            .unwrap()
            .rig_snapshot(&id)
            .unwrap()
            .expect("compiled");
    let spec = snap
        .rig
        .seats
        .iter()
        .find(|x| x.id == crate::common::m2::seat(&id, "a"))
        .unwrap();
    let params =
        controld::seat_tools::launch_params(&s.st, spec, model::PermissionPosture::Unrestricted)
            .unwrap();
    assert!(
        !params.workflow,
        "complete_work and its siblings would only be refused"
    );
}
