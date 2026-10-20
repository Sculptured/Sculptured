//! `/api/community-link` (C5): whether Community is on, as whom, and turning it on or off.
use super::{CommunityLinkView, creds};
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
        .route("/api/community-link/key-left", axum::routing::delete(dismiss_key_left))
        .route("/api/community-link/enable", post(enable))
}

async fn get_link(State(st): State<AppState>) -> Result<Json<CommunityLinkView>, ApiError> {
    let Some(link) = st.community.clone() else { return Ok(Json(CommunityLinkView::off())) };
    let s = st.store.lock().unwrap().settings()?;
    if s.sculptured_origin.is_none() {
        return Ok(Json(CommunityLinkView { suggested_origin: s.suggested_origin, key_left_on: link.key_left_on(), ..CommunityLinkView::off() }));
    }
    Ok(Json(link.view(s.sculptured_origin).await))
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
    if let Some(old) = creds::load(link.data_dir()).filter(|c| c.origin != origin) {
        link.cancel_pairing();
        link.revoke(&old).await;
        link.sign_out();
    }
    set_origin(&st, Some(origin.clone()))?;
    Ok(Json(link.view(Some(origin)).await))
}

/// Turns Community off: tries to revoke the key on the server (best effort, a few seconds at
/// most), then forgets the key and the server.
async fn turn_off(State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    if let Some(link) = st.community.clone() {
        link.cancel_pairing();
        if let Some(c) = creds::load(link.data_dir()) {
            // A failed revoke is remembered and shown (`key_left_on`); the key is forgotten here either way.
            link.revoke(&c).await;
        }
        link.sign_out();
    }
    set_origin(&st, None)?;
    Ok(Json(json!({"enabled": false})))
}

/// The user saw that a key may remain (and removed it on the Computers page, or chose not to).
async fn dismiss_key_left(State(st): State<AppState>) -> Result<Json<CommunityLinkView>, ApiError> {
    if let Some(link) = st.community.clone() {
        link.dismiss_key_left();
    }
    get_link(State(st)).await
}

fn set_origin(st: &AppState, origin: Option<String>) -> Result<(), ApiError> {
    let mut store = st.store.lock().unwrap();
    let mut s = store.settings()?;
    if origin.is_some() {
        s.suggested_origin = None;
    }
    s.sculptured_origin = origin;
    store.save_settings(&s)?;
    Ok(())
}
