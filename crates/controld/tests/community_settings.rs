//! The Community setting (C5): only /api/community-link turns Community on or off.
mod common;
use common::community::{on, with_stub};
use common::m2::{call, get, serve};
use serde_json::json;

#[tokio::test]
async fn put_settings_never_turns_community_on() {
    let s = serve().await;
    let (_, cur) = get(&s.router, "/api/settings").await;
    let mut next = cur.clone();
    next["sculptured_origin"] = json!("https://app.example.org");
    next["suggested_origin"] = json!("https://evil.example");
    let (st, body) = call(&s.router, "PUT", "/api/settings", json!({"settings": next})).await;
    assert_eq!(st.as_u16(), 200, "{body}");
    let (_, after) = get(&s.router, "/api/settings").await;
    assert!(after.get("sculptured_origin").is_none() && after.get("suggested_origin").is_none(), "{after}");
}

#[tokio::test]
async fn put_settings_never_turns_community_off_or_moves_it() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    let (_, mut cur) = get(&s.router, "/api/settings").await;
    cur.as_object_mut().unwrap().remove("sculptured_origin");
    cur["question_timeout_min"] = json!(12);
    let (st, _) = call(&s.router, "PUT", "/api/settings", json!({"settings": cur})).await;
    assert_eq!(st.as_u16(), 200);
    let (_, after) = get(&s.router, "/api/settings").await;
    assert_eq!((after["sculptured_origin"].as_str(), after["question_timeout_min"].as_u64()), (Some(stub.origin.as_str()), Some(12)));
}
