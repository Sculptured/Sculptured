//! Sending a team's runs as OpenTelemetry traces: the setting keeps its key on the server, and a run's
//! spans reach a stand-in collector (no third-party call). Runs are events written to the store as the
//! runner and controld record them.
mod common;
use axum::Router;
use axum::extract::State;
use axum::http::HeaderMap;
use common::m2::{Served, rid};
use controld::store::RigSource;
use model::{Actor, EventBody, Provenance, SeatLifecycle, Subject, TurnStatus};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

async fn team(s: &Served, dir: &std::path::Path) -> String {
    let (_, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Traced" }),
    )
    .await;
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([{ "id": "a", "group_id": null, "name": "Writer", "kind": "claude", "job": "Write the page.", "folder": dir.display().to_string(), "x": 40, "y": 40 }]);
    draft["connections"] = json!([]);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{body}");
    id
}

#[tokio::test]
async fn the_key_stays_on_the_server_and_bad_settings_are_refused() {
    let s = crate::common::m2::serve().await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let url = format!("/api/teams/{id}/traces");
    let (_, v) = crate::common::m2::get(&s.router, &url).await;
    assert_eq!(
        v,
        json!({ "enabled": false, "endpoint": "https://api.lmnr.ai/v1/traces", "key_saved": false, "content": "metadata", "last_error": null, "last_sent_ms": null })
    );
    let (status, v) = crate::common::m2::call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://api.lmnr.ai/v1/traces", "key": "lmnr-secret", "content": "metadata" })).await;
    assert_eq!(status.as_u16(), 200, "{v}");
    assert_eq!(
        (v["enabled"].as_bool(), v["key_saved"].as_bool()),
        (Some(true), Some(true))
    );
    assert!(
        !v.to_string().contains("lmnr-secret")
            && !crate::common::m2::get(&s.router, &url)
                .await
                .1
                .to_string()
                .contains("lmnr-secret"),
        "the key is never sent back"
    );
    let (_, v) = crate::common::m2::call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://api.lmnr.ai/v1/traces", "content": "full" })).await;
    assert_eq!(
        (v["key_saved"].as_bool(), v["content"].as_str()),
        (Some(true), Some("full")),
        "no key given: the saved one stays"
    );
    let (_, v) = crate::common::m2::call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://otel.example.com/v1/traces", "content": "full" })).await;
    assert_eq!(
        v["key_saved"], false,
        "a key saved for one address never goes to another"
    );
    let (_, v) = crate::common::m2::call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://otel.example.com/v1/traces", "key": "k2", "content": "full" })).await;
    assert_eq!(v["key_saved"], true);
    let (_, v) = crate::common::m2::call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://otel.example.com/v1/traces", "key": "", "content": "full" })).await;
    assert_eq!(v["key_saved"], false, "an empty key removes it");
    for (body, code) in [
        (
            json!({ "request_id": rid(), "enabled": true, "endpoint": "http://evil.example/v1/traces", "content": "metadata" }),
            "trace_endpoint_invalid",
        ),
        (
            json!({ "request_id": rid(), "enabled": true, "endpoint": "not a url", "content": "metadata" }),
            "trace_endpoint_invalid",
        ),
    ] {
        let (status, v) = crate::common::m2::call(&s.router, "PUT", &url, body).await;
        assert_eq!(
            (status.as_u16(), v["error"]["code"].as_str()),
            (400, Some(code)),
            "{v}"
        );
    }
    let (status, v) = crate::common::m2::call(&s.router, "PUT", &url, json!({ "request_id": rid(), "enabled": true, "endpoint": "https://api.lmnr.ai/v1/traces", "content": "everything" })).await;
    assert_eq!(
        (status.as_u16(), v["error"]["code"].as_str()),
        (422, Some("bad_body")),
        "{v}"
    );
    let (status, _) = crate::common::m2::get(&s.router, "/api/teams/nope/traces").await;
    assert_eq!(status.as_u16(), 404);
}

type Got = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

