//! Sending a team's runs as OpenTelemetry traces: the setting keeps its key on the server, and a run's
//! spans reach a stand-in collector (no third-party call).
mod common;
use axum::{Router, extract::State, http::HeaderMap, routing::post};
use common::fake_bin;
use common::m2::*;
use drivers::claude::ClaudeStreamDriver;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

async fn team(s: &Served, dir: &std::path::Path) -> String {
    let (_, rec) = call(&s.router, "POST", "/api/teams", json!({ "request_id": rid(), "template": "blank", "name": "Traced" })).await;
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([{ "id": "a", "group_id": null, "name": "Writer", "kind": "claude", "job": "spend:100:50:2", "folder": dir.display().to_string(), "x": 40, "y": 40 }]);
    draft["connections"] = json!([]);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status.as_u16(), 200, "{body}");
    id
}

#[tokio::test]
async fn the_key_stays_on_the_server_and_bad_settings_are_refused() {
    let s = serve().await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let url = format!("/api/teams/{id}/traces");
    let (_, v) = get(&s.router, &url).await;
    assert_eq!(v, json!({ "enabled": false, "endpoint": "https://api.lmnr.ai/v1/traces", "key_saved": false, "content": "metadata", "last_error": null, "last_sent_ms": null }));
    let (status, v) = call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://api.lmnr.ai/v1/traces", "key": "lmnr-secret", "content": "metadata" })).await;
    assert_eq!(status.as_u16(), 200, "{v}");
    assert_eq!((v["enabled"].as_bool(), v["key_saved"].as_bool()), (Some(true), Some(true)));
    assert!(!v.to_string().contains("lmnr-secret") && !get(&s.router, &url).await.1.to_string().contains("lmnr-secret"), "the key is never sent back");
    let (_, v) = call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://api.lmnr.ai/v1/traces", "content": "full" })).await;
    assert_eq!((v["key_saved"].as_bool(), v["content"].as_str()), (Some(true), Some("full")), "no key given: the saved one stays");
    let (_, v) = call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://otel.example.com/v1/traces", "content": "full" })).await;
    assert_eq!(v["key_saved"], false, "a key saved for one address never goes to another");
    let (_, v) = call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://otel.example.com/v1/traces", "key": "k2", "content": "full" })).await;
    assert_eq!(v["key_saved"], true);
    let (_, v) = call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://otel.example.com/v1/traces", "key": "", "content": "full" })).await;
    assert_eq!(v["key_saved"], false, "an empty key removes it");
    for (body, code) in [
        (json!({ "request_id": rid(), "enabled": true, "endpoint": "http://evil.example/v1/traces", "content": "metadata" }), "trace_endpoint_invalid"),
        (json!({ "request_id": rid(), "enabled": true, "endpoint": "not a url", "content": "metadata" }), "trace_endpoint_invalid"),
    ] {
        let (status, v) = call(&s.router, "PUT", &url, body).await;
        assert_eq!((status.as_u16(), v["error"]["code"].as_str()), (400, Some(code)), "{v}");
    }
    let (status, v) = call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://api.lmnr.ai/v1/traces", "content": "everything" })).await;
    assert_eq!((status.as_u16(), v["error"]["code"].as_str()), (422, Some("bad_body")), "{v}");
    let (status, _) = get(&s.router, "/api/teams/nope/traces").await;
    assert_eq!(status.as_u16(), 404);
}

type Got = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

async fn collector() -> (String, Got) {
    let got: Got = Arc::default();
    let app = Router::new().route("/v1/traces", post(|State(g): State<Got>, h: HeaderMap, body: String| async move {
        g.lock().unwrap().push((h, serde_json::from_str(&body).unwrap()));
        "{}"
    })).with_state(got.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}/v1/traces", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, got)
}

