//! Setup progress: which setup steps the person finished or skipped, kept on the server so Setup
//! resumes on any device.
mod common;
use axum::http::StatusCode;
use common::m2::*;
use serde_json::json;

#[tokio::test]
async fn setup_progress_starts_empty_and_round_trips_without_duplicates() {
    let s = serve().await;
    let (status, body) = get(&s.router, "/api/setup").await;
    assert_eq!((status, body.clone()), (StatusCode::OK, json!({ "done": [], "skipped": [] })), "{body}");
    let put = json!({ "request_id": rid(), "done": ["welcome", "computer", "models", "computer"], "skipped": ["tour", "tour"] });
    let (status, body) = call(&s.router, "PUT", "/api/setup", put).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let want = json!({ "done": ["welcome", "computer", "models"], "skipped": ["tour"] });
    assert_eq!(body, want);
    assert_eq!(get(&s.router, "/api/setup").await.1, want, "kept on the server");
    let (_, body) = call(&s.router, "PUT", "/api/setup", json!({ "done": ["welcome", "tour"], "skipped": ["tour", "computers"] })).await;
    assert_eq!(body, json!({ "done": ["welcome", "tour"], "skipped": ["computers"] }), "a step that is done is not also skipped");
}

#[tokio::test]
async fn setup_progress_refuses_unknown_steps_and_keeps_what_was_saved() {
    let s = serve().await;
    call(&s.router, "PUT", "/api/setup", json!({ "done": ["welcome"], "skipped": [] })).await;
    let (status, body) = call(&s.router, "PUT", "/api/setup", json!({ "done": ["welcome", "payment"], "skipped": [] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "setup_step_invalid");
    assert!(body["error"]["message"].as_str().unwrap().contains("payment"), "{body}");
    assert_eq!(get(&s.router, "/api/setup").await.1, json!({ "done": ["welcome"], "skipped": [] }));
}

#[tokio::test]
async fn hosted_setup_progress_is_behind_the_gateway_like_every_route() {
    let s = serve_hosted().await;
    let (status, _) = signed(&s.router, "PUT", "/api/setup", Some(json!({ "done": ["computer"], "skipped": ["computers"] }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(signed(&s.router, "GET", "/api/setup", None).await.1, json!({ "done": ["computer"], "skipped": ["computers"] }));
    assert_eq!(get(&s.router, "/api/setup").await.0, StatusCode::UNAUTHORIZED, "unsigned requests are refused");
}
