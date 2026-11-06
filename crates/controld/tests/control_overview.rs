//! Tests: the organisation overview (`GET /api/control/overview`) shows every team as a unit with its
//! agents by state, its tool calls per minute and latest call, and its latest run's usage. Stub agents only.
mod common;
use serde_json::Value;

fn unit<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["teams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id)
        .unwrap()
}

#[tokio::test]
async fn every_team_is_a_unit_with_its_state_and_key_figures() {
    let s = common::m2::serve().await;
    common::m2::connect_stub(&s).await;
    let idle = common::m2::practice_chain(&s, "Not started", &[("a", "A", "")], &[]).await;
    let id = common::m2::practice_team(&s, "solo").await;
    let agent = common::m2::seat(&id, "claude");
    common::m2::say(&s, &agent, "tool:echo checked").await;
    common::wait_for("the call is recorded", || {
        let v = controld::control_overview::read(&s.st, model::now_ms()).unwrap();
        v.teams
            .iter()
            .any(|t| t.id == id && t.last_action.is_some())
    })
    .await;

    let (status, v) = common::m2::get(&s.router, "/api/control/overview").await;
    assert_eq!(status.as_u16(), 200, "{v}");
    running_unit(unit(&v, &id), &agent);
    never_started_unit(unit(&v, &idle));
}

/// A running team: its agent counted as running, one call in its last minutes, that call as its latest.
fn running_unit(running: &Value, agent: &str) {
    assert_eq!(running["started"], true);
    assert_eq!(running["desired"], "running");
    assert_eq!(
        (running["agents"].as_u64(), running["running"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(running["actions_per_min"].as_array().unwrap().len(), 15);
    assert_eq!(
        running["actions_per_min"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap())
            .sum::<u64>(),
        1
    );
    assert_eq!(running["last_action"]["seat"], agent);
    assert_eq!(running["last_action"]["tool"], "Bash");
    assert_eq!(running["failed"], 0);
}

/// A design never started: its agents counted from the design, nothing else known.
fn never_started_unit(never: &Value) {
    assert_eq!(
        (never["started"].as_bool(), never["desired"].is_null()),
        (Some(false), true)
    );
    assert_eq!(
        (never["agents"].as_u64(), never["last_action"].is_null()),
        (Some(1), true)
    );
    assert_eq!(
        (
            never["run_tokens"].as_u64(),
            never["run_cost_usd"].is_null(),
            never["run_cost_complete"].as_bool()
        ),
        (Some(0), true, Some(false))
    );
}
