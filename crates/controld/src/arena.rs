//! Tests: a pass-through to arenad, the arena engine that records a team's run step by step, restores
//! its database to any step, and reruns from there (`arena/` repo). The web app's Tests pages call
//! `/api/arena/<path>`; this forwards to `<arenad>/api/<path>` so the browser stays on Sculptured's
//! origin, behind its guard and login. Local mode only: the hosted app has no arena (`AppState::arena`).
//!
//! arenad answers errors as `{"error": "<text>"}`. They come back in Sculptured's envelope with a code
//! per status and arenad's text as `raw` evidence; successful answers (JSON, or the SSE feed of a
//! running branch) are streamed through as they are.
use crate::error::ApiError;
use crate::state::AppState;
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use std::time::Duration;

/// Where arenad listens unless `SCULPTURED_ARENA_URL` says otherwise.
pub const DEFAULT_ARENA_URL: &str = "http://127.0.0.1:4510";

pub struct Arena {
    base: String,
    client: reqwest::Client,
    /// The external runs Sculptured drives on this arenad (Run in Arena), by branch.
    pub(crate) drives: crate::arena_runs::Drives,
}

impl Arena {
    /// Forwards to arenad at `base` (scheme, host and port, e.g. `http://127.0.0.1:4510`).
    #[must_use]
    pub fn new(base: &str) -> Arc<Arena> {
        // In the full build reqwest has rustls with no provider chosen; building a client then panics.
        let _ = rustls::crypto::ring::default_provider().install_default();
        // No overall timeout: a running branch's feed stays open until the branch ends.
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("http client");
        Arc::new(Arena {
            base: base.trim_end_matches('/').to_string(),
            client,
            drives: crate::arena_runs::Drives::default(),
        })
    }

    /// arenad's own address for API `path` (`runs`, `branches/<id>/finish`).
    pub(crate) fn api_url(&self, path: &str) -> String {
        format!("{}/api/{path}", self.base)
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.client
    }

    /// `SCULPTURED_ARENA_URL`, else [`DEFAULT_ARENA_URL`].
    #[must_use]
    pub fn from_env() -> Arc<Arena> {
        let url = std::env::var("SCULPTURED_ARENA_URL")
            .ok()
            .filter(|u| !u.trim().is_empty());
        Arena::new(url.as_deref().unwrap_or(DEFAULT_ARENA_URL))
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/arena/{*rest}", axum::routing::any(forward))
}

/// Only arenad's own API: each segment is a plain id or word, so nothing can climb out of `/api/`.
fn plain_path(rest: &str) -> bool {
    rest.split('/').all(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

async fn forward(
    State(st): State<AppState>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> Result<Response, ApiError> {
    let Some(arena) = st.arena.clone() else {
        return Err(crate::hosted::needs_computer());
    };
    // The raw path, not the decoded one: an escaped `/` or `.` is refused, never decoded into the URL.
    let rest = uri.path().strip_prefix("/api/arena/").unwrap_or_default();
    if !plain_path(rest) {
        return Err(ApiError::not_found("no such Tests route"));
    }
    if ![Method::GET, Method::POST, Method::DELETE].contains(&method) {
        return Err(ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Tests routes take GET, POST or DELETE",
        ));
    }
    let mut url = format!("{}/api/{rest}", arena.base);
    if let Some(q) = uri.query() {
        url.push('?');
        url.push_str(q);
    }
    let mut req = arena.client.request(method, &url);
    if !body.is_empty() {
        req = req
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
    }
    let res = req.send().await.map_err(|_| unreachable_arena())?;
    pass_through(res).await
}

/// arenad's answer as ours: a success streams through with its content type, an error is translated.
async fn pass_through(res: reqwest::Response) -> Result<Response, ApiError> {
    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(arena_error(status, &text));
    }
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| HeaderValue::from_bytes(v.as_bytes()).ok())
        .unwrap_or(HeaderValue::from_static("application/json"));
    let mut out = (status, Body::from_stream(res.bytes_stream())).into_response();
    out.headers_mut().insert(header::CONTENT_TYPE, content_type);
    out.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(out)
}

pub(crate) fn unreachable_arena() -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "arena_unreachable",
        "the arena engine (arenad) is not answering",
    )
}

/// arenad's `{"error": "<text>"}` in Sculptured's envelope: a code per status, the text as evidence.
pub(crate) fn arena_error(status: StatusCode, text: &str) -> ApiError {
    let said = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .unwrap_or_else(|| text.chars().take(300).collect());
    let code = match status {
        StatusCode::NOT_FOUND => "arena_not_found",
        StatusCode::CONFLICT => "arena_busy",
        StatusCode::INSUFFICIENT_STORAGE => "arena_full",
        StatusCode::BAD_REQUEST => "arena_refused",
        _ => "arena_failed",
    };
    ApiError {
        raw: Some(said.clone()),
        ..ApiError::new(status, code, said)
    }
}

#[cfg(test)]
mod tests {
    use super::{arena_error, plain_path};
    use axum::http::StatusCode;

    #[test]
    fn only_plain_segments_pass() {
        assert!(plain_path("runs"));
        assert!(plain_path("branches/0b9e-4c_1/rows"));
        assert!(!plain_path("../mcp/x/y"));
        assert!(!plain_path("runs/../../gateway"));
        assert!(!plain_path("runs//x"));
        assert!(!plain_path("runs/%2e%2e"));
        assert!(!plain_path("runs/a.b"));
    }

    #[test]
    fn errors_get_a_code_per_status_and_keep_arenads_text_as_evidence() {
        let e = arena_error(
            StatusCode::CONFLICT,
            r#"{"error":"the branch is still running"}"#,
        );
        assert_eq!(e.status, StatusCode::CONFLICT);
        assert_eq!(e.code, "arena_busy");
        assert_eq!(e.raw.as_deref(), Some("the branch is still running"));
        assert_eq!(
            arena_error(StatusCode::INSUFFICIENT_STORAGE, "{}").code,
            "arena_full"
        );
        assert_eq!(
            arena_error(StatusCode::BAD_GATEWAY, "not json")
                .raw
                .as_deref(),
            Some("not json")
        );
    }
}
