//! Explicit project-scoped Git operations. The work is `localops::git` (shared with connected
//! computers); this module finds the project folder, serializes actions per team and keeps requests
//! idempotent.
use crate::error::ApiError;
use crate::extract::{Json, Path, Query};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
pub use localops::git::{GitFile, GitSnapshot};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/git", axum::routing::get(status))
        .route("/api/teams/{id}/git/diff", axum::routing::get(diff))
        .route("/api/teams/{id}/git/action", axum::routing::post(action))
}
fn fail(s: impl Into<String>) -> ApiError {
    ApiError::bad_request("git_failed", s)
}
fn folder(st: &AppState, id: &str) -> Result<PathBuf, ApiError> {
    let draft = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .draft(id)?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;
    let f = draft
        .folder
        .ok_or_else(|| fail("Choose a project folder first"))?;
    let home = crate::folders::home(st)?;
    let path = std::fs::canonicalize(f).map_err(|_| fail("Project folder unavailable"))?;
    if !path.starts_with(home.real) {
        return Err(fail("Project folder must be inside your home folder"));
    }
    Ok(path)
}

/// The repository to work on: here, the checked project folder; hosted, the team's folder as saved
/// (the computer resolves and checks it within its allowed folders).
fn repo(st: &AppState, id: &str) -> Result<String, ApiError> {
    if crate::ops::hosted(st) {
        let draft = st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .draft(id)?
            .ok_or_else(|| ApiError::not_found("Project not found"))?;
        return Ok(draft.folder.unwrap_or_default());
    }
    Ok(folder(st, id)?.display().to_string())
}

async fn status(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = repo(&st, &id)?;
    Ok(Json(
        crate::ops::run(
            &st,
            crate::ops::Target::Team(&id),
            localops::LocalOp::GitStatus { repo },
        )
        .await?,
    ))
}

#[derive(Deserialize)]
struct Diff {
    path: String,
    #[serde(default)]
    staged: bool,
}

async fn diff(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<Diff>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let repo = repo(&st, &id)?;
    Ok(Json(
        crate::ops::run(
            &st,
            crate::ops::Target::Team(&id),
            localops::LocalOp::GitDiff {
                repo,
                path: q.path,
                staged: q.staged,
            },
        )
        .await?,
    ))
}

#[derive(Deserialize, Serialize)]
struct Action {
    request_id: String,
    #[serde(flatten)]
    action: localops::git::GitAction,
}

async fn action(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<Action>,
) -> axum::response::Response {
    let fp = crate::idempotent::fingerprint(&format!("git:{id}"), &req);
    let request = req.request_id.clone();
    let state = st.clone();
    crate::idempotent::idempotent(&st, &request, fp, move || async move {
        let _claim = state
            .try_begin_boot(&format!("git:{id}"))
            .ok_or_else(|| ApiError::conflict("git_busy", "Another Git operation is running"))?;
        let repo = repo(&state, &id)?;
        crate::ops::run(
            &state,
            crate::ops::Target::Team(&id),
            localops::LocalOp::GitAct {
                repo,
                action: req.action,
            },
        )
        .await
    })
    .await
}
