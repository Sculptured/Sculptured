//! Folder picker routes. The work (resolving inside home, listing, creating) is `localops::folders`,
//! shared with connected computers' runners.
use crate::error::ApiError;
use crate::extract::{Json, Query};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
pub use localops::folders::{Home, MAX_ENTRIES};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/folders",
        axum::routing::get(list_folders).post(create_folder),
    )
}

/// The home folder, or `home_unknown` when HOME is unset or can't be resolved.
pub(crate) fn home(st: &AppState) -> Result<Home, ApiError> {
    let given = st.home.as_deref().ok_or_else(|| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "home_unknown",
            "Your home folder is unknown (HOME is not set)",
        )
    })?;
    Ok(Home::of(given)?)
}

/// `localops::folders::resolve_inside` with controld's error type.
pub fn resolve_inside(home: &Home, requested: &Path) -> Result<PathBuf, ApiError> {
    Ok(localops::folders::resolve_inside(home, requested)?)
}

#[derive(Deserialize)]
struct ListQuery {
    path: Option<String>,
    /// Hosted: browse the computer that runs this team…
    team: Option<String>,
    /// …or this computer (else the only connected one).
    host: Option<String>,
    /// Also list the folder's file names (attaching files to the Input).
    #[serde(default)]
    files: bool,
}

fn target<'a>(team: Option<&'a str>, host: Option<&'a str>) -> crate::ops::Target<'a> {
    match (team, host) {
        (Some(t), _) => crate::ops::Target::Team(t),
        (None, Some(h)) => crate::ops::Target::Host(h),
        (None, None) => crate::ops::Target::Default,
    }
}

async fn list_folders(State(st): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    let path = q.path.clone().filter(|p| !p.trim().is_empty());
    match crate::ops::run(
        &st,
        target(q.team.as_deref(), q.host.as_deref()),
        localops::LocalOp::ListFolder {
            path,
            files: q.files,
        },
    )
    .await
    {
        Ok(l) => Json(l).into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Deserialize, Serialize)]
struct CreateReq {
    #[serde(default)]
    request_id: String,
    parent: String,
    name: String,
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    host: Option<String>,
}

async fn create_folder(State(st): State<AppState>, Json(req): Json<CreateReq>) -> Response {
    let fp = crate::api::fingerprint("create-folder", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::api::idempotent(&st, &request_id, fp, move || async move {
        crate::ops::run(
            &st2,
            target(req.team.as_deref(), req.host.as_deref()),
            localops::LocalOp::CreateFolder {
                parent: req.parent.clone(),
                name: req.name.clone(),
            },
        )
        .await
    })
    .await
}