async fn collector() -> (String, Got) {
    let got: Got = Arc::default();
    let app = Router::new()
        .route(
            "/v1/traces",
            axum::routing::post(
                |State(g): State<Got>, h: HeaderMap, body: String| async move {
                    g.lock()
                        .unwrap()
                        .push((h, serde_json::from_str(&body).unwrap()));
                    "{}"
                },
            ),
        )
        .with_state(got.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://127.0.0.1:{}/v1/traces",
        listener.local_addr().unwrap().port()
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, got)
}

fn span_names(got: &Got) -> Vec<String> {
    got.lock()
        .unwrap()
        .iter()
        .flat_map(|(_, v)| {
            v["resourceSpans"][0]["scopeSpans"][0]["spans"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect()
}

/// The next runner sequence number: every event the "runner" reports in this binary is new.
static HOST_SEQ: AtomicU64 = AtomicU64::new(0);

/// Records `body` on `seat` as the runner reports it.
fn host(s: &Served, seat: &str, body: EventBody) {
    let ev = host_proto::HostEvent {
        host_seq: HOST_SEQ.fetch_add(1, Ordering::SeqCst) + 1,
        at_ms: model::now_ms(),
        actor: Actor::System("hostd".into()),
        subject: Subject::Seat(seat.to_string()),
        provenance: Provenance::ProtocolObserved,
        body,
    };
    s.st.store.lock().unwrap().ingest("local", ev).unwrap();
}

/// Records `body` on team `id` as controld does.
fn on_team(s: &Served, id: &str, body: EventBody) {
    s.st.store
        .lock()
        .unwrap()
        .append_local(
            Actor::Human("local".into()),
            Subject::Rig(id.to_string()),
            Provenance::Operator,
            body,
        )
        .unwrap();
}

/// Team `id` starts and its agent's session begins (session number `tenure`).
fn start_run(s: &Served, id: &str, tenure: u32) {
    {
        let mut store = s.st.store.lock().unwrap();
        let draft = store.draft(id).unwrap().unwrap();
        let rig = rigspec::from_draft(&draft).unwrap().rig;
        let source = RigSource::Team {
            id: id.to_string(),
            revision: draft.revision,
        };
        store
            .upsert_rig(&rig, &source, model::Desired::Running)
            .unwrap();
    }
    on_team(
        s,
        id,
        EventBody::RigBooted {
            spec_hash: "x".into(),
            seat_count: 1,
        },
    );
    let seat = format!("team.a@{id}");
    host(
        s,
        &seat,
        EventBody::TenureStarted {
            tenure,
            cause: model::TenureCause::Fresh,
            driver: "claude-stream".into(),
            native_session_id: None,
            caps: Default::default(),
        },
    );
    host(
        s,
        &seat,
        EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Running,
            reason: None,
        },
    );
}

/// The agent of team `id` does one turn that writes `text`.
fn turn(s: &Served, id: &str, turn_id: &str, text: &str) {
    let seat = format!("team.a@{id}");
    host(
        s,
        &seat,
        EventBody::TurnStarted {
            turn_id: turn_id.into(),
        },
    );
    host(
        s,
        &seat,
        EventBody::AgentText {
            turn_id: Some(turn_id.into()),
            text: text.into(),
        },
    );
    host(
        s,
        &seat,
        EventBody::TurnEnded {
            turn_id: turn_id.into(),
            status: TurnStatus::Completed,
        },
    );
}

/// Team `id` is stopped and its agent's session `tenure` ends.
fn stop_run(s: &Served, id: &str, tenure: u32) {
    on_team(
        s,
        id,
        EventBody::RigDesiredChanged {
            desired: model::Desired::Stopped,
        },
    );
    host(
        s,
        &format!("team.a@{id}"),
        EventBody::TenureEnded {
            tenure,
            exit_code: Some(0),
            signal: None,
            stderr_tail: String::new(),
        },
    );
}

#[tokio::test]
async fn a_runs_spans_reach_the_collector_with_the_key() {
    let s = crate::common::m2::serve().await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let (endpoint, got) = collector().await;
    let (status, v) = crate::common::m2::call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "key": "k-1", "content": "full" })).await;
    assert_eq!(status.as_u16(), 200, "{v}");
    start_run(&s, &id, 1);
    controld::traces::export::tick(&s.st).await;
    assert!(
        got.lock().unwrap().is_empty(),
        "nothing has ended yet: {:?}",
        span_names(&got)
    );
    turn(&s, &id, "t1", "the secret page text");
    // Switched to timings only while the run goes: from now on no text is sent.
    let (_, v) = crate::common::m2::call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "content": "metadata" })).await;
    assert_eq!(v["key_saved"], true, "{v}");
    controld::traces::export::tick(&s.st).await;
    let names = span_names(&got);
    let saved = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/traces"))
        .await
        .1;
    assert!(names.iter().any(|n| n == "turn"), "{names:?} {saved}");
    let (h, _) = got.lock().unwrap()[0].clone();
    assert_eq!(h["authorization"].to_str().unwrap(), "Bearer k-1");
    assert_eq!(h["content-type"].to_str().unwrap(), "application/json");
    stop_run(&s, &id, 1);
    controld::traces::export::tick(&s.st).await;
    let names = span_names(&got);
    assert!(
        names.iter().any(|n| n == "run Traced"),
        "the run's own span goes once the run ends: {names:?}"
    );
    assert!(names.iter().any(|n| n == "agent Writer"), "{names:?}");
    assert_eq!(
        crate::common::m2::get(&s.router, &format!("/api/teams/{id}/traces"))
            .await
            .1["last_error"],
        Value::Null
    );
    assert!(
        got.lock()
            .unwrap()
            .iter()
            .all(|(_, body)| !body.to_string().contains("k-1")),
        "the key travels only in the header"
    );
    assert!(
        got.lock().unwrap().iter().all(|(_, body)| {
            let b = body.to_string();
            !b.contains("lmnr.span.input")
                && !b.contains("lmnr.span.output")
                && !b.contains("the secret page text")
        }),
        "timings only"
    );
    let before = names.len();
    controld::traces::export::tick(&s.st).await;
    assert_eq!(span_names(&got).len(), before, "nothing is sent twice");
}