fn span_names(got: &Got) -> Vec<String> {
    got.lock().unwrap().iter().flat_map(|(_, v)| v["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().cloned().unwrap_or_default())
        .map(|s| s["name"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn a_runs_spans_reach_the_collector_with_the_key() {
    let s = serve().await;
    let _runner = connect(&s, vec![Arc::new(ClaudeStreamDriver { program: fake_bin("fake-claude") })], None).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let (endpoint, got) = collector().await;
    let (status, v) = call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "key": "k-1", "content": "full" })).await;
    assert_eq!(status.as_u16(), 200, "{v}");
    start_team(&s, &id).await;
    common::wait_for("the session began", || events(&s.st, &format!("team.a@{id}")).iter().any(|e| matches!(e.body, model::EventBody::TenureStarted { .. }))).await;
    controld::traces::export::tick(&s.st).await;
    assert!(controld_turns_ended(&s, &format!("team.a@{id}")) == 0 || !got.lock().unwrap().is_empty(), "the run is followed in full mode first");
    common::wait_for("a turn ends", || controld_turns_ended(&s, &format!("team.a@{id}")) > 0).await;
    // Switched to timings only while the run goes: from now on no text is sent.
    let (_, v) = call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "content": "metadata" })).await;
    assert_eq!(v["key_saved"], true, "{v}");
    controld::traces::export::tick(&s.st).await;
    let names = span_names(&got);
    let saved = get(&s.router, &format!("/api/teams/{id}/traces")).await.1;
    assert!(names.iter().any(|n| n == "turn"), "{names:?} {saved}");
    let (h, _) = got.lock().unwrap()[0].clone();
    assert_eq!(h["authorization"].to_str().unwrap(), "Bearer k-1");
    assert_eq!(h["content-type"].to_str().unwrap(), "application/json");
    let (status, _) = call(&s.router, "POST", &format!("/api/rigs/{id}/stop"), json!({ "request_id": rid() })).await;
    assert!(status.is_success());
    let seat = format!("team.a@{id}");
    common::wait_for("the session ended", || events(&s.st, &seat).iter().any(|e| matches!(e.body, model::EventBody::TenureEnded { .. }))).await;
    controld::traces::export::tick(&s.st).await;
    let names = span_names(&got);
    assert!(names.iter().any(|n| n == "run Traced"), "the run's own span goes once the run ends: {names:?}");
    assert!(names.iter().any(|n| n == "agent Writer"), "{names:?}");
    assert_eq!(get(&s.router, &format!("/api/teams/{id}/traces")).await.1["last_error"], Value::Null);
    assert!(got.lock().unwrap().iter().all(|(_, body)| !body.to_string().contains("k-1")), "the key travels only in the header");
    assert!(got.lock().unwrap().iter().all(|(_, body)| !body.to_string().contains("lmnr.span.input") && !body.to_string().contains("lmnr.span.output")), "timings only");
    let before = names.len();
    controld::traces::export::tick(&s.st).await;
    assert_eq!(span_names(&got).len(), before, "nothing is sent twice");
}

#[tokio::test]
async fn a_run_started_again_before_it_was_sent_still_goes_whole() {
    let s = serve().await;
    let _runner = connect(&s, vec![Arc::new(ClaudeStreamDriver { program: fake_bin("fake-claude") })], None).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let (endpoint, got) = collector().await;
    call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "key": "k", "content": "metadata" })).await;
    let seat = format!("team.a@{id}");
    start_team(&s, &id).await;
    common::wait_for("a turn ends", || controld_turns_ended(&s, &seat) > 0).await;
    controld::traces::export::tick(&s.st).await;
    let (status, _) = call(&s.router, "POST", &format!("/api/rigs/{id}/stop"), json!({ "request_id": rid() })).await;
    assert!(status.is_success());
    common::wait_for("the session ended", || events(&s.st, &seat).iter().any(|e| matches!(e.body, model::EventBody::TenureEnded { .. }))).await;
    start_team(&s, &id).await;
    common::wait_for("the second run began", || events(&s.st, &seat).iter().filter(|e| matches!(e.body, model::EventBody::TenureStarted { .. })).count() > 1).await;
    controld::traces::export::tick(&s.st).await;
    let names = span_names(&got);
    assert!(names.iter().any(|n| n == "run Traced"), "the first run's own span went: {names:?}");
    assert!(names.iter().any(|n| n == "agent Writer"), "and its agent's session: {names:?}");
}

fn controld_turns_ended(s: &Served, seat: &str) -> usize {
    events(&s.st, seat).iter().filter(|e| matches!(e.body, model::EventBody::TurnEnded { .. })).count()
}

#[tokio::test]
async fn a_collector_that_keeps_failing_is_tried_less_often() {
    let s = serve().await;
    let _runner = connect(&s, vec![Arc::new(ClaudeStreamDriver { program: fake_bin("fake-claude") })], None).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = hits.clone();
    let app = Router::new().route("/v1/traces", post(move || {
        h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "down") }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://127.0.0.1:{}/v1/traces", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "key": "k", "content": "metadata" })).await;
    start_team(&s, &id).await;
    common::wait_for("a turn ends", || controld_turns_ended(&s, &format!("team.a@{id}")) > 0).await;
    controld::traces::export::tick(&s.st).await;
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(get(&s.router, &format!("/api/teams/{id}/traces")).await.1["last_error"].as_str().unwrap().contains("500"));
    controld::traces::export::tick(&s.st).await;
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1, "the next try waits");
}
