//! Model connections: a hosted Claude agent with a connection gets its keys from the gateway at
//! launch, in its launch environment only. A failed read keeps it from starting, in plain words; the
//! keys are never stored or recorded; local mode ignores connections. A stand-in gateway only, and no
//! model runs: what is checked is what Sculptured would launch with (`launch_params_with_connection`,
//! what every launch path sends) and what it records. The computer's Claude Code and OpenCode drivers
//! are the real ones with no CLI installed, so a launch that does go ahead fails on the computer.
mod common;
use axum::http::{HeaderMap, StatusCode};
use common::m2::*;
use common::wait_for;
use model::{EventBody, SeatLifecycle};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

const SECRET: &[u8] = b"instance-secret";
const USER: &str = "6f1c0b3e-1111-4222-8333-944455556666";
const KEY: &str = "sk-ant-api03-NEVER-STORE-THIS-0123456789";
const OR_KEY: &str = "sk-or-v1-NEVER-STORE-THIS-TOO-9876543210";

type Asked = Arc<Mutex<Vec<String>>>;

/// A stand-in gateway: answers `/internal/connection-env` for `conn-1` (an Anthropic key) and `conn-or`
/// (OpenRouter, in Claude Code's shape as the real gateway gives it) when the request is signed as this
/// instance's user, 404 for any other connection, 401 when the signature is wrong.
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
                if id == "conn-or" {
                    return (StatusCode::OK, axum::Json(json!({ "provider": "openrouter", "model": "deepseek/deepseek-chat-v3.1",
                        "env": [["ANTHROPIC_BASE_URL", "https://openrouter.ai/api"], ["ANTHROPIC_AUTH_TOKEN", OR_KEY],
                                ["ANTHROPIC_API_KEY", ""], ["ANTHROPIC_MODEL", "deepseek/deepseek-chat-v3.1"]] })));
                }
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

/// The real Claude Code and OpenCode drivers, pointed at CLIs this computer does not have.
fn uninstalled_drivers() -> Vec<Arc<dyn drivers::Driver>> {
    vec![
        Arc::new(drivers::claude::ClaudeStreamDriver {
            program: "/no/such/claude".into(),
        }),
        Arc::new(drivers::opencode::OpencodeAcpDriver {
            program: "/no/such/opencode".into(),
        }),
    ]
}

/// A hosted controld that reads model connections from the gateway at `url`, with a computer "laptop".
async fn hosted_with_gateway(url: &str) -> (Served, hostd::supervisor::Supervisor) {
    let mut s = serve_hosted().await;
    s.st = s.st.clone().with_gateway(controld::gateway::Gateway::new(
        url.to_string(),
        USER.to_string(),
        Arc::new(SECRET.to_vec()),
    ));
    s.router = controld::app(s.st.clone(), None);
    let laptop = connect_as(&s, "laptop", uninstalled_drivers()).await;
    (s, laptop)
}

/// A real folder on the computer.
fn folder(s: &Served) -> String {
    let dir = s.home.path().join("project");
    std::fs::create_dir_all(&dir).unwrap();
    dir.display().to_string()
}

/// What team `id`'s first agent would be launched with on `host`, connection included.
async fn launch_params(
    s: &Served,
    id: &str,
    host: &str,
) -> Result<host_proto::LaunchParams, String> {
    let draft = s.st.store.lock().unwrap().draft(id).unwrap().unwrap();
    let rig = rigspec::from_draft(&draft).unwrap().rig;
    controld::launch::launch_params_with_connection(&s.st, host, id, &rig.seats[0], s.home.path())
        .await
}

