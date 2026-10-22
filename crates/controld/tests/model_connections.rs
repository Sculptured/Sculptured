//! Model connections: a hosted Claude agent with a connection gets its keys from the gateway at
//! launch, in its process environment only. A failed read keeps it from starting, in plain words; the
//! keys are never stored or recorded; local mode ignores connections. Fake gateway and stub CLI only.
mod common;
use axum::http::{HeaderMap, StatusCode};
use common::m2::*;
use common::{lifecycle, wait_for};
use drivers::stub::StubDriver;
use drivers::Driver;
use model::{EventBody, SeatLifecycle};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const SECRET: &[u8] = b"instance-secret";
const USER: &str = "6f1c0b3e-1111-4222-8333-944455556666";
const KEY: &str = "sk-ant-api03-NEVER-STORE-THIS-0123456789";

type Asked = Arc<Mutex<Vec<String>>>;

/// A stand-in gateway: answers `/internal/connection-env` for `conn-1` when the request is signed
/// as this instance's user, 404 for any other connection, 401 when the signature is wrong.
async fn fake_gateway() -> (String, Asked) {
    let asked: Asked = Default::default();
    let keep = asked.clone();
    let app = axum::Router::new().route(
        "/internal/connection-env",
        axum::routing::post(move |headers: HeaderMap, axum::Json(body): axum::Json<Value>| {
            let keep = keep.clone();
            async move {
                let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
                let ts: i64 = h(signing::TS_HEADER).parse().unwrap_or(0);
                if h(signing::USER_HEADER) != USER || !signing::verify(SECRET, "POST", "/internal/connection-env", USER, ts, &h(signing::SIG_HEADER), now) {
                    return (StatusCode::UNAUTHORIZED, axum::Json(json!({ "error": "bad signature" })));
                }
                let id = body["connection_id"].as_str().unwrap_or_default().to_string();
                keep.lock().unwrap().push(id.clone());
                if id != "conn-1" {
                    return (StatusCode::NOT_FOUND, axum::Json(json!({ "error": "no such connection" })));
                }
                (StatusCode::OK, axum::Json(json!({ "provider": "anthropic-key", "model": null, "env": [["ANTHROPIC_API_KEY", KEY]] })))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, asked)
}

type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

/// Runs as Claude Code (`claude-stream`) but is the stub underneath; keeps each launch's environment.
struct RecordingClaude(Seen);

#[async_trait::async_trait]
impl Driver for RecordingClaude {
    fn name(&self) -> &'static str {
        "claude-stream"
    }
    fn capabilities(&self) -> model::Caps {
        StubDriver.capabilities()
    }
    async fn launch(&self, spec: drivers::LaunchSpec) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        self.0.lock().unwrap().push((spec.seat.to_string(), spec.env.clone()));
        StubDriver.launch(spec).await
    }
    async fn resume(&self, spec: drivers::LaunchSpec, native_id: &str) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        self.0.lock().unwrap().push((spec.seat.to_string(), spec.env.clone()));
        StubDriver.resume(spec, native_id).await
    }
}

/// A hosted controld that reads model connections from the gateway at `url`, with a computer
/// "laptop" whose Claude Code records what it was launched with.
async fn hosted_with_gateway(url: &str) -> (Served, Seen, hostd::supervisor::Supervisor) {
    let mut s = serve_hosted().await;
    s.st = s.st.clone().with_gateway(controld::gateway::Gateway::new(url.to_string(), USER.to_string(), Arc::new(SECRET.to_vec())));
    s.router = controld::app(s.st.clone(), None);
    let seen: Seen = Default::default();
    let laptop = connect_as(&s, "laptop", vec![Arc::new(RecordingClaude(seen.clone()))]).await;
    (s, seen, laptop)
}

/// A real folder on the computer (the stub runs in it).
fn folder(s: &Served) -> String {
    let dir = s.home.path().join("project");
    std::fs::create_dir_all(&dir).unwrap();
    dir.display().to_string()
}

/// A team with one Claude agent "Writer" using `connection`; returns its id.
async fn claude_team(s: &Served, hosted: bool, folder: &str, connection: Option<&str>) -> String {
    let body = json!({ "request_id": rid(), "template": "blank", "name": "Keys" });
    let (status, rec) = if hosted { signed(&s.router, "POST", "/api/teams", Some(body)).await } else { call(&s.router, "POST", "/api/teams", body).await };
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    let mut agent = json!({ "id": "w", "group_id": null, "name": "Writer", "kind": "claude", "job": "write", "folder": folder, "x": 40, "y": 40 });
    if let Some(c) = connection {
        agent["connection"] = json!(c);
    }
    draft["agents"] = json!([agent]);
    let id = draft["id"].as_str().unwrap().to_string();
    let put = json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft });
    let uri = format!("/api/teams/{id}");
    let (status, saved) = if hosted { signed(&s.router, "PUT", &uri, Some(put)).await } else { call(&s.router, "PUT", &uri, put).await };
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["draft"]["agents"][0]["connection"], connection.map_or(Value::Null, |c| json!(c)), "the connection is saved with the draft");
    id
}

