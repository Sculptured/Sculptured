use super::{
    Arc, BTreeSet, Body, Deserialize, Duration, HashMap, HeaderMap, HeaderValue, IntoResponse,
    Json, Mutex, Next, Path, Request, Response, Router, Serialize, State, StatusCode, Value, api,
    devices, json, middleware, seed,
};

/// What `POST /gw/api/device/token` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PairingMode {
    Pending,
    Approve,
    Deny,
    Expire,
}

/// One request as the stub received it (header names lower-case).
#[derive(Clone, Debug, Serialize)]
pub struct Seen {
    pub method: String,
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// A Community member in the stub.
#[derive(Clone, Debug)]
pub struct User {
    pub handle: Option<String>,
    pub name: String,
    pub bio: String,
    pub links: Vec<String>,
    pub avatar: bool,
    pub compact: bool,
    pub admin: bool,
    pub disabled: bool,
}

/// A published block in the stub.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: String,
    pub kind: &'static str,
    pub author: String,
    pub version: u32,
    pub title: String,
    pub description: String,
    pub content: Value,
    pub published: bool,
    pub hidden: bool,
}

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
    /// `POST /gw/api/device/revoke` answers 500 (the server cannot revoke right now).
    pub revoke_fails: bool,
}

impl Default for StubState {
    fn default() -> Self {
        Self {
            requests: Vec::new(),
            pairing: PairingMode::Pending,
            delay: Duration::ZERO,
            key_given: false,
            keys: HashMap::new(),
            issued: 0,
            you: seed::you(),
            others: vec![seed::ana()],
            blocks: seed::blocks(),
            saved: BTreeSet::new(),
            revoke_fails: false,
        }
    }
}

impl StubState {
    /// Who a request is: Ok(None) without `Authorization`, Ok(Some(())) for a live key, else the
    /// gateway's `401 device_unknown` (C4: a bad key is never treated as signed out).
    pub(crate) fn caller(&self, headers: &HeaderMap) -> Result<Option<()>, Response> {
        let Some(auth) = headers.get("authorization") else {
            return Ok(None);
        };
        let bearer = auth
            .to_str()
            .ok()
            .and_then(|a| a.strip_prefix("Bearer "))
            .unwrap_or_default();
        if self.keys.get(bearer).copied().unwrap_or(false) {
            Ok(Some(()))
        } else {
            Err(error(StatusCode::UNAUTHORIZED, "device_unknown"))
        }
    }
    pub(crate) fn issue(&mut self) -> (String, String) {
        self.issued += 1;
        let (id, key) = (
            format!("dev-{}", self.issued),
            format!("key{}-0123456789abcdef", self.issued),
        );
        self.keys.insert(format!("{id}.{key}"), true);
        (id, key)
    }
}

/// `{"error":{"code","message"}}`, as the gateway and Community answer.
pub(crate) fn error(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(json!({"error": {"code": code, "message": format!("stub: {code}")}})),
    )
        .into_response()
}

pub struct Stub {
    pub origin: String,
    pub state: Arc<Mutex<StubState>>,
}

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
        .route("/stub/reset", crate::community_stub::post(reset))
        .route(
            "/stub/pairing",
            crate::community_stub::post(set_pairing_route),
        )
        .route("/stub/delay", crate::community_stub::post(set_delay_route))
        .route(
            "/stub/blocks/{id}/bump",
            crate::community_stub::post(bump_route),
        )
        .route("/stub/requests", crate::community_stub::get(requests_route))
        .layer(middleware::from_fn_with_state(state, record))
        .with_state(shared)
}

pub(crate) type Shared = (Arc<Mutex<StubState>>, String);

