//! `GET /api/teams/{id}/controls`: every agent of a team with a control mode, in one request
//! (Practice agents, no model is called).
mod common;
use axum::http::StatusCode;
use common::m2::{connect_stub, get, seat, start_team};
use common::pausing::*;
use serde_json::json;

#[tokio::test]
async fn team_controls_lists_each_agents_mode_and_only_this_team() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let (id, [a, _b, c]) = three_with_c_stopped(&s, "controls-one").await;
    let other = team(&s, "controls-two", &["x"]).await;
    common::m2::start_team(&s, &other).await;
    ok(control(&s, &seat(&other, "x"), "hard").await);

    ok(control(&s, &a, "pause").await);
    wait_paused(&s, &[&a]).await;

    let (status, body) = get(&s.router, &format!("/api/teams/{id}/controls")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ a: "paused", c: "hard" }),
        "b has no mode and x is another team's"
    );
}

#[tokio::test]
async fn team_controls_is_empty_for_a_team_without_controls() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let id = team(&s, "controls-none", &["a"]).await;
    start_team(&s, &id).await;
    let (status, body) = get(&s.router, &format!("/api/teams/{id}/controls")).await;
    assert_eq!((status, body), (StatusCode::OK, json!({})));
}