/// Starts team `id` (hosted, signed).
async fn start_signed(s: &Served, id: &str) {
    let (status, body) = signed(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        Some(json!({ "request_id": rid() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// Whether `seat` ever began a session (a launch that went ahead).
fn launched(s: &Served, seat: &str) -> bool {
    events(&s.st, seat)
        .iter()
        .any(|e| matches!(e.body, EventBody::TenureStarted { .. }))
}

/// The reason of `seat`'s latest move to `lifecycle`, waiting for one.
async fn reason_for(s: &Served, seat: &str, lifecycle: SeatLifecycle) -> String {
    let find = || {
        events(&s.st, seat)
            .into_iter()
            .filter_map(|e| match e.body {
                EventBody::SeatLifecycleChanged {
                    lifecycle: l,
                    reason,
                } if l == lifecycle => Some(reason.unwrap_or_default()),
                _ => None,
            })
            .last()
    };
    wait_for("the agent's lifecycle", || find().is_some()).await;
    find().unwrap()
}

/// A team with one Claude agent "Writer" using `connection`; returns its id.
async fn claude_team(s: &Served, hosted: bool, folder: &str, connection: Option<&str>) -> String {
    team_of(s, hosted, folder, connection, "claude").await
}

/// A team with one agent "Writer" of `kind` using `connection`; returns its id.
async fn team_of(
    s: &Served,
    hosted: bool,
    folder: &str,
    connection: Option<&str>,
    kind: &str,
) -> String {
    let body = json!({ "request_id": rid(), "template": "blank", "name": "Keys" });
    let (status, rec) = if hosted {
        signed(&s.router, "POST", "/api/teams", Some(body)).await
    } else {
        call(&s.router, "POST", "/api/teams", body).await
    };
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    let mut agent = json!({ "id": "w", "group_id": null, "name": "Writer", "kind": kind, "job": "write", "folder": folder, "x": 40, "y": 40 });
    if let Some(c) = connection {
        agent["connection"] = json!(c);
    }
    draft["agents"] = json!([agent]);
    let id = draft["id"].as_str().unwrap().to_string();
    let put = json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft });
    let uri = format!("/api/teams/{id}");
    let (status, saved) = if hosted {
        signed(&s.router, "PUT", &uri, Some(put)).await
    } else {
        call(&s.router, "PUT", &uri, put).await
    };
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["draft"]["agents"][0]["connection"],
        connection.map_or(Value::Null, |c| json!(c)),
        "the connection is saved with the draft"
    );
    id
}

/// Every text value in every table of the store, joined: what a leak would show up in.
fn whole_store(s: &Served) -> String {
    let store = s.st.store.lock().unwrap();
    let conn = store.connection();
    let tables: Vec<String> = {
        let mut q = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap();
        q.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let mut all = String::new();
    for t in tables {
        let mut q = conn.prepare(&format!("SELECT * FROM \"{t}\"")).unwrap();
        let n = q.column_count();
        let mut rows = q.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for i in 0..n {
                match row.get_ref(i).unwrap() {
                    rusqlite::types::ValueRef::Text(b) | rusqlite::types::ValueRef::Blob(b) => {
                        all.push_str(&String::from_utf8_lossy(b))
                    }
                    _ => {}
                }
                all.push('\n');
            }
        }
    }
    all
}

#[tokio::test]
async fn a_hosted_claude_agent_with_a_connection_gets_its_key_in_its_environment_and_nowhere_else()
{
    let (url, asked) = fake_gateway().await;
    let (s, _laptop) = hosted_with_gateway(&url).await;
    let id = claude_team(&s, true, &folder(&s), Some("conn-1")).await;
    let w = seat(&id, "w");
    // A real start: the key is read once, sent with the launch, and the launch fails on the computer.
    start_signed(&s, &id).await;
    reason_for(&s, &w, SeatLifecycle::Attention).await;
    assert_eq!(
        asked.lock().unwrap().clone(),
        ["conn-1"],
        "read once, for its own connection, with a signed request"
    );
    let params = launch_params(&s, &id, "laptop").await.unwrap();
    assert_eq!(
        params.env.0,
        vec![("ANTHROPIC_API_KEY".to_string(), KEY.to_string())],
        "the key is in the agent's environment"
    );

    let dump = whole_store(&s);
    assert!(
        dump.contains("conn-1"),
        "the store does hold the connection id, so this check reads the right place"
    );
    assert!(
        !dump.contains(KEY) && !dump.contains("NEVER-STORE"),
        "the key is never stored or recorded"
    );
    let snapshot =
        serde_json::to_string(&s.st.store.lock().unwrap().rig_snapshot(&id).unwrap()).unwrap();
    assert!(!snapshot.contains(KEY));
    let events = serde_json::to_string(&events(&s.st, &w)).unwrap();
    assert!(!events.contains(KEY), "{events}");
}

#[tokio::test]
async fn a_hosted_opencode_agent_with_an_openrouter_connection_gets_the_key_as_openrouter_api_key()
{
    let (url, asked) = fake_gateway().await;
    let (s, _laptop) = hosted_with_gateway(&url).await;
    let id = team_of(&s, true, &folder(&s), Some("conn-or"), "opencode").await;
    let params = launch_params(&s, &id, "laptop").await.unwrap();
    assert_eq!(asked.lock().unwrap().clone(), ["conn-or"]);
    assert_eq!(
        params.env.0,
        vec![("OPENROUTER_API_KEY".to_string(), OR_KEY.to_string())],
        "only the key, under the name OpenCode reads"
    );
    assert_eq!(
        params.model.as_deref(),
        Some("openrouter/deepseek/deepseek-chat-v3.1")
    );
    assert!(
        !whole_store(&s).contains(OR_KEY),
        "the key is never stored or recorded"
    );
}

#[tokio::test]
async fn an_opencode_agent_with_an_anthropic_key_connection_does_not_start_and_says_why() {
    let (url, _asked) = fake_gateway().await;
    let (s, _laptop) = hosted_with_gateway(&url).await;
    let id = team_of(&s, true, &folder(&s), Some("conn-1"), "opencode").await;
    start_signed(&s, &id).await;
    let w = seat(&id, "w");
    wait_for("the launch is refused in plain words", || {
        events(&s.st, &w).iter().any(|e| {
            serde_json::to_string(e)
                .unwrap()
                .contains("OpenCode agents need an OpenRouter connection")
        })
    })
    .await;
    assert!(!launched(&s, &w), "never launched");
}

#[tokio::test]
async fn a_connection_that_cannot_be_read_keeps_the_agent_from_starting_and_says_why() {
    let (url, _asked) = fake_gateway().await;
    let (s, _laptop) = hosted_with_gateway(&url).await;
    let id = claude_team(&s, true, &folder(&s), Some("conn-gone")).await;
    start_signed(&s, &id).await;
    let w = seat(&id, "w");
    let reason = reason_for(&s, &w, SeatLifecycle::Attention).await;
    assert!(!launched(&s, &w), "it never launched");
    assert!(
        reason.starts_with("Couldn't read the model connection for Writer: "),
        "{reason}"
    );
    assert!(
        reason.ends_with("Check it under Setup → Models."),
        "{reason}"
    );
}

#[tokio::test]
async fn a_hosted_agent_without_a_connection_uses_its_computers_own_sign_in() {
    let (url, asked) = fake_gateway().await;
    let (s, _laptop) = hosted_with_gateway(&url).await;
    let id = claude_team(&s, true, &folder(&s), None).await;
    let params = launch_params(&s, &id, "laptop").await.unwrap();
    assert!(asked.lock().unwrap().is_empty(), "nothing to read");
    assert!(params.env.0.is_empty());
}

#[tokio::test]
async fn sculptured_on_your_own_computer_ignores_model_connections() {
    let s = serve().await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = claude_team(&s, false, &dir.path().display().to_string(), Some("conn-1")).await;
    let params = launch_params(&s, &id, controld::hosts::LOCAL_HOST)
        .await
        .unwrap();
    assert!(
        params.env.0.is_empty(),
        "local mode: the computer's own sign-in, no keys from anywhere"
    );
}
