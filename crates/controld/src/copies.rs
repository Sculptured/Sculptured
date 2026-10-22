//! Agents' own copies (routes and settings). The git work is `localops::copies`, shared with connected
//! computers; this module resolves the copies folder (the `copies_folder` setting, default
//! `~/.sculptured/copies`), the project folder and which agents are running.
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
pub use localops::copies::{AgentCopies, AgentCopy, CopySeat, branch_of};
use model::SeatLifecycle;
use serde::Deserialize;
use std::path::{Path as FsPath, PathBuf};

#[derive(Deserialize)]
struct CopyAction {
    #[allow(dead_code)]
    request_id: String,
    seat: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/copies", axum::routing::get(list))
        .route("/api/teams/{id}/copies/merge", axum::routing::post(merge))
        .route("/api/teams/{id}/copies/remove", axum::routing::post(remove))
}

/// Whether the team gives its agents their own copies.
#[must_use]
pub fn enabled(st: &AppState, team: &str) -> bool {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .team_settings(team)
        .map(|t| t.own_copies)
        .unwrap_or(false)
}

/// The folder holding every copy. A chosen folder must already exist: on a drive that is not mounted,
/// making it would quietly fill the system disk instead.
pub fn copies_root(st: &AppState) -> Result<PathBuf, String> {
    let chosen = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .settings()
        .ok()
        .and_then(|s| s.copies_folder)
        .filter(|f| !f.trim().is_empty());
    match chosen {
        Some(f) if !FsPath::new(&f).is_dir() => Err(format!(
            "The copies folder {f} is not available. Is its drive connected?"
        )),
        Some(f) => Ok(PathBuf::from(f)),
        None => Ok(st
            .home
            .as_deref()
            .ok_or("Your home folder is unknown (HOME is not set)")?
            .join(".sculptured/copies")),
    }
}

/// The folder `seat` should run in: its own copy (made on first launch). Err in plain words.
pub fn prepare(st: &AppState, team: &str, seat: &str, cwd: &FsPath) -> Result<PathBuf, String> {
    let root = copies_root(st)?;
    let parent = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .parent(seat)
        .ok()
        .flatten();
    localops::copies::prepare(&root, team, seat, parent.as_deref(), cwd).map_err(|e| e.message)
}

fn project_folder(st: &AppState, team: &str) -> Result<PathBuf, ApiError> {
    let draft = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .draft(team)?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;
    draft
        .folder
        .map(PathBuf::from)
        .ok_or_else(|| ApiError::bad_request("git_failed", "Choose a project folder first"))
}

/// The team's agents (spawned ones included) and whether each is running.
pub fn team_seats(st: &AppState, team: &str) -> Result<Vec<CopySeat>, ApiError> {
    let ids: Vec<String> = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let mut stmt = store
            .connection()
            .prepare("SELECT id FROM seats WHERE rig=?1 ORDER BY id")?;
        stmt.query_map([team], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(ids
        .into_iter()
        .map(|seat| CopySeat {
            running: crate::reconciler::lifecycle(st, &seat) == Some(SeatLifecycle::Running),
            seat,
        })
        .collect())
}

async fn read(st: &AppState, team: &str) -> Result<serde_json::Value, ApiError> {
    if st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .draft(team)?
        .is_none()
    {
        return Err(ApiError::not_found("Project not found"));
    }
    let op = localops::LocalOp::CopiesRead {
        project: project_folder(st, team)
            .ok()
            .map(|p| p.display().to_string()),
        team: team.to_string(),
        seats: team_seats(st, team)?,
        enabled: enabled(st, team),
    };
    crate::ops::run(st, crate::ops::Target::Team(team), op).await
}

async fn list(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(read(&st, &id).await?))
}

fn one_seat(st: &AppState, team: &str, seat: &str) -> Result<CopySeat, ApiError> {
    if crate::seat_ids::rig_of(seat) != Some(team) {
        return Err(ApiError::not_found(format!(
            "no agent {seat:?} in this project"
        )));
    }
    Ok(CopySeat {
        seat: seat.to_string(),
        running: crate::reconciler::lifecycle(st, seat) == Some(SeatLifecycle::Running),
    })
}

/// Commits the copy's uncommitted work on the agent's branch, then merges it (see `localops::copies::merge`).
async fn merge(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CopyAction>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let seat = one_seat(&st, &id, &req.seat)?;
    let project = project_folder(&st, &id)?.display().to_string();
    crate::ops::run(
        &st,
        crate::ops::Target::Team(&id),
        localops::LocalOp::CopyMerge {
            project,
            team: id.clone(),
            seat,
        },
    )
    .await?;
    Ok(Json(read(&st, &id).await?))
}

/// Removes the copy's folder once it has no uncommitted work; its branch is kept.
async fn remove(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<CopyAction>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let seat = one_seat(&st, &id, &req.seat)?;
    let project = project_folder(&st, &id)?.display().to_string();
    crate::ops::run(
        &st,
        crate::ops::Target::Team(&id),
        localops::LocalOp::CopyRemove {
            project,
            team: id.clone(),
            seat,
        },
    )
    .await?;
    Ok(Json(read(&st, &id).await?))
}
