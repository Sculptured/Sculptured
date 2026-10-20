//! `/api/community-link` (C5): whether Community is on, as whom, and turning it on or off.
use super::{CommunityLinkView, REVOKE_TIMEOUT, creds, saved_origin};
use crate::extract::Json;
use crate::state::AppState;
use crate::ApiError;
use axum::Router;
use axum::extract::State;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::{Value, json};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/community-link", get(get_link).delete(turn_off))
        .route("/api/community-link/enable", post(enable))
}

async fn get_link(State(st): State<AppState>) -> Result<Json<CommunityLinkView>, ApiError> {
    let Some(link) = st.community.clone() else { return Ok(Json(CommunityLinkView::off())) };
    Ok(Json(link.view(saved_origin(&st)?).await))
}

#[derive(Deserialize, Default)]
struct Enable {
    origin: Option<String>,
}

/// Turns Community on for `origin` (default: the Sculptured server this install was built for).
/// A key for another server is forgotten. Nothing is sent.
async fn enable(State(st): State<AppState>, Json(req): Json<Enable>) -> Result<Json<CommunityLinkView>, ApiError> {
    let Some(link) = st.community.clone() else { return Err(ApiError::not_found("Community is not available here")) };
    let origin = crate::settings::valid_origin(req.origin.as_deref().unwrap_or(link.default_origin()))?;
    if creds::load(link.data_dir()).is_some_and(|c| c.origin != origin) {
        link.sign_out();
    }
    set_origin(&st, Some(origin.clone()))?;
    Ok(Json(link.view(Some(origin)).await))
}

/// Turns Community off: tries to revoke the key on the server (best effort, a few seconds at
/// most), then forgets the key and the server.
async fn turn_off(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    if let Some(link) = st.community.clone() {
        if let Some(c) = creds::load(link.data_dir()) {
            let revoke = link.http.post(format!("{}/gw/api/device/revoke", c.origin)).bearer_auth(c.bearer()).header("origin", &c.origin).timeout(REVOKE_TIMEOUT);
            let _ = revoke.send().await;
        }
        link.sign_out();
    }
    set_origin(&st, None)?;
    Ok(Json(json!({"enabled": false})))
}

fn set_origin(st: &AppState, origin: Option<String>) -> Result<(), ApiError> {
    let mut store = st.store.lock().unwrap();
    let mut s = store.settings()?;
    s.sculptured_origin = origin;
    store.save_settings(&s)?;
    Ok(())
}
