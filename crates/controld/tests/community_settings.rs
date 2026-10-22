//! The Community setting (C5): only /api/community-link turns Community on or off.
mod common;
use serde_json::json;

#[tokio::test]
async fn put_settings_never_turns_community_on() {
    let s = crate::common::m2::serve().await;
    let (_, cur) = crate::common::m2::get(&s.router, "/api/settings").await;
    let mut next = cur.clone();
    next["sculptured_origin"] = json!("https://app.example.org");
    next["suggested_origin"] = json!("https://evil.example");
    let (st, body) =
        crate::common::m2::call(&s.router, "PUT", "/api/settings", json!({"settings": next})).await;
    assert_eq!(st.as_u16(), 200, "{body}");
    let (_, after) = crate::common::m2::get(&s.router, "/api/settings").await;
    assert!(
        after.get("sculptured_origin").is_none() && after.get("suggested_origin").is_none(),
        "{after}"
    );
}

#[tokio::test]
async fn put_settings_never_turns_community_off_or_moves_it() {
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (_, mut cur) = crate::common::m2::get(&s.router, "/api/settings").await;
    cur.as_object_mut().unwrap().remove("sculptured_origin");
    cur["question_timeout_min"] = json!(12);
    let (st, _) =
        crate::common::m2::call(&s.router, "PUT", "/api/settings", json!({"settings": cur})).await;
    assert_eq!(st.as_u16(), 200);
    let (_, after) = crate::common::m2::get(&s.router, "/api/settings").await;
    assert_eq!(
        (
            after["sculptured_origin"].as_str(),
            after["question_timeout_min"].as_u64()
        ),
        (Some(stub.origin.as_str()), Some(12))
    );
}
