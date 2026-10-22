//! DNS-rebinding and cross-site protection: only this machine's sculptured
//! address may call the API (browser, hostd, CLI).
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn guard(State(st): State<AppState>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let host_ok = st.allowed_hosts.iter().any(|h| h == host);
    let origin_ok = match req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
    {
        None => true,
        Some(origin) => st
            .allowed_hosts
            .iter()
            .any(|h| origin == format!("http://{h}")),
    };
    if !(host_ok && origin_ok) {
        return ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden_host",
            "requests must use this machine's sculptured address",
        )
        .into_response();
    }
    next.run(req).await
}
