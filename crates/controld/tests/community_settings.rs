//! The Community setting (C5): `sculptured_origin`, validated on save.
mod common;
use common::m2::{call, get, serve};
use serde_json::json;

#[tokio::test]
async fn put_settings_refuses_a_plain_http_origin_with_community_origin_invalid() {
    let s = serve().await;
    let (_, cur) = get(&s.router, "/api/settings").await;
    let mut next = cur.clone();
    next["sculptured_origin"] = json!("http://evil.example");
    let (st, body) = call(&s.router, "PUT", "/api/settings", json!({"settings": next})).await;
    assert_eq!((st.as_u16(), body["error"]["code"].as_str()), (400, Some("community_origin_invalid")));
}

#[tokio::test]
async fn put_settings_stores_the_origin_without_a_trailing_slash() {
    let s = serve().await;
    let (_, cur) = get(&s.router, "/api/settings").await;
    let mut next = cur.clone();
    next["sculptured_origin"] = json!("https://app.example.org/");
    let (st, body) = call(&s.router, "PUT", "/api/settings", json!({"settings": next})).await;
    assert_eq!((st.as_u16(), body["sculptured_origin"].as_str()), (200, Some("https://app.example.org")), "{body}");
}
