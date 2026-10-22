//! The Claude Code and Codex CLIs (routes). The work is `localops::clis`, shared with connected computers.
use crate::error::ApiError;
use crate::extract::Path;
use crate::state::AppState;
use axum::extract::State;
use axum::{Json, Router};
pub use localops::clis::{CliStatus, LoginStarted};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/clis", axum::routing::get(list))
        .route("/api/clis/{name}/login", axum::routing::post(login))
        .route(
            "/api/clis/{name}/login/code",
            axum::routing::post(login_code),
        )
}

#[derive(Deserialize)]
struct On {
    /// Hosted: which computer's CLIs (else the team's computer, else the only connected one).
    host: Option<String>,
    /// The team whose computer it is (what Start checks before starting).
    team: Option<String>,
}

fn target(on: &On) -> crate::ops::Target<'_> {
    match (on.host.as_deref(), on.team.as_deref()) {
        (Some(h), _) => crate::ops::Target::Host(h),
        (None, Some(t)) => crate::ops::Target::Team(t),
        (None, None) => crate::ops::Target::Default,
    }
}

async fn list(
    State(st): State<AppState>,
    axum::extract::Query(on): axum::extract::Query<On>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Ok(Json(
        crate::ops::run(&st, target(&on), localops::LocalOp::Clis).await?,
    ))
}

#[derive(Deserialize)]
struct LoginReq {
    #[allow(dead_code)]
    request_id: String,
}

/// Starts the CLI's sign-in and returns the link it prints (see `localops::clis::login`).
async fn login(
    State(st): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Query(on): axum::extract::Query<On>,
    Json(_req): Json<LoginReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if name != "claude" && name != "codex" {
        return Err(ApiError::not_found(format!("no CLI {name:?}")));
    }
    Ok(Json(
        crate::ops::run(&st, target(&on), localops::LocalOp::CliLogin { name }).await?,
    ))
}

#[derive(Deserialize)]
struct CodeReq {
    #[allow(dead_code)]
    request_id: String,
    code: String,
}

/// Passes the code the sign-in page shows to the sign-in under way on that computer.
async fn login_code(
    State(st): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Query(on): axum::extract::Query<On>,
    Json(req): Json<CodeReq>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if name != "claude" && name != "codex" {
        return Err(ApiError::not_found(format!("no CLI {name:?}")));
    }
    Ok(Json(
        crate::ops::run(
            &st,
            target(&on),
            localops::LocalOp::CliLoginCode {
                name,
                code: req.code,
            },
        )
        .await?,
    ))
}
