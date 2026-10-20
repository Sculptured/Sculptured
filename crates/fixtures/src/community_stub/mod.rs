//! A stand-in for the Sculptured server (gateway device pairing plus Community's `/api/community/*`)
//! for controld tests and e2e. It records every request, answers like the real server in the cases
//! the app handles, and has control routes (`/stub/*`) for Playwright. No real network.
// A refusal is an axum `Response` (a test fixture: its size does not matter).
#![allow(clippy::result_large_err)]
mod api;
mod devices;
mod seed;

use axum::body::{Body, to_bytes};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use seed::{SEED_BLOCK, SEED_OTHER_KIND};

/// What `POST /gw/api/device/token` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PairingMode { Pending, Approve, Deny, Expire }

/// One request as the stub received it (header names lower-case).
#[derive(Clone, Debug, Serialize)]
pub struct Seen { pub method: String, pub path_and_query: String, pub headers: Vec<(String, String)>, pub body: Vec<u8> }

/// A Community member in the stub.
#[derive(Clone, Debug)]
pub struct User { pub handle: Option<String>, pub name: String, pub bio: String, pub links: Vec<String>, pub avatar: bool, pub compact: bool, pub admin: bool, pub disabled: bool }

/// A published block in the stub.
#[derive(Clone, Debug)]
pub struct Block { pub id: String, pub kind: &'static str, pub author: String, pub version: u32, pub title: String, pub description: String, pub content: Value, pub published: bool, pub hidden: bool }

pub struct StubState {
    pub requests: Vec<Seen>,
    pub pairing: PairingMode,
    pub delay: Duration,
    /// Whether the approved pairing already handed out its key (a code works once).
    pub key_given: bool,
    /// Bearer (`<device_id>.<key>`) → still accepted.
    pub keys: HashMap<String, bool>,
    pub issued: u32,
    /// The paired user "you"; `others` are everyone else (ana).
    pub you: User,
    pub others: Vec<User>,
    pub blocks: Vec<Block>,
    pub saved: BTreeSet<String>,
}

impl Default for StubState {
    fn default() -> Self {
        Self {
            requests: Vec::new(), pairing: PairingMode::Pending, delay: Duration::ZERO, key_given: false, keys: HashMap::new(), issued: 0,
            you: seed::you(), others: vec![seed::ana()], blocks: seed::blocks(), saved: BTreeSet::new(),
        }
    }
}

impl StubState {
    /// Who a request is: Ok(None) without `Authorization`, Ok(Some(())) for a live key, else the
    /// gateway's `401 device_unknown` (C4: a bad key is never treated as signed out).
    pub(crate) fn caller(&self, headers: &HeaderMap) -> Result<Option<()>, Response> {
        let Some(auth) = headers.get("authorization") else { return Ok(None) };
        let bearer = auth.to_str().ok().and_then(|a| a.strip_prefix("Bearer ")).unwrap_or_default();
        if self.keys.get(bearer).copied().unwrap_or(false) { Ok(Some(())) } else { Err(error(StatusCode::UNAUTHORIZED, "device_unknown")) }
    }
    pub(crate) fn issue(&mut self) -> (String, String) {
        self.issued += 1;
        let (id, key) = (format!("dev-{}", self.issued), format!("key{}-0123456789abcdef", self.issued));
        self.keys.insert(format!("{id}.{key}"), true);
        (id, key)
    }
}

/// `{"error":{"code","message"}}`, as the gateway and Community answer.
pub(crate) fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error": {"code": code, "message": format!("stub: {code}")}}))).into_response()
}

pub struct Stub { pub origin: String, pub state: Arc<Mutex<StubState>> }

/// A stub on `127.0.0.1` at a free port.
pub async fn spawn() -> Stub {
    spawn_on(0).await.expect("bind the stub")
}

pub async fn spawn_on(port: u16) -> std::io::Result<Stub> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let state = Arc::new(Mutex::new(StubState::default()));
    let app = router(state.clone(), origin.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.ok() });
    Ok(Stub { origin, state })
}

