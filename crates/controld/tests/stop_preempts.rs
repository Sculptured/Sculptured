//! Safety stops never wait for a team action: Stop everything now, and an agent's hard stop, go ahead
//! while Resume all holds the team's claim, and Resume all leaves the stopped agents stopped. Live: the
//! agents run on the real Claude Code CLI (no job; a pause asks for one short wrap-up). Resume all is
//! held mid-way for real: the computer's link drops just before it, so it waits for the computer to
//! come back (as it does after a Wi-Fi change) while the test stops agents.
mod common;
#[path = "common/live_controls.rs"]
mod live_controls;
use axum::http::StatusCode;
use common::m2::{Served, call, rid, seat, start_team};
use common::pausing::*;
use controld::store::ControlMode;
use live_controls::{Runner, wait_lifecycle, wait_paused};
use model::SeatLifecycle;
use serde_json::{Value, json};
use std::time::Duration;

/// A started live team of `agents`, all paused.
async fn paused_team(name: &str, agents: &[&str]) -> (Served, Paused) {
    let s = common::m2::serve().await;
    let runner = Runner::connect(&s).await;
    let id = team(&s, name, agents).await;
    start_team(&s, &id).await;
    let seats: Vec<String> = agents.iter().map(|a| seat(&id, a)).collect();
    ok(team_action(&s, &id, "pause", &rid()).await);
    wait_paused(&s, &seats.iter().collect::<Vec<_>>()).await;
    let paused = Paused { runner, id, seats };
    (s, paused)
}

struct Paused {
    runner: Runner,
    id: String,
    seats: Vec<String>,
}

/// Drops the computer's link, then starts Resume all in the background; returns it once it holds the
/// claim with an agent starting (its pause cleared, its relaunch waiting for the computer), and that
/// agent.
async fn resume_all_in_flight(
    s: &Served,
    p: &Paused,
) -> (tokio::task::JoinHandle<(StatusCode, Value)>, String) {
    p.runner.drop_link(s).await;
    let (router, uri) = (s.router.clone(), format!("/api/teams/{}/resume", p.id));
    let resume =
        tokio::spawn(
            async move { call(&router, "POST", &uri, json!({ "request_id": rid() })).await },
        );
    let starting = || p.seats.iter().find(|x| control_of(s, x).is_none()).cloned();
    common::wait_for("an agent resuming", || {
        s.st.team_action_running(&p.id) && starting().is_some()
    })
    .await;
    (resume, starting().unwrap())
}

async fn stop_now(s: &Served, id: &str) -> (StatusCode, Value) {
    let uri = format!("/api/teams/{id}/stop-now");
    let call = call(&s.router, "POST", &uri, json!({ "request_id": rid() }));
    tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("a safety stop does not wait for Resume all")
}

fn control_of(s: &Served, seat: &str) -> Option<ControlMode> {
    s.st.store.lock().unwrap().control(seat).unwrap()
}

/// No agent of `seats` runs, now or a moment later (nothing started after the stop).
async fn stays_stopped(s: &Served, seats: &[String]) {
    wait_lifecycle(s, &seats.iter().collect::<Vec<_>>(), SeatLifecycle::Stopped).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for x in seats {
        assert_eq!(
            common::lifecycle(&s.st, x),
            Some(SeatLifecycle::Stopped),
            "{x}"
        );
    }
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stop_everything_now_goes_ahead_during_resume_all_and_nothing_resumes_after_it() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, p) = paused_team("preempt-team", &["a", "b"]).await;
    let (resume, _) = resume_all_in_flight(&s, &p).await;
    let Paused {
        mut runner,
        id,
        seats,
    } = p;

    ok(stop_now(&s, &id).await);
    runner.reconnect(&s).await;
    let body = ok(resume.await.unwrap());

    assert_eq!(
        affected(&body),
        Vec::<String>::new(),
        "nothing resumed: {body}"
    );
    let skipped = skipped(&body);
    assert_eq!(skipped.len(), 2, "{body}");
    for (seat, code, reason) in skipped {
        assert_eq!(code, "already_stopping", "{seat}: {reason}");
        assert!(reason.contains("stopped meanwhile"), "{reason}");
    }
    stays_stopped(&s, &seats).await;
    // Both are stopped for good: Stop everything now records a hard stop for each.
    let modes: Vec<_> = seats.iter().map(|x| control_of(&s, x)).collect();
    assert!(modes.contains(&Some(ControlMode::Hard)), "{modes:?}");
    assert!(
        modes
            .iter()
            .all(|m| matches!(m, Some(ControlMode::Hard | ControlMode::Paused))),
        "{modes:?}"
    );
    assert!(
        !s.st.team_action_running(&id),
        "Resume all let go of the team"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_agents_hard_stop_goes_ahead_during_resume_all_and_only_it_stays_stopped() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, mut p) = paused_team("preempt-agent", &["a", "b"]).await;
    let (resume, starting) = resume_all_in_flight(&s, &p).await;
    let other = p.seats.iter().find(|x| **x != starting).unwrap().clone();

    let stop = control(&s, &starting, "hard");
    let (status, body) = tokio::time::timeout(Duration::from_secs(5), stop)
        .await
        .expect("an agent's hard stop does not wait for Resume all");
    assert_eq!(status, StatusCode::OK, "{body}");
    p.runner.reconnect(&s).await;
    let body = ok(resume.await.unwrap());

    assert_eq!(
        affected(&body),
        vec![other.clone()],
        "the other agent resumes: {body}"
    );
    stays_stopped(&s, std::slice::from_ref(&starting)).await;
    assert_eq!(control_of(&s, &starting), Some(ControlMode::Hard));
    wait_lifecycle(&s, &[&other], SeatLifecycle::Running).await;
}

/// While a team action holds the team (here the claim is taken directly, as Resume all takes it), a
/// second one is refused in plain words.
#[tokio::test]
async fn another_team_action_is_refused_in_plain_words_while_one_runs() {
    let s = common::m2::serve().await;
    let id = team(&s, "preempt-busy", &["a", "b"]).await;
    let held = s.st.try_begin_team_action(&id, s.st.stop_count());
    assert!(held.is_some(), "the team is free");
    for action in ["pause", "resume"] {
        let (status, body) = team_action(&s, &id, action, &rid()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{action}: {body}");
        assert_eq!(body["error"]["code"], "team_busy", "{action}: {body}");
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("Another team action is running")),
            "{body}"
        );
    }
    drop(held);
    let body = ok(team_action(&s, &id, "pause", &rid()).await);
    assert!(affected(&body).is_empty(), "free again: {body}");
}
