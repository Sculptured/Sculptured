//! The team briefing an agent gets: its own job and its neighbours' in full, the rest of the team by
//! name, once per conversation. Briefings are checked live, on the real Claude Code CLI with its
//! cheapest model; launch settings are read from a started team with no computer connected.
mod common;
use axum::http::StatusCode;

use common::m2::{Served, rid};
use model::EventBody;
use serde_json::json;

const OK: &str = "reply with the single word OK and nothing else.";

/// A, B and C (Claude agents on haiku), A handing work to B; not started.
async fn team(s: &Served) -> String {
    let (a, b, c) = (
        format!("JOB-OF-A: {OK}"),
        format!("JOB-OF-B: {OK}"),
        format!("JOB-OF-C: {OK}"),
    );
    crate::common::m2::chain(
        s,
        "Brief",
        &[
            ("a", "A", a.as_str()),
            ("b", "B", b.as_str()),
            ("c", "C", c.as_str()),
        ],
        &[("a", "b")],
    )
    .await
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
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_agent_is_briefed_once_with_its_neighbours_jobs_and_the_rest_by_name() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let id = team(&s).await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("A's job", || !queued(&s, &a).is_empty()).await;
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
    crate::common::m2::say(&s, &a, &format!("second message: {OK}")).await;
    common::live::wait_for("the second message", || queued(&s, &a).len() >= 2).await;
    let second = &queued(&s, &a)[1];
    assert!(
        !second.contains("Team architecture"),
        "briefed once per conversation: {second}"
    );
    assert!(second.contains("second message"));
    crate::common::m2::stop_team(&s, &id).await;
}

/// Starts team `id` with no computer connected: its compiled agents are recorded, none launches.
async fn start_without_computer(s: &Served, id: &str) {
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::wait_for("boot finished", || !s.st.is_booting(id)).await;
}

#[tokio::test]
async fn a_team_without_a_workflow_launches_without_the_workflow_tools() {
    let s = crate::common::m2::serve().await;
    let id = team(&s).await;
    start_without_computer(&s, &id).await;
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
