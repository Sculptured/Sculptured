//! Helpers for M2 integration tests: a served controld, a stub host, HTTP calls and event waits.
//! Later tasks use these and keep their own extra helpers in their own test files.
#![allow(dead_code)]
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use controld::AppState;
use controld::store::Store;
use drivers::Driver;
use drivers::stub::StubDriver;
use hostd::journal::Journal;
use hostd::supervisor::{SeatToolsHost, Supervisor};
use http_body_util::BodyExt;
use model::{Actor, EventBody, EventRecord, SeatLifecycle};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

pub const TOKEN: &str = "test-token";

pub struct Served {
    pub st: AppState,
    pub router: Router,
    pub url: String,
    pub home: tempfile::TempDir,
}

/// A served controld (its background tasks start with the router) over an in-memory store.
pub async fn serve() -> Served {
    serve_with_store(Store::open_in_memory().unwrap()).await
}

pub const GATEWAY_SECRET: &[u8] = b"test-gateway-secret";

/// A hosted controld: `router` answers only gateway-signed requests (use `signed`), while runners
/// connect over `url` as usual. Both views share one state (store, connected computers).
pub async fn serve_hosted() -> Served {
    let mut s = serve().await;
    let hosted = s.st.clone().with_auth(controld::auth::AuthPolicy::Gateway {
        secret: Arc::new(GATEWAY_SECRET.to_vec()),
    });
    s.router = controld::app(hosted.clone(), None);
    s.st = hosted;
    s
}

/// A gateway-signed request for user `u1`, as the hosted router expects.
pub async fn signed(
    router: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "test")
        .header(signing::USER_HEADER, "u1")
        .header(signing::TS_HEADER, ts.to_string())
        .header(
            signing::SIG_HEADER,
            signing::sign(GATEWAY_SECRET, method, uri, "u1", ts),
        );
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A served controld over `store`, for tests that prepare the store before anything runs.
pub async fn serve_with_store(store: Store) -> Served {
    let home = tempfile::tempdir().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let st = AppState::new(store, TOKEN.into(), vec![addr.to_string(), "test".into()])
        .with_home(home.path());
    let served = controld::app(st.clone(), None);
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    Served {
        router: controld::app(st.clone(), None),
        st,
        url: format!("ws://{addr}/host"),
        home,
    }
}

pub async fn connect(
    s: &Served,
    drivers: Vec<Arc<dyn Driver>>,
    seat_tools: Option<SeatToolsHost>,
) -> Supervisor {
    connect_with(s, "local", drivers, seat_tools).await
}

/// A runner connected as `host_id` that answers local operations with `ops` (a connected computer).
pub async fn connect_with_ops(
    s: &Served,
    host_id: &str,
    drivers: Vec<Arc<dyn Driver>>,
    ops: hostd::ops::OpsHost,
) -> Supervisor {
    let sup = Supervisor::with_seat_tools(
        host_id.into(),
        drivers,
        Journal::open_in_memory(10_000).unwrap(),
        None,
    )
    .with_ops(ops);
    let (url, link_sup) = (s.url.clone(), sup.clone());
    tokio::spawn(async move {
        let _ = hostd::link::run_link(&url, TOKEN, &link_sup).await;
    });
    crate::common::wait_for("host connected", || s.st.hosts.is_connected(host_id)).await;
    sup
}

/// A runner connected as `host_id` (another computer).
pub async fn connect_as(s: &Served, host_id: &str, drivers: Vec<Arc<dyn Driver>>) -> Supervisor {
    connect_with(s, host_id, drivers, None).await
}

async fn connect_with(
    s: &Served,
    host_id: &str,
    drivers: Vec<Arc<dyn Driver>>,
    seat_tools: Option<SeatToolsHost>,
) -> Supervisor {
    let sup = Supervisor::with_seat_tools(
        host_id.into(),
        drivers,
        Journal::open_in_memory(10_000).unwrap(),
        seat_tools,
    );
    let (url, link_sup) = (s.url.clone(), sup.clone());
    tokio::spawn(async move {
        let _ = hostd::link::run_link(&url, TOKEN, &link_sup).await;
    });
    crate::common::wait_for("host connected", || s.st.hosts.is_connected(host_id)).await;
    sup
}

