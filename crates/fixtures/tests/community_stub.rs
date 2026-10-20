//! The stub Sculptured server behaves like the gateway and Community in the cases the app handles.
use fixtures::community_stub::{PairingMode, spawn};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap()
}
async fn token(origin: &str) -> (u16, Value) {
    let r = client().post(format!("{origin}/gw/api/device/token")).json(&json!({"device_code": "dc-1"})).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
}

#[tokio::test]
async fn pairing_is_pending_until_approved_then_gives_a_key_once() {
    let stub = spawn().await;
    let start: Value = client().post(format!("{}/gw/api/device/start", stub.origin)).json(&json!({"name": "n", "platform": "linux", "scope": "community"}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(start["user_code"], "ABCD-EFGH");
    assert_eq!(token(&stub.origin).await.0, 428);
    stub.set_pairing(PairingMode::Approve);
    let (st, v) = token(&stub.origin).await;
    assert_eq!(st, 200);
    assert!(v["device_id"].is_string() && v["device_key"].is_string());
    assert_eq!(token(&stub.origin).await.0, 410, "a code gives its key once");
}

#[tokio::test]
async fn a_denied_pairing_answers_403_and_an_expired_one_410() {
    let stub = spawn().await;
    stub.set_pairing(PairingMode::Deny);
    let (st, v) = token(&stub.origin).await;
    assert_eq!((st, v["error"]["code"].as_str()), (403, Some("access_denied")));
    stub.set_pairing(PairingMode::Expire);
    assert_eq!(token(&stub.origin).await.0, 410);
}

#[tokio::test]
async fn the_bearer_of_a_paired_key_is_signed_in_and_others_are_not() {
    let stub = spawn().await;
    let (id, key) = stub.issue_key();
    let me = |auth: Option<String>| {
        let mut r = client().get(format!("{}/api/community/me", stub.origin));
        if let Some(a) = auth { r = r.header("authorization", a) }
        async move { let r = r.send().await.unwrap(); (r.status().as_u16(), r.json::<Value>().await.unwrap()) }
    };
    assert_eq!(me(Some(format!("Bearer {id}.{key}"))).await.1["user"]["name"], "You Example");
    assert_eq!(me(None).await.1, json!({"user": null}));
    let (st, v) = me(Some("Bearer dev-9.nope".into())).await;
    assert_eq!((st, v["error"]["code"].as_str()), (401, Some("device_unknown")));
    stub.revoke_all();
    assert_eq!(me(Some(format!("Bearer {id}.{key}"))).await.0, 401);
}

#[tokio::test]
async fn patch_me_refuses_a_taken_handle_with_handle_taken() {
    let stub = spawn().await;
    let (id, key) = stub.issue_key();
    for (handle, code) in [("taken", "handle_taken"), ("ana", "handle_taken"), ("admin", "handle_reserved"), ("A!", "handle_invalid")] {
        let r = client().patch(format!("{}/api/community/me", stub.origin)).bearer_auth(format!("{id}.{key}")).json(&json!({"handle": handle})).send().await.unwrap();
        let st = r.status().as_u16();
        assert_eq!(r.json::<Value>().await.unwrap()["error"]["code"], code, "{handle}");
        assert!(st == 409 || st == 400);
    }
    let r = client().patch(format!("{}/api/community/me", stub.origin)).json(&json!({"bio": "x"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 401, "signed out");
}

#[tokio::test]
async fn avatar_over_262144_bytes_is_avatar_too_large() {
    let stub = spawn().await;
    let (id, key) = stub.issue_key();
    let put = |n: usize, ty: &'static str| client().put(format!("{}/api/community/me/avatar", stub.origin)).bearer_auth(format!("{id}.{key}")).header("content-type", ty).body(vec![0u8; n]).send();
    let r = put(262_145, "image/png").await.unwrap();
    assert_eq!(r.status().as_u16(), 413);
    assert_eq!(r.json::<Value>().await.unwrap()["error"]["code"], "avatar_too_large");
    assert_eq!(put(1000, "image/png").await.unwrap().status().as_u16(), 200);
    assert_eq!(put(1000, "image/gif").await.unwrap().status().as_u16(), 415);
}

#[tokio::test]
async fn every_request_is_recorded_with_its_headers() {
    let stub = spawn().await;
    client().get(format!("{}/api/community/blocks?q=x", stub.origin)).header("cookie", "a=1").send().await.unwrap();
    client().post(format!("{}/stub/reset", stub.origin)).send().await.ok();
    let seen = stub.requests();
    assert_eq!(seen.len(), 0, "a reset forgets what was seen");
    client().get(format!("{}/api/community/blocks?q=x", stub.origin)).header("cookie", "a=1").send().await.unwrap();
    let seen = stub.requests();
    assert_eq!(seen[0].path_and_query, "/api/community/blocks?q=x");
    assert!(seen[0].headers.contains(&("cookie".into(), "a=1".into())));
}

#[tokio::test]
async fn delay_holds_every_community_answer() {
    let stub = spawn().await;
    stub.set_delay(Duration::from_millis(300));
    let t = Instant::now();
    client().get(format!("{}/api/community/blocks", stub.origin)).send().await.unwrap();
    assert!(t.elapsed() >= Duration::from_millis(300));
}

#[tokio::test]
async fn community_answers_carry_cookies_and_an_old_handle_redirects_absolutely() {
    let stub = spawn().await;
    let r = client().get(format!("{}/api/community/blocks", stub.origin)).send().await.unwrap();
    assert_eq!(r.headers()["set-cookie"], "stub=1");
    assert_eq!(r.headers()["cache-control"], "no-store");
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["blocks"][0]["author"]["name"], "Ana Lee");
    let r = client().get(format!("{}/api/community/users/ana/avatar", stub.origin)).send().await.unwrap();
    assert_eq!(r.headers()["cache-control"], "public, max-age=3600");
    let r = client().get(format!("{}/api/community/users/old-ana", stub.origin)).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 301);
    assert_eq!(r.headers()["location"], format!("{}/api/community/users/ana", stub.origin).as_str());
}

#[test]
fn the_seed_block_is_a_valid_group_block() {
    let b: model::GroupBlock = serde_json::from_str(include_str!("../community/review-pair.group.json")).unwrap();
    assert_eq!((b.draft.groups.len(), b.draft.agents.len(), b.draft.connections.len()), (1, 2, 1));
}