/// Every text value in every table of the store, joined: what a leak would show up in.
fn whole_store(s: &Served) -> String {
    let store = s.st.store.lock().unwrap();
    let conn = store.connection();
    let tables: Vec<String> = {
        let mut q = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").unwrap();
        q.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    };
    let mut all = String::new();
    for t in tables {
        let mut q = conn.prepare(&format!("SELECT * FROM \"{t}\"")).unwrap();
        let n = q.column_count();
        let mut rows = q.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..n {
                match row.get_ref(i).unwrap() {
                    rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => all.push_str(&String::from_utf8_lossy(b)),
                    _ => {}
                }
                all.push('\n');
            }
        }
    }
    all
}

#[tokio::test]
async fn a_hosted_claude_agent_with_a_connection_gets_its_key_in_its_environment_and_nowhere_else() {
    let (url, asked) = fake_gateway().await;
    let (s, seen, _laptop) = hosted_with_gateway(&url).await;
    let id = claude_team(&s, true, &folder(&s), Some("conn-1")).await;
    let (status, body) = signed(&s.router, "POST", &format!("/api/teams/{id}/start"), Some(json!({ "request_id": rid() }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let w = seat(&id, "w");
    wait_for("the agent runs", || lifecycle(&s.st, &w) == Some(SeatLifecycle::Running)).await;
    assert_eq!(asked.lock().unwrap().clone(), ["conn-1"], "read once, for its own connection, with a signed request");
    let launches = seen.lock().unwrap().clone();
    assert_eq!(launches.len(), 1, "{:?}", launches.iter().map(|(s, _)| s).collect::<Vec<_>>());
    assert_eq!(launches[0].1, vec![("ANTHROPIC_API_KEY".to_string(), KEY.to_string())], "the key is in the agent's environment");

    let dump = whole_store(&s);
    assert!(dump.contains("conn-1"), "the store does hold the connection id, so this check reads the right place");
    assert!(!dump.contains(KEY) && !dump.contains("NEVER-STORE"), "the key is never stored or recorded");
    let snapshot = serde_json::to_string(&s.st.store.lock().unwrap().rig_snapshot(&id).unwrap()).unwrap();
    assert!(!snapshot.contains(KEY));
    let events = serde_json::to_string(&events(&s.st, &w)).unwrap();
    assert!(!events.contains(KEY), "{events}");
}

#[tokio::test]
async fn a_connection_that_cannot_be_read_keeps_the_agent_from_starting_and_says_why() {
    let (url, _asked) = fake_gateway().await;
    let (s, seen, _laptop) = hosted_with_gateway(&url).await;
    let id = claude_team(&s, true, &folder(&s), Some("conn-gone")).await;
    let (status, body) = signed(&s.router, "POST", &format!("/api/teams/{id}/start"), Some(json!({ "request_id": rid() }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let w = seat(&id, "w");
    wait_for("the agent needs attention", || lifecycle(&s.st, &w) == Some(SeatLifecycle::Attention)).await;
    assert!(seen.lock().unwrap().is_empty(), "it never launched");
    let reason = events(&s.st, &w)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Attention, reason } => reason,
            _ => None,
        })
        .last()
        .unwrap();
    assert!(reason.starts_with("Couldn't read the model connection for Writer: "), "{reason}");
    assert!(reason.ends_with("Check it under Setup → Models."), "{reason}");
}

#[tokio::test]
async fn a_hosted_agent_without_a_connection_uses_its_computers_own_sign_in() {
    let (url, asked) = fake_gateway().await;
    let (s, seen, _laptop) = hosted_with_gateway(&url).await;
    let id = claude_team(&s, true, &folder(&s), None).await;
    signed(&s.router, "POST", &format!("/api/teams/{id}/start"), Some(json!({ "request_id": rid() }))).await;
    wait_for("the agent runs", || lifecycle(&s.st, &seat(&id, "w")) == Some(SeatLifecycle::Running)).await;
    assert!(asked.lock().unwrap().is_empty(), "nothing to read");
    assert!(seen.lock().unwrap()[0].1.is_empty());
}

#[tokio::test]
async fn sculptured_on_your_own_computer_ignores_model_connections() {
    let s = serve().await;
    let seen: Seen = Default::default();
    let _runner = connect(&s, vec![Arc::new(RecordingClaude(seen.clone()))], None).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = claude_team(&s, false, &dir.path().display().to_string(), Some("conn-1")).await;
    start_team(&s, &id).await;
    let launches = seen.lock().unwrap().clone();
    assert_eq!(launches.len(), 1);
    assert!(launches[0].1.is_empty(), "local mode: the computer's own sign-in, no keys from anywhere");
}
