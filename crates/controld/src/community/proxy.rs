//! `/api/community/*` (C5): forwards an allow-listed request to `{origin}/api/community/…` with the
//! saved key, and relays the answer with only the headers the page needs. Cookies, `Origin` and
//! the CSRF header never leave; upstream cookies never come back; nothing redirects.
use super::{CommunityLink, allow, creds, device_unknown, saved_origin};
use crate::state::AppState;
use crate::ApiError;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

/// The same as Community's own request limit (its `DefaultBodyLimit`), so a group that publishes
/// from the hosted app publishes from here too.
pub const MAX_REQUEST_BYTES: usize = 1_100_000;
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// R7: an upstream SVG opened directly on this origin must not run script.
const SANDBOX: &str = "sandbox; default-src 'none'; img-src 'self' data:; style-src 'unsafe-inline'";
const SENT: [header::HeaderName; 3] = [header::CONTENT_TYPE, header::ACCEPT, header::IF_NONE_MATCH];
const RELAYED: [header::HeaderName; 3] = [header::CONTENT_TYPE, header::ETAG, header::CACHE_CONTROL];
const TYPES: [&str; 5] = ["application/json", "image/png", "image/jpeg", "image/webp", "image/svg+xml"];

struct Target {
    link: Arc<CommunityLink>,
    origin: String,
    creds: Option<creds::Credentials>,
    url: String,
}

pub async fn forward(State(st): State<AppState>, req: Request) -> Response {
    let mut res = match go(&st, req).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    let h = res.headers_mut();
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(SANDBOX));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    res
}

async fn go(st: &AppState, req: Request) -> Result<Response, ApiError> {
    let target = check(st, &req)?;
    let (parts, body) = req.into_parts();
    let body = to_bytes(body, MAX_REQUEST_BYTES).await
        .map_err(|_| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "community_too_large", "That is too large to send to Community"))?;
    let upstream = build_upstream(&target, &parts.method, &parts.headers, body);
    let res = send(upstream).await?;
    relay(&target, res).await
}

/// Off, outside the allow-list, or a write while signed out: answered here, nothing is sent.
fn check(st: &AppState, req: &Request) -> Result<Target, ApiError> {
    let off = || ApiError::conflict("community_off", "Community is off on this computer");
    let link = st.community.clone().ok_or_else(off)?;
    let origin = saved_origin(st)?.ok_or_else(off)?;
    let path = req.uri().path().strip_prefix("/api/community/").unwrap_or_default();
    if !allow::allowed(req.method(), path) {
        return Err(ApiError::not_found(format!("no API route {}", req.uri().path())));
    }
    let creds = link.creds_for(&origin);
    let write = !matches!(*req.method(), Method::GET | Method::HEAD);
    if write && creds.is_none() {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "signed_out", "Sign in to Sculptured to do this"));
    }
    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let url = format!("{origin}/api/community/{path}{query}");
    Ok(Target { link, origin, creds, url })
}

/// Only the content headers go along, plus the key when signed in. No `Origin` (R1).
fn build_upstream(t: &Target, method: &Method, headers: &HeaderMap, body: axum::body::Bytes) -> reqwest::RequestBuilder {
    let mut r = t.link.http.request(method.clone(), &t.url).timeout(t.link.timeout);
    for name in SENT {
        if let Some(v) = headers.get(&name) {
            r = r.header(name, v);
        }
    }
    if let Some(c) = &t.creds {
        r = r.bearer_auth(c.bearer());
    }
    if !body.is_empty() { r.body(body) } else { r }
}

async fn send(r: reqwest::RequestBuilder) -> Result<reqwest::Response, ApiError> {
    r.send().await.map_err(|e| {
        if e.is_timeout() {
            ApiError::new(StatusCode::GATEWAY_TIMEOUT, "community_timeout", "Community did not answer in time")
        } else {
            ApiError::bad_gateway("community_unreachable", "Could not reach Community")
        }
    })
}

fn bad_answer() -> ApiError {
    ApiError::bad_gateway("community_bad_answer", "Community answered in a way this app does not accept")
}

/// Reads at most `MAX_RESPONSE_BYTES`; more is a bad answer.
async fn read_capped(mut res: reqwest::Response) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| if e.is_timeout() { ApiError::new(StatusCode::GATEWAY_TIMEOUT, "community_timeout", "Community did not answer in time") } else { bad_answer() })? {
        if out.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(bad_answer());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// The status, the body (JSON or a picture; an empty body may have no type) and a few headers.
/// A key the server refused (`401 device_unknown`) is forgotten before the answer goes back.
async fn relay(t: &Target, res: reqwest::Response) -> Result<Response, ApiError> {
    let status = StatusCode::from_u16(res.status().as_u16()).map_err(|_| bad_answer())?;
    let upstream = res.headers().clone();
    let body = read_capped(res).await?;
    let kind = upstream.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|v| v.split(';').next().unwrap_or_default().trim().to_ascii_lowercase());
    if !body.is_empty() && !kind.is_some_and(|k| TYPES.contains(&k.as_str())) {
        return Err(bad_answer());
    }
    if t.creds.is_some() && device_unknown(status.as_u16(), &body) {
        t.link.sign_out();
    }
    let mut out = (status, Body::from(body)).into_response();
    for name in RELAYED {
        if let Some(v) = upstream.get(&name) {
            out.headers_mut().insert(name, v.clone());
        }
    }
    if let Some(to) = upstream.get(header::LOCATION).and_then(|v| v.to_str().ok()).and_then(|l| local_location(&t.origin, l)) {
        out.headers_mut().insert(header::LOCATION, to);
    }
    Ok(out)
}

/// R6: a redirect to Community on the server becomes the same path here; any other is dropped.
fn local_location(origin: &str, location: &str) -> Option<HeaderValue> {
    let path = location.strip_prefix(origin).unwrap_or(location);
    let rest = path.strip_prefix("/api/community/")?;
    let (p, _query) = rest.split_once('?').unwrap_or((rest, ""));
    if !p.split('/').all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')) {
        return None;
    }
    HeaderValue::from_str(path).ok()
}
