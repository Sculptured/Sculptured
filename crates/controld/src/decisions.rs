//! The decision log (`TeamDraft.decision_log`): precedents people record (written directly, or from a
//! decision card), which agents read with `read_decisions`. Agents can't write to it. Entries are never
//! edited; hiding one keeps it and records when.
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::{Value, json};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/teams/{id}/decisions",
            axum::routing::get(list).post(write),
        )
        .route(
            "/api/teams/{id}/decisions/{entry}/hide",
            axum::routing::post(hide_route),
        )
}

const MAX_TEXT: usize = 4096;

async fn list(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::error::respond(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .decisions(&id, None, 200, false)
            .map(|e| json!(e))
            .map_err(ApiError::from),
    )
}

#[derive(Deserialize)]
struct Write {
    text: String,
}

async fn write(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<Write>,
) -> Response {
    crate::error::respond(add(&st, &id, &req.text, "person").map(|entry| json!({ "id": entry })))
}

async fn hide_route(
    State(st): State<AppState>,
    Path((id, entry)): Path<(String, String)>,
) -> Response {
    crate::error::respond(hide(&st, &id, &entry).map(|_| json!({ "hidden": entry })))
}

/// Adds a person's entry (`source` "person" or "decision").
pub fn add(st: &AppState, team: &str, text: &str, source: &str) -> Result<String, ApiError> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_TEXT {
        return Err(ApiError::bad_request(
            "decision_invalid",
            "Write 1–4096 bytes.",
        ));
    }
    let store = st.store.lock().expect("store mutex must not be poisoned");
    store
        .draft(team)?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;
    Ok(store.add_decision(team, "a person", text, source)?)
}

pub fn hide(st: &AppState, team: &str, entry: &str) -> Result<(), ApiError> {
    if st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .hide_decision(team, entry)?
    {
        Ok(())
    } else {
        Err(ApiError::not_found(
            "No such entry, or it is already hidden",
        ))
    }
}

/// The `read_decisions` seat tool: the latest 50 visible entries, or those matching `query`.
pub fn read_tool(st: &AppState, seat: &str, args: &Value) -> Result<Value, String> {
    let team = crate::seat_ids::rig_of(seat).ok_or("Unknown agent.")?;
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let on = store
        .draft(team)
        .map_err(|e| e.to_string())?
        .is_some_and(|d| d.decision_log);
    if !on {
        return Ok(json!({ "text": "The decision log is off for this team." }));
    }
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty());
    let entries = store
        .decisions(team, query, 50, false)
        .map_err(|e| e.to_string())?;
    let lines: Vec<Value> = entries
        .iter()
        .map(
            |e| json!({ "at_ms": e.at_ms, "author": e.author, "source": e.source, "text": e.text }),
        )
        .collect();
    Ok(json!({ "text": serde_json::to_string(&lines).map_err(|e| e.to_string())? }))
}
