//! Pause and resume at the edges: calling a pause off, an agent still settling, an agent that is not
//! running or already stopping, and agents started by another agent. Live: the agents run on the real
//! Claude Code CLI (they get no job; a pause asks for one short wrap-up, a child one short task).
mod common;
#[path = "common/live_controls.rs"]
mod live_controls;
use common::lifecycle;
use common::m2::{Served, rid, seat, start_team};
use common::pausing::*;
use controld::store::ControlMode;
use live_controls::{SAY_OK, wait_lifecycle, wait_paused};
use model::{EventBody, SeatLifecycle};
use serde_json::{Value, json};

/// A running team of live agents a and b.
async fn running(s: &Served, name: &str) -> (String, String, String) {
    let id = team(s, name, &["a", "b"]).await;
    start_team(s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    (id, a, b)
}

/// Records a control for `seat` directly, as a wrap-up still under way (no message to wait on).
fn record(s: &Served, seat: &str, mode: ControlMode) {
    let store = s.st.store.lock().unwrap();
    store.record_control(seat, mode, 0).unwrap();
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_resume_while_pausing_calls_the_pause_off() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let (_, a, _) = running(&s, "call-off").await;
    record(&s, &a, ControlMode::Pausing);

    let body = ok(control(&s, &a, "resume").await);
    assert_eq!(affected(&body), vec![a.clone()]);
    assert_eq!(mode(&s, &a).await, Value::Null);
    assert!(
        queued(&s, &a)
            .iter()
            .any(|t| t.contains("Pause called off"))
    );
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(
        lifecycle(&s.st, &a),
        Some(SeatLifecycle::Running),
        "the watcher does not stop an agent whose pause was called off"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_resume_while_still_stopping_is_refused_and_stays_paused() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let (_, a, _) = running(&s, "settling").await;
    ok(control(&s, &a, "pause").await);
    wait_paused(&s, &[&a]).await;
    s.st.local_event(
        model::Actor::System("test".into()),
        model::Subject::Seat(a.clone()),
        model::Provenance::ProtocolObserved,
        EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Stopping,
            reason: Some("test".into()),
        },
    )
    .unwrap();

    assert_eq!(refused(control(&s, &a, "resume").await), "agent_settling");
    assert_eq!(mode(&s, &a).await, json!("paused"));
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_pause_needs_a_running_agent_without_another_stop() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let (id, a, b) = running(&s, "refusals").await;
    ok(control(&s, &a, "hard").await);
    wait_lifecycle(&s, &[&a], SeatLifecycle::Stopped).await;
    assert_eq!(refused(control(&s, &a, "pause").await), "agent_not_running");
    assert_eq!(mode(&s, &a).await, json!("hard"));

    record(&s, &b, ControlMode::Soft);
    assert_eq!(refused(control(&s, &b, "pause").await), "agent_stopping");
    let body = ok(team_action(&s, &id, "pause", &rid()).await);
    assert!(affected(&body).is_empty(), "{body}");
    let skipped = skipped(&body);
    assert!(
        skipped.contains(&(
            b.clone(),
            "already_stopping".into(),
            "It is already stopping".into()
        )),
        "{skipped:?}"
    );
    assert_eq!(mode(&s, &b).await, json!("soft"), "the soft stop is kept");
}

/// A running team of one agent that started a child; returns (team, parent, child).
async fn parent_and_child(s: &Served) -> (String, String, String) {
    let id = team(s, "solo", &["lead"]).await;
    start_team(s, &id).await;
    let parent = seat(&id, "lead");
    let child = spawn(s, &parent, "child1").await;
    (id, parent, child)
}

/// `parent` starts a child agent (made directly through Sculptured's tool handler, as the parent's
/// call would make it) with one short task; returns it once it runs.
async fn spawn(s: &Served, parent: &str, request_id: &str) -> String {
    let args = json!({"request_id": request_id, "name": "Helper", "task": SAY_OK});
    let (ok, result, error) = common::seat_tools::direct(s, parent, "spawn_agent", args).await;
    assert!(ok, "{error:?}");
    let child = result["child"].as_str().unwrap().to_string();
    wait_lifecycle(s, &[&child], SeatLifecycle::Running).await;
    child
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_paused_agent_pauses_and_resumes_the_agents_it_started() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner, _dir) = common::seat_tools::with_tools().await;
    let (_, parent, child) = parent_and_child(&s).await;

    let body = ok(control(&s, &parent, "pause").await);
    assert_eq!(affected(&body), vec![parent.clone()]);
    assert_eq!(skipped(&body)[0].0, child);
    assert_eq!(skipped(&body)[0].1, "started_by_agent");
    wait_paused(&s, &[&parent, &child]).await;
    let kept = (session(&s, &parent), session(&s, &child));

    let body = ok(control(&s, &parent, "resume").await);
    assert_eq!(affected(&body), {
        let mut both = vec![parent.clone(), child.clone()];
        both.sort();
        both
    });
    wait_lifecycle(&s, &[&parent, &child], SeatLifecycle::Running).await;
    assert_resumed(&s, &parent, kept.0);
    assert_resumed(&s, &child, kept.1);
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_pause_all_and_resume_all_cover_the_agents_an_agent_started() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner, _dir) = common::seat_tools::with_tools().await;
    let (id, parent, child) = parent_and_child(&s).await;

    let body = ok(team_action(&s, &id, "pause", &rid()).await);
    assert_eq!(affected(&body), vec![parent.clone()]);
    let skipped = skipped(&body);
    assert_eq!(
        (&skipped[0].0, skipped[0].1.as_str()),
        (&child, "started_by_agent")
    );
    wait_paused(&s, &[&parent, &child]).await;

    let body = ok(team_action(&s, &id, "resume", &rid()).await);
    assert_eq!(affected(&body).len(), 2, "{body}");
    assert!(common::pausing::skipped(&body).is_empty(), "{body}");
    wait_lifecycle(&s, &[&parent, &child], SeatLifecycle::Running).await;
}

