//! Helpers for the pause and resume tests: teams, control calls and what they answered.
use super::m2::{Served, call, chain, get, rid, seat, start_team};
use super::{lifecycle, wait_for};
use axum::http::StatusCode;
use controld::store::ControlMode;
use model::{EventBody, SeatLifecycle, TenureCause};
use serde_json::{Value, json};

/// A team of Claude `agents` (no links), not started.
pub async fn team(s: &Served, name: &str, agents: &[&str]) -> String {
    let agents: Vec<(&str, &str, &str)> = agents.iter().map(|a| (*a, *a, "")).collect();
    chain(s, name, &agents, &[]).await
}

pub async fn control(s: &Served, seat: &str, mode: &str) -> (StatusCode, Value) {
    let body = json!({ "request_id": rid(), "mode": mode });
    call(
        &s.router,
        "POST",
        &format!("/api/seats/{seat}/control"),
        body,
    )
    .await
}

pub async fn team_action(
    s: &Served,
    id: &str,
    action: &str,
    request_id: &str,
) -> (StatusCode, Value) {
    let body = json!({ "request_id": request_id });
    call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/{action}"),
        body,
    )
    .await
}

pub async fn mode(s: &Served, seat: &str) -> Value {
    let (status, body) = get(&s.router, &format!("/api/seats/{seat}/control")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["mode"].clone()
}

pub fn queued(s: &Served, seat: &str) -> Vec<String> {
    super::m2::events(&s.st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::MessageQueued { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// The sessions `seat` started, in order: (cause, native session id).
pub fn tenures(s: &Served, seat: &str) -> Vec<(TenureCause, Option<String>)> {
    super::m2::events(&s.st, seat)
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

pub fn session(s: &Served, seat: &str) -> Option<String> {
    let store = s.st.store.lock().unwrap();
    store.seat(seat).unwrap().and_then(|v| v.native_session_id)
}

pub async fn wait_lifecycle(s: &Served, seats: &[&String], want: SeatLifecycle) {
    wait_for(&format!("{seats:?} {want:?}"), || {
        seats.iter().all(|x| lifecycle(&s.st, x) == Some(want))
    })
    .await;
}

pub fn affected(body: &Value) -> Vec<String> {
    let mut out: Vec<String> = body["affected"]
        .as_array()
        .unwrap_or_else(|| panic!("no affected list: {body}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    out.sort();
    out
}

/// Each skipped agent of a pause or resume answer: (seat, code, reason).
pub fn skipped(body: &Value) -> Vec<(String, String, String)> {
    let text = |v: &Value| v.as_str().unwrap().to_string();
    body["skipped"]
        .as_array()
        .unwrap_or_else(|| panic!("no skipped list: {body}"))
        .iter()
        .map(|v| (text(&v["seat"]), text(&v["code"]), text(&v["reason"])))
        .collect()
}

/// The error code of a refused call.
pub fn refused((status, body): (StatusCode, Value)) -> String {
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    body["error"]["code"].as_str().unwrap().to_string()
}

/// Waits until each of `seats` has stopped and is paused.
pub async fn wait_paused(s: &Served, seats: &[&String]) {
    wait_lifecycle(s, seats, SeatLifecycle::Stopped).await;
    wait_for("paused", || {
        let store = s.st.store.lock().unwrap();
        seats
            .iter()
            .all(|x| store.control(x).unwrap() == Some(ControlMode::Paused))
    })
    .await;
}

/// The body of a call that must succeed.
pub fn ok((status, body): (StatusCode, Value)) -> Value {
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

/// A running team of agents a, b and c, with c stopped by the operator.
pub async fn three_with_c_stopped(s: &Served, name: &str) -> (String, [String; 3]) {
    let id = team(s, name, &["a", "b", "c"]).await;
    start_team(s, &id).await;
    let seats = ["a", "b", "c"].map(|x| seat(&id, x));
    ok(control(s, &seats[2], "hard").await);
    wait_lifecycle(s, &[&seats[2]], SeatLifecycle::Stopped).await;
    (id, seats)
}

/// `seat`'s latest session resumed the one it kept (`kept`), and it was told to continue.
pub fn assert_resumed(s: &Served, seat: &str, kept: Option<String>) {
    let all = tenures(s, seat);
    assert_eq!(
        all.last(),
        Some(&(TenureCause::Resume, kept)),
        "Resume continues the same session: {all:?}"
    );
    let told = queued(s, seat);
    assert!(
        told.iter()
            .any(|t| t.contains("Resumed by You. Continue your job where you stopped")),
        "{told:?}"
    );
}
