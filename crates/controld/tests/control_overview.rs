//! Tests: the organisation overview (`GET /api/control/overview`) shows every team as a unit with its
//! agents by state, its tool calls per minute and latest call, and its latest run's usage. The running team's agent is a live one (`common::live`).
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
async fn a_team_never_started_is_a_unit_counted_from_its_design() {
    let s = common::m2::serve().await;
    let idle = common::m2::chain(&s, "Not started", &[("a", "A", "")], &[]).await;
    let (status, v) = common::m2::get(&s.router, "/api/control/overview").await;
    assert_eq!(status.as_u16(), 200, "{v}");
    never_started_unit(unit(&v, &idle));
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_every_team_is_a_unit_with_its_state_and_key_figures() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = common::m2::serve().await;
    let _host = common::live::connect(&s).await;
    let idle = common::m2::chain(&s, "Not started", &[("a", "A", "")], &[]).await;
    let id = common::live::team(&s, "solo").await;
    let agent = common::m2::seat(&id, "claude");
    common::m2::say(
        &s,
        &agent,
        "Use your Bash tool to run exactly: echo checked. Then reply OK.",
    )
    .await;
    common::live::wait_for("the call is recorded", || {
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
    common::m2::stop_team(&s, &id).await;
}

/// A running team: its agent counted as running, its calls in its last minutes, the latest one its agent's.
fn running_unit(running: &Value, agent: &str) {
    assert_eq!(running["started"], true);
    assert_eq!(running["desired"], "running");
    assert_eq!(
        (running["agents"].as_u64(), running["running"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(running["actions_per_min"].as_array().unwrap().len(), 15);
    assert!(
        running["actions_per_min"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap())
            .sum::<u64>()
            >= 1,
        "{running}"
    );
    assert_eq!(running["last_action"]["seat"], agent);
    assert!(running["last_action"]["tool"].is_string(), "{running}");
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