/// Records every request but the control routes, then holds `/api/community/*` and token answers for `delay`.
pub(super) async fn record(
    State(state): State<Arc<Mutex<StubState>>>,
    req: Request,
    next: Next,
) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = crate::community_stub::to_bytes(body, 8 * 1024 * 1024)
        .await
        .unwrap_or_default();
    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let delay = {
        let mut s = state.lock().expect("state mutex must not be poisoned");
        if !path_and_query.starts_with("/stub/") {
            let headers = parts
                .headers
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().to_string(),
                        v.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect();
            s.requests.push(Seen {
                method: parts.method.to_string(),
                path_and_query: path_and_query.clone(),
                headers,
                body: bytes.to_vec(),
            });
        }
        s.delay
    };
    let community = path_and_query.starts_with("/api/community/");
    if (community || path_and_query == "/gw/api/device/token") && !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    let mut res = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    if community {
        let h = res.headers_mut();
        h.insert("set-cookie", HeaderValue::from_static("stub=1"));
        h.insert("x-upstream-secret", HeaderValue::from_static("s"));
        h.entry("cache-control")
            .or_insert(HeaderValue::from_static("no-store"));
    }
    res
}

pub(super) async fn reset(State((state, _)): State<Shared>) -> Json<Value> {
    *state.lock().expect("state mutex must not be poisoned") = StubState::default();
    Json(json!({"ok": true}))
}
#[derive(Deserialize)]
pub(super) struct ModeBody {
    pub(super) mode: PairingMode,
}
pub(super) async fn set_pairing_route(
    State((state, _)): State<Shared>,
    Json(b): Json<ModeBody>,
) -> Json<Value> {
    state
        .lock()
        .expect("state mutex must not be poisoned")
        .pairing = b.mode;
    Json(json!({"ok": true}))
}
#[derive(Deserialize)]
pub(super) struct DelayBody {
    pub(super) ms: u64,
}
pub(super) async fn set_delay_route(
    State((state, _)): State<Shared>,
    Json(b): Json<DelayBody>,
) -> Json<Value> {
    state
        .lock()
        .expect("state mutex must not be poisoned")
        .delay = Duration::from_millis(b.ms);
    Json(json!({"ok": true}))
}
pub(super) async fn bump_route(
    State((state, _)): State<Shared>,
    Path(id): Path<String>,
) -> Json<Value> {
    bump(&state, &id);
    Json(json!({"ok": true}))
}
pub(super) async fn requests_route(State((state, _)): State<Shared>) -> Json<Vec<Seen>> {
    Json(
        state
            .lock()
            .expect("state mutex must not be poisoned")
            .requests
            .clone(),
    )
}

pub(super) fn bump(state: &Mutex<StubState>, id: &str) {
    if let Some(b) = state
        .lock()
        .expect("state mutex must not be poisoned")
        .blocks
        .iter_mut()
        .find(|b| b.id == id)
    {
        b.version += 1;
    }
}

impl Stub {
    #[must_use]
    pub fn requests(&self) -> Vec<Seen> {
        self.state
            .lock()
            .expect("state mutex must not be poisoned")
            .requests
            .clone()
    }
    pub fn set_pairing(&self, m: PairingMode) {
        self.state
            .lock()
            .expect("state mutex must not be poisoned")
            .pairing = m;
    }
    /// Held before every `/api/community/*` and `/gw/api/device/token` answer.
    pub fn set_delay(&self, d: Duration) {
        self.state
            .lock()
            .expect("state mutex must not be poisoned")
            .delay = d;
    }
    /// A key for the user "you", as an approved pairing would give: (device_id, key).
    #[must_use]
    pub fn issue_key(&self) -> (String, String) {
        self.state
            .lock()
            .expect("state mutex must not be poisoned")
            .issue()
    }
    pub fn bump_version(&self, block_id: &str) {
        bump(&self.state, block_id);
    }
    pub fn set_revoke_failing(&self, fails: bool) {
        self.state
            .lock()
            .expect("state mutex must not be poisoned")
            .revoke_fails = fails;
    }
    /// How many keys the server still accepts.
    #[must_use]
    pub fn live_keys(&self) -> usize {
        self.state
            .lock()
            .expect("state mutex must not be poisoned")
            .keys
            .values()
            .filter(|live| **live)
            .count()
    }
    /// Every issued key now gets `401 device_unknown`.
    pub fn revoke_all(&self) {
        let mut state = self.state.lock().expect("state mutex must not be poisoned");
        for live in state.keys.values_mut() {
            *live = false;
        }
    }
}
