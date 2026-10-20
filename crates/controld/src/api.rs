use crate::error::ApiError;
use crate::state::AppState;
use crate::store::RigSource;
use crate::views::StopRigOutcome;
pub(crate) use crate::idempotent::{fingerprint, idempotent};
use crate::{guard, hosts, reconciler, stream};
use crate::extract::{Json, Path, Query};
use axum::extract::State;
use axum::http::Uri;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::Router;
use model::MessageIntent;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

/// `fallback` serves every unmatched non-API path (Task 12: the web UI). It is
/// attached before the guard layer, so it is guarded like the API.
pub fn app(st: AppState, fallback: Option<Router>) -> Router {
    // Agents hand work over whether or not anyone is watching; started once per state.
    crate::handoffs::ensure_running(&st);
    crate::background::ensure_running(&st);
    let mut router = routes();
    if let Some(fallback) = fallback {
        router = router.fallback_service(fallback);
    }
    // Order matters: the guard (outermost) rejects foreign hosts first, then the auth seam.
    router
        .layer(middleware::from_fn_with_state(st.clone(), crate::auth::auth))
        .layer(middleware::from_fn_with_state(st.clone(), guard::guard))
        .with_state(st)
}

/// Every API route, the host link, and the API's own 404 and 405.
fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/rigs", get(list_rigs).post(boot_rig))
        .route("/api/rigs/{name}", get(get_rig))
        .route("/api/rigs/{name}/stop", post(stop_rig))
        .route("/api/seats/{id}/events", get(seat_events))
        .route("/api/seats/{id}/send", post(send))
        .route("/api/seats/{id}/interrupt", post(interrupt))
        .route("/api/seats/{id}/stop", post(stop_seat))
        .route("/api/seats/{id}/restart", post(restart_seat))
        .route("/api/seats/{id}/handover", post(hand_over_again))
        .route("/api/seats/{id}/resend", post(resend_handoffs))
        .route("/api/stream", get(stream::stream))
        .merge(crate::teams::routes())
        .merge(crate::group_blocks::routes())
        .merge(crate::project_visibility::routes())
        .merge(crate::collaboration::routes())
        .merge(crate::workflow::routes())
        .merge(crate::schedules::routes())
        .merge(crate::decisions::routes())
        .merge(crate::knowledge::routes())
        .merge(crate::folders::routes())
        .merge(crate::questions::routes())
        .merge(crate::settings::routes())
        .merge(crate::git::routes())
        .merge(crate::monitor::routes())
        .merge(crate::restore::routes())
        .merge(crate::clis::routes())
        .merge(crate::plan_usage::routes())
        .merge(crate::usage::routes())
        .merge(crate::run_reports::routes())
        .merge(crate::recent::routes())
        .merge(crate::budget::pause::routes())
        .merge(crate::budget::raise::routes())
        .merge(crate::traces::settings::routes())
        .merge(crate::account::routes())
        .merge(crate::copies::routes())
        .merge(crate::checks::routes())
        .merge(crate::portable::routes())
        .merge(crate::generate::routes())
        .merge(crate::community::routes())
        .route("/api", any(api_not_found))
        .route("/api/", any(api_not_found))
        .route("/api/{*rest}", any(api_not_found))
        .route("/host", get(hosts::host_ws))
        .route("/api/hosts", get(list_hosts))
        .route("/api/hosts/{id}/limit", axum::routing::put(set_host_limit))
        .method_not_allowed_fallback(crate::extract::method_not_allowed)
}

async fn api_not_found(uri: Uri) -> ApiError {
    ApiError::not_found(format!("no API route {}", uri.path()))
}

#[derive(Deserialize, Serialize)]
struct BootReq {
    #[serde(default)]
    request_id: String,
    spec_path: String,
}

#[derive(Deserialize, Serialize)]
struct IdOnly {
    #[serde(default)]
    request_id: String,
}

#[derive(Deserialize, Serialize)]
struct SendReq {
    #[serde(default)]
    request_id: String,
    text: String,
    intent: MessageIntent,
    #[serde(default)]
    interrupt: bool,
}

#[derive(Deserialize)]
struct EventsQuery {
    after: Option<i64>,
    limit: Option<usize>,
    /// The newest N events instead of a page after `after`.
    tail: Option<usize>,
}