/// P started C, C started G; G is paused on its own and C is wrapping up a soft stop.
async fn paused_below_a_soft_stop(s: &Served) -> (String, String, String, String) {
    let (id, p, c) = parent_and_child(s).await;
    let g = spawn(s, &c, "grandchild1").await;
    ok(control(s, &g, "pause").await);
    wait_paused(s, &[&g]).await;
    record(s, &c, ControlMode::Soft);
    (id, p, c, g)
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_pause_says_what_it_does_to_agents_stopping_another_way() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner, _dir) = common::seat_tools::with_tools().await;
    let (_, p, c, _) = paused_below_a_soft_stop(&s).await;
    let body = ok(control(&s, &p, "pause").await);
    assert_eq!(affected(&body), vec![p.clone()]);
    assert_eq!(
        skipped(&body),
        vec![(
            c.clone(),
            "already_stopping".into(),
            "It is already stopping".into()
        )],
        "the paused grandchild is not running, and the soft stop is not called a pause"
    );
    wait_paused(&s, &[&p]).await;
    assert_eq!(mode(&s, &c).await, json!("soft"), "its own stop is kept");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_resume_all_resumes_an_agent_below_a_soft_stop_once() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner, _dir) = common::seat_tools::with_tools().await;
    let (id, p, c, g) = paused_below_a_soft_stop(&s).await;
    ok(control(&s, &p, "pause").await);
    wait_paused(&s, &[&p]).await;

    let body = ok(team_action(&s, &id, "resume", &rid()).await);
    let mut want = vec![p.clone(), g.clone()];
    want.sort();
    assert_eq!(affected(&body), want, "each once: {body}");
    let skipped = skipped(&body);
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!((&skipped[0].0, skipped[0].1.as_str()), (&c, "not_paused"));
    wait_lifecycle(&s, &[&p, &g], SeatLifecycle::Running).await;
}