pub fn router(state: Arc<Mutex<StubState>>, origin: String) -> Router {
    let shared = (state.clone(), origin);
    Router::new()
        .merge(devices::routes())
        .merge(api::routes())
        .route("/stub/reset", post(reset))
        .route("/stub/pairing", post(set_pairing_route))
        .route("/stub/delay", post(set_delay_route))
        .route("/stub/blocks/{id}/bump", post(bump_route))
        .route("/stub/requests", get(requests_route))
        .layer(middleware::from_fn_with_state(state, record))
        .with_state(shared)
}

pub(crate) type Shared = (Arc<Mutex<StubState>>, String);

/// Records every request but the control routes, then holds `/api/community/*` answers for `delay`.
async fn record(State(state): State<Arc<Mutex<StubState>>>, req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = to_bytes(body, 8 * 1024 * 1024).await.unwrap_or_default();
    let path_and_query = parts.uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let delay = {
        let mut s = state.lock().unwrap();
        if !path_and_query.starts_with("/stub/") {
            let headers = parts.headers.iter().map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or_default().to_string())).collect();
            s.requests.push(Seen { method: parts.method.to_string(), path_and_query: path_and_query.clone(), headers, body: bytes.to_vec() });
        }
        s.delay
    };
    let community = path_and_query.starts_with("/api/community/");
    if community && !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let mut res = next.run(Request::from_parts(parts, Body::from(bytes))).await;
    if community {
        let h = res.headers_mut();
        h.insert("set-cookie", HeaderValue::from_static("stub=1"));
        h.insert("x-upstream-secret", HeaderValue::from_static("s"));
        h.entry("cache-control").or_insert(HeaderValue::from_static("no-store"));
    }
    res
}

async fn reset(State((state, _)): State<Shared>) -> Json<Value> {
    *state.lock().unwrap() = StubState::default();
    Json(json!({"ok": true}))
}
#[derive(Deserialize)]
struct ModeBody { mode: PairingMode }
async fn set_pairing_route(State((state, _)): State<Shared>, Json(b): Json<ModeBody>) -> Json<Value> {
    state.lock().unwrap().pairing = b.mode;
    Json(json!({"ok": true}))
}
#[derive(Deserialize)]
struct DelayBody { ms: u64 }
async fn set_delay_route(State((state, _)): State<Shared>, Json(b): Json<DelayBody>) -> Json<Value> {
    state.lock().unwrap().delay = Duration::from_millis(b.ms);
    Json(json!({"ok": true}))
}
async fn bump_route(State((state, _)): State<Shared>, Path(id): Path<String>) -> Json<Value> {
    bump(&state, &id);
    Json(json!({"ok": true}))
}
async fn requests_route(State((state, _)): State<Shared>) -> Json<Vec<Seen>> {
    Json(state.lock().unwrap().requests.clone())
}

fn bump(state: &Mutex<StubState>, id: &str) {
    if let Some(b) = state.lock().unwrap().blocks.iter_mut().find(|b| b.id == id) {
        b.version += 1;
    }
}

impl Stub {
    pub fn requests(&self) -> Vec<Seen> {
        self.state.lock().unwrap().requests.clone()
    }
    pub fn set_pairing(&self, m: PairingMode) {
        self.state.lock().unwrap().pairing = m;
    }
    /// Held before every `/api/community/*` answer.
    pub fn set_delay(&self, d: Duration) {
        self.state.lock().unwrap().delay = d;
    }
    /// A key for the user "you", as an approved pairing would give: (device_id, key).
    pub fn issue_key(&self) -> (String, String) {
        self.state.lock().unwrap().issue()
    }
    pub fn bump_version(&self, block_id: &str) {
        bump(&self.state, block_id);
    }
    /// Every issued key now gets `401 device_unknown`.
    pub fn revoke_all(&self) {
        self.state.lock().unwrap().keys.values_mut().for_each(|live| *live = false);
    }
}
