//! `/gw/api/device/*`: the gateway's device-code pairing (C4), driven by `PairingMode`.
use super::{PairingMode, Shared, error};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;

pub(super) fn routes() -> Router<Shared> {
    Router::new()
        .route("/gw/api/device/start", post(start))
        .route("/gw/api/device/token", post(token))
        .route("/gw/api/device/revoke", post(revoke))
}

async fn start(State((_, origin)): State<Shared>) -> Json<serde_json::Value> {
    Json(json!({
        "device_code": "dc-1", "user_code": "ABCD-EFGH",
        "verify_url": format!("{origin}/connect?code=ABCD-EFGH"), "interval": 1, "expires_in": 600,
    }))
}

async fn token(State((state, _)): State<Shared>) -> Response {
    let mut s = state.lock().unwrap();
    match s.pairing {
        PairingMode::Pending => error(StatusCode::PRECONDITION_REQUIRED, "authorization_pending"),
        PairingMode::Deny => error(StatusCode::FORBIDDEN, "access_denied"),
        PairingMode::Expire => error(StatusCode::GONE, "expired_token"),
        PairingMode::Approve if s.key_given => error(StatusCode::GONE, "expired_token"),
        PairingMode::Approve => {
            s.key_given = true;
            let (device_id, device_key) = s.issue();
            Json(json!({"device_id": device_id, "device_key": device_key})).into_response()
        }
    }
}

/// Revokes the calling key itself (C4); an unknown or revoked key is `401 device_unknown`.
async fn revoke(State((state, _)): State<Shared>, headers: HeaderMap) -> Response {
    let mut s = state.lock().unwrap();
    if s.revoke_fails {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "internal");
    }
    if let Err(refused) = s.caller(&headers) {
        return refused;
    }
    let bearer = headers.get("authorization").and_then(|a| a.to_str().ok()).and_then(|a| a.strip_prefix("Bearer "));
    match bearer {
        Some(b) => {
            s.keys.insert(b.to_string(), false);
            Json(json!({"ok": true})).into_response()
        }
        None => error(StatusCode::UNAUTHORIZED, "device_unknown"),
    }
}
