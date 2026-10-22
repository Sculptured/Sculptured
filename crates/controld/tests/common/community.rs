//! A controld with Community linked to the stub Sculptured server (`fixtures::community_stub`).
use super::m2::Served;
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use controld::community::{CommunityLink, creds};
use fixtures::community_stub::Stub;
use http_body_util::BodyExt;
use serde_json::json;
use std::time::Duration;
use tower::ServiceExt;

/// The link keeps its files in the served home; the default origin is the stub's; requests to it
/// time out after 300 ms so slow-server tests stay quick.
pub async fn with_stub() -> (Served, Stub) {
    let stub = fixtures::community_stub::spawn().await;
    let mut s = crate::common::m2::serve().await;
    let link = CommunityLink::new(s.home.path().to_path_buf(), stub.origin.clone())
        .with_timeout(Duration::from_millis(300));
    s.st = s.st.clone().with_community(link);
    s.router = controld::app(s.st.clone(), None);
    (s, stub)
}

pub async fn raw(router: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    (
        status,
        headers,
        resp.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
}

/// Turns Community on with the default (stub) origin.
pub async fn on(s: &Served) {
    let (st, v) =
        crate::common::m2::call(&s.router, "POST", "/api/community-link/enable", json!({})).await;
    assert_eq!(st.as_u16(), 200, "{v}");
}

/// Signed in as the stub's "you": a key saved as an approved pairing would. Returns (device_id, key).
pub fn signed_in(s: &Served, stub: &Stub) -> (String, String) {
    let (id, key) = stub.issue_key();
    creds::save(
        s.home.path(),
        &creds::Credentials {
            origin: stub.origin.clone(),
            device_id: id.clone(),
            key: key.clone(),
        },
    )
    .unwrap();
    (id, key)
}