pub async fn connect_stub(s: &Served) -> Supervisor {
    connect(s, vec![Arc::new(StubDriver)], None).await
}

pub fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn call(router: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "test")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

pub async fn get(router: &Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .header("host", "test")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The seat id of `agent` in team `team` (every agent sits in the default group, `team`).
pub fn seat(team: &str, agent: &str) -> String {
    format!("team.{agent}@{team}")
}

/// Creates a Practice team from a built-in template and starts it; returns the team id once every agent runs.
pub async fn practice_team(s: &Served, template: &str) -> String {
    let (status, rec) = call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": template, "practice": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let id = rec["draft"]["id"].as_str().unwrap().to_string();
    start_team(s, &id).await;
    id
}

/// A Practice team of `(id, name, job)` agents joined by "hands work to" `(from, to)` links. Not started.
pub async fn practice_chain(
    s: &Served,
    name: &str,
    agents: &[(&str, &str, &str)],
    links: &[(&str, &str)],
) -> String {
    let (status, rec) = call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "practice": true, "name": name }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = agents
        .iter()
        .enumerate()
        .map(|(i, (id, name, job))| {
            json!({ "id": id, "group_id": null, "name": name, "kind": "practice", "job": job, "folder": null, "x": 40 + 260 * i as i32, "y": 40 })
        })
        .collect();
    draft["connections"] =
        links.iter().map(|(from, to)| json!({ "id": format!("c-{from}-{to}"), "from": from, "to": to, "kind": "hands-work-to" })).collect();
    let id = draft["id"].as_str().unwrap().to_string();
    let revision = draft["revision"].clone();
    let (status, body) = call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": revision, "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

pub async fn start_team(s: &Served, id: &str) {
    let (status, body) = call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("boot finished", || !s.st.is_booting(id)).await;
    let seats: Vec<String> =
        s.st.store
            .lock()
            .unwrap()
            .rig_snapshot(id)
            .unwrap()
            .unwrap()
            .seats
            .into_iter()
            .map(|x| x.id)
            .collect();
    crate::common::wait_for("every agent running", || {
        seats
            .iter()
            .all(|x| crate::common::lifecycle(&s.st, x) == Some(SeatLifecycle::Running))
    })
    .await;
}

pub async fn stop_team(s: &Served, id: &str) {
    let (status, body) = call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// The operator sends `text` (inform) to `seat`.
pub async fn say(s: &Served, seat: &str, text: &str) {
    let body = json!({ "request_id": rid(), "text": text, "intent": "inform" });
    let (status, resp) = call(&s.router, "POST", &format!("/api/seats/{seat}/send"), body).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
}

pub fn events(st: &AppState, seat: &str) -> Vec<EventRecord> {
    st.store
        .lock()
        .unwrap()
        .events_for_seat(seat, 0, 5000)
        .unwrap()
}

pub async fn wait_event(
    st: &AppState,
    seat: &str,
    what: &str,
    pred: impl Fn(&EventBody) -> bool,
) -> EventRecord {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(e) = events(st, seat).into_iter().find(|e| pred(&e.body)) {
            return e;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub fn agent_texts(st: &AppState, seat: &str) -> Vec<String> {
    events(st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::AgentText { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

/// `(seq, message id, text)` of each message queued to `seat` by system actor `actor` (`team-start`, `handoff`).
pub fn messages_by(st: &AppState, seat: &str, actor: &str) -> Vec<(i64, String, String)> {
    events(st, seat)
        .into_iter()
        .filter_map(|e| match (&e.actor, e.body) {
            (
                Actor::System(a),
                EventBody::MessageQueued {
                    message_id, text, ..
                },
            ) if a == actor => Some((e.seq, message_id, super::work_message(&text))),
            _ => None,
        })
        .collect()
}
