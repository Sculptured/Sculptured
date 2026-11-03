//! Pause and resume, for one agent and for a whole team: a paused agent stops at its next safe point
//! and keeps its session, and Resume continues that session (Practice agents, no model is called).
mod common;
use axum::http::StatusCode;
use common::m2::{Served, call, connect_stub, get, practice_chain, rid, seat, start_team};
use common::{lifecycle, wait_for};
use model::{EventBody, SeatLifecycle, TenureCause};
use serde_json::{Value, json};

async fn team(s: &Served, name: &str, agents: &[&str]) -> String {
    let agents: Vec<(&str, &str, &str)> = agents.iter().map(|a| (*a, *a, "")).collect();
    practice_chain(s, name, &agents, &[]).await
}

async fn control(s: &Served, seat: &str, mode: &str) -> (StatusCode, Value) {
    let body = json!({ "request_id": rid(), "mode": mode });
    call(
        &s.router,
        "POST",
        &format!("/api/seats/{seat}/control"),
        body,
    )
    .await
}

async fn team_action(s: &Served, id: &str, action: &str, request_id: &str) -> (StatusCode, Value) {
    let body = json!({ "request_id": request_id });
    call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/{action}"),
        body,
    )
    .await
}

async fn mode(s: &Served, seat: &str) -> Value {
    let (status, body) = get(&s.router, &format!("/api/seats/{seat}/control")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["mode"].clone()
}

fn queued(s: &Served, seat: &str) -> Vec<String> {
    common::m2::events(&s.st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::MessageQueued { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// The sessions `seat` started, in order: (cause, native session id).
fn tenures(s: &Served, seat: &str) -> Vec<(TenureCause, Option<String>)> {
    common::m2::events(&s.st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::TenureStarted {
                cause,
                native_session_id,
                ..
            } => Some((cause, native_session_id)),
            _ => None,
        })
        .collect()
}

fn session(s: &Served, seat: &str) -> Option<String> {
    let store = s.st.store.lock().unwrap();
    store.seat(seat).unwrap().and_then(|v| v.native_session_id)
}

async fn wait_lifecycle(s: &Served, seats: &[&String], want: SeatLifecycle) {
    wait_for(&format!("{seats:?} {want:?}"), || {
        seats.iter().all(|x| lifecycle(&s.st, x) == Some(want))
    })
    .await;
}

fn affected(body: &Value) -> Vec<String> {
    let mut out: Vec<String> = body["affected"]
        .as_array()
        .unwrap_or_else(|| panic!("no affected list: {body}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

fn skipped(body: &Value) -> Vec<(String, String)> {
    body["skipped"]
        .as_array()
        .unwrap_or_else(|| panic!("no skipped list: {body}"))
        .iter()
        .map(|v| {
            let reason = v["reason"].as_str().unwrap().to_string();
            (v["seat"].as_str().unwrap().to_string(), reason)
        })
        .collect()
}

/// The body of a call that must succeed.
fn ok((status, body): (StatusCode, Value)) -> Value {
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// A running team of agents a, b and c, with c stopped by the operator.
async fn three_with_c_stopped(s: &Served, name: &str) -> (String, [String; 3]) {
    let id = team(s, name, &["a", "b", "c"]).await;
    start_team(s, &id).await;
    let seats = ["a", "b", "c"].map(|x| seat(&id, x));
    ok(control(s, &seats[2], "hard").await);
    wait_lifecycle(s, &[&seats[2]], SeatLifecycle::Stopped).await;
    (id, seats)
}

/// `seat`'s latest session resumed the one it kept (`kept`).
fn assert_resumed(s: &Served, seat: &str, kept: Option<String>) {
    let all = tenures(s, seat);
    assert_eq!(
        all.last(),
        Some(&(TenureCause::Resume, kept)),
        "Resume continues the same session: {all:?}"
    );
}

#[tokio::test]
async fn pause_agent_then_resume_continues_session() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let id = team(&s, "pause-one", &["a", "b"]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));

    ok(control(&s, &a, "pause").await);
    assert!(
        queued(&s, &a)
            .iter()
            .any(|t| t.contains("Paused by You") && t.contains(".sculptured/notes/a.md")),
        "the agent is asked to stop at a checkpoint and write its note: {:?}",
        queued(&s, &a)
    );
    wait_lifecycle(&s, &[&a], SeatLifecycle::Stopped).await;
    assert_eq!(mode(&s, &a).await, json!("paused"));
    assert_eq!(lifecycle(&s.st, &b), Some(SeatLifecycle::Running));
    let (status, body) = control(&s, &a, "pause").await;
    assert_eq!(status, StatusCode::OK, "pausing again is a no-op: {body}");
    let kept = session(&s, &a).expect("the paused agent keeps its session");
    let before = tenures(&s, &a).len();

    ok(control(&s, &a, "resume").await);
    wait_lifecycle(&s, &[&a], SeatLifecycle::Running).await;
    assert_eq!(tenures(&s, &a).len(), before + 1);
    assert_resumed(&s, &a, Some(kept));
    wait_for("the continue message", || {
        queued(&s, &a)
            .iter()
            .any(|t| t.contains("Continue your job where you stopped"))
    })
    .await;
    assert_eq!(mode(&s, &a).await, Value::Null);
}

#[tokio::test]
async fn team_pause_affects_all_live_agents_and_skips_stopped() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let (id, [a, b, c]) = three_with_c_stopped(&s, "pause-all").await;

    let request = rid();
    let body = ok(team_action(&s, &id, "pause", &request).await);
    assert_eq!(affected(&body), vec![a.clone(), b.clone()]);
    let skipped = skipped(&body);
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(skipped[0].0, c);
    assert!(skipped[0].1.contains("not running"), "{skipped:?}");
    let (status, again) = team_action(&s, &id, "pause", &request).await;
    assert_eq!(
        (status, &again),
        (StatusCode::OK, &body),
        "the same request answers the same"
    );

    wait_lifecycle(&s, &[&a, &b], SeatLifecycle::Stopped).await;
    assert_eq!(mode(&s, &a).await, json!("paused"));
    assert_eq!(mode(&s, &b).await, json!("paused"));
    assert_eq!(
        mode(&s, &c).await,
        json!("hard"),
        "a stopped agent stays stopped"
    );
}

#[tokio::test]
async fn team_resume_restarts_paused() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let (id, [a, b, c]) = three_with_c_stopped(&s, "resume-all").await;
    ok(team_action(&s, &id, "pause", &rid()).await);
    wait_lifecycle(&s, &[&a, &b], SeatLifecycle::Stopped).await;
    wait_for("both paused", || {
        let store = s.st.store.lock().unwrap();
        [&a, &b]
            .iter()
            .all(|x| store.control(x).unwrap() == Some(controld::store::ControlMode::Paused))
    })
    .await;
    let kept = (session(&s, &a), session(&s, &b));

    let body = ok(team_action(&s, &id, "resume", &rid()).await);
    assert_eq!(affected(&body), vec![a.clone(), b.clone()]);
    assert_eq!(
        skipped(&body).iter().map(|x| &x.0).collect::<Vec<_>>(),
        vec![&c],
        "the agent stopped before the pause is not paused"
    );
    wait_lifecycle(&s, &[&a, &b], SeatLifecycle::Running).await;
    assert_resumed(&s, &a, kept.0);
    assert_resumed(&s, &b, kept.1);
    assert_eq!(lifecycle(&s.st, &c), Some(SeatLifecycle::Stopped));
}

#[tokio::test]
async fn pause_when_nothing_running_is_empty_result_not_error() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let never = team(&s, "never-started", &["a"]).await;
    for action in ["pause", "resume"] {
        let body = ok(team_action(&s, &never, action, &rid()).await);
        assert_eq!(body, json!({ "affected": [], "skipped": [] }), "{action}");
    }

    let id = team(&s, "all-stopped", &["a", "b"]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    common::m2::stop_team(&s, &id).await;
    wait_lifecycle(&s, &[&a, &b], SeatLifecycle::Stopped).await;
    for action in ["pause", "resume"] {
        let body = ok(team_action(&s, &id, action, &rid()).await);
        assert!(affected(&body).is_empty(), "{action}: {body}");
        assert_eq!(skipped(&body).len(), 2, "{action}: {body}");
    }

    let (status, body) = team_action(&s, "no-such-team", "pause", &rid()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