#[tokio::test]
async fn a_run_started_again_before_it_was_sent_still_goes_whole() {
    let s = crate::common::m2::serve().await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let (endpoint, got) = collector().await;
    crate::common::m2::call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "key": "k", "content": "metadata" })).await;
    start_run(&s, &id, 1);
    turn(&s, &id, "t1", "done");
    controld::traces::export::tick(&s.st).await;
    stop_run(&s, &id, 1);
    start_run(&s, &id, 2);
    controld::traces::export::tick(&s.st).await;
    let names = span_names(&got);
    assert!(
        names.iter().any(|n| n == "run Traced"),
        "the first run's own span went: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "agent Writer"),
        "and its agent's session: {names:?}"
    );
}
#[tokio::test]
async fn a_collector_that_keeps_failing_is_tried_less_often() {
    let s = crate::common::m2::serve().await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path()).await;
    let hits = Arc::new(AtomicU64::new(0));
    let h = hits.clone();
    let app = Router::new().route(
        "/v1/traces",
        axum::routing::post(move || {
            h.fetch_add(1, Ordering::SeqCst);
            async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "down") }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://127.0.0.1:{}/v1/traces",
        listener.local_addr().unwrap().port()
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    crate::common::m2::call(&s.router, "PUT", &format!("/api/teams/{id}/traces"), json!({ "request_id": rid(), "enabled": true, "endpoint": endpoint, "key": "k", "content": "metadata" })).await;
    start_run(&s, &id, 1);
    turn(&s, &id, "t1", "done");
    controld::traces::export::tick(&s.st).await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(
        crate::common::m2::get(&s.router, &format!("/api/teams/{id}/traces"))
            .await
            .1["last_error"]
            .as_str()
            .unwrap()
            .contains("500")
    );
    controld::traces::export::tick(&s.st).await;
    assert_eq!(hits.load(Ordering::SeqCst), 1, "the next try waits");
}