async fn list_rigs(State(st): State<AppState>) -> Response {
    let rigs = st.store.lock().unwrap().list_rigs();
    match rigs {
        Ok(r) => Json(r).into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

async fn get_rig(State(st): State<AppState>, Path(name): Path<String>) -> Response {
    let snap = st.store.lock().unwrap().rig_snapshot(&name);
    match snap {
        Ok(Some(s)) => Json(s).into_response(),
        Ok(None) => ApiError::not_found(format!("no rig {name:?}")).into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

async fn boot_rig(State(st): State<AppState>, Json(req): Json<BootReq>) -> Response {
    let fp = fingerprint("boot", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move {
        let path = PathBuf::from(&req.spec_path);
        let yaml = std::fs::read_to_string(&path)
            .map_err(|e| ApiError::bad_request("spec_unreadable", format!("cannot read {}: {e}", path.display())))?;
        let rig = rigspec::compile(&yaml).map_err(|d| ApiError::unprocessable("spec_invalid", "the rig spec has errors", d))?;
        let spec_dir = path
            .canonicalize()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        reconciler::start_rig(&st2, rig, RigSource::File { path: req.spec_path.clone() }, spec_dir)
    })
    .await
}

async fn stop_rig(State(st): State<AppState>, Path(name): Path<String>, Json(req): Json<IdOnly>) -> Response {
    let fp = fingerprint(&format!("stop-rig:{name}"), &req);
    let st2 = st.clone();
    idempotent(&st, &req.request_id, fp, move || async move {
        let failed = reconciler::stop_rig(st2, name).await?;
        Ok::<Value, ApiError>(serde_json::to_value(StopRigOutcome { failed }).expect("outcome serializes"))
    })
    .await
}

async fn seat_events(State(st): State<AppState>, Path(id): Path<String>, Query(q): Query<EventsQuery>) -> Response {
    let events = {
        let store = st.store.lock().unwrap();
        match q.tail {
            Some(n) => store.latest_events_for_seat(&id, n.min(2000)),
            None => store.events_for_seat(&id, q.after.unwrap_or(0), q.limit.unwrap_or(500).min(2000)),
        }
    };
    match events {
        Ok(e) => Json(e).into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

async fn send(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<SendReq>) -> Response {
    let fp = fingerprint(&format!("send:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move {
        let message_id = reconciler::send_message(&st2, &id, req.text, req.intent, req.interrupt).await?;
        Ok::<Value, ApiError>(json!({ "message_id": message_id }))
    })
    .await
}

async fn interrupt(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<IdOnly>) -> Response {
    let fp = fingerprint(&format!("interrupt:{id}"), &req);
    let st2 = st.clone();
    idempotent(&st, &req.request_id, fp, move || async move {
        reconciler::interrupt_seat(&st2, &id).await?;
        Ok::<Value, ApiError>(json!({}))
    })
    .await
}

/// "Restart this agent": relaunch one agent of a running team, then send what it is owed.
async fn restart_seat(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<IdOnly>) -> Response {
    let fp = fingerprint(&format!("restart-seat:{id}"), &req);
    let st2 = st.clone();
    idempotent(&st, &req.request_id, fp, move || async move {
        reconciler::restart_seat(&st2, &id).await?;
        Ok::<Value, ApiError>(json!({}))
    })
    .await
}

/// "Hand over again": pass the agent's latest finished turn to its downstream agents once.
async fn hand_over_again(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<IdOnly>) -> Response {
    let fp = fingerprint(&format!("handover:{id}"), &req);
    let st2 = st.clone();
    idempotent(&st, &req.request_id, fp, move || async move {
        crate::handoffs::request_handover(&st2, &id)?;
        Ok::<Value, ApiError>(json!({}))
    })
    .await
}

/// Re-send the agent's handoffs that never reached it.
async fn resend_handoffs(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<IdOnly>) -> Response {
    let fp = fingerprint(&format!("resend:{id}"), &req);
    let st2 = st.clone();
    idempotent(&st, &req.request_id, fp, move || async move {
        let resent = crate::handoffs::resend(&st2, &id).await?;
        Ok::<Value, ApiError>(json!({ "resent": resent }))
    })
    .await
}

async fn stop_seat(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<IdOnly>) -> Response {
    let fp = fingerprint(&format!("stop-seat:{id}"), &req);
    let st2 = st.clone();
    idempotent(&st, &req.request_id, fp, move || async move {
        reconciler::stop_seat(&st2, &id).await?;
        Ok::<Value, ApiError>(json!({}))
    })
    .await
}

/// Connected computers (agent runners) and what each can run.
async fn list_hosts(State(st): State<AppState>) -> axum::Json<serde_json::Value> {
    let limit = |id: &str| st.store.lock().unwrap().host_limit(id).ok().flatten();
    axum::Json(serde_json::Value::Array(st.hosts.connected().into_iter().map(|id| serde_json::json!({"drivers": st.hosts.drivers(&id), "policy": st.hosts.policy(&id), "max_working": limit(&id), "id": id})).collect()))
}

#[derive(serde::Deserialize)]
struct HostLimit {
    max_working: Option<u32>,
}

/// `PUT /api/hosts/{id}/limit`: most agents working at once on that computer, across its teams (none
/// = no cap). Held work waits for a slot, as with a team's own cap.
async fn set_host_limit(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<HostLimit>) -> Result<axum::Json<serde_json::Value>, ApiError> {
    if req.max_working == Some(0) || req.max_working.is_some_and(|n| n > 100) {
        return Err(ApiError::bad_request("bad_limit", "The cap is 1 to 100 agents, or none."));
    }
    st.store.lock().unwrap().set_host_limit(&id, req.max_working)?;
    Ok(axum::Json(serde_json::json!({ "id": id, "max_working": req.max_working })))
}
