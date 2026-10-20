//! C5: signing in to Community with a community-scope device key, against the stub server.
mod common;
use common::community::{on, with_stub};
use common::m2::{Served, call, get};
use controld::community::creds;
use fixtures::community_stub::PairingMode;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

async fn status_until(s: &Served, want: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (_, v) = get(&s.router, "/api/community-link/pair").await;
        let got = v["status"].as_str().unwrap_or_default().to_string();
        if got == want || Instant::now() > deadline {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn pairing_starts_with_scope_community_and_shows_code_and_link() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    let (st, v) = call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    assert_eq!(st.as_u16(), 200, "{v}");
    assert_eq!((v["user_code"].as_str(), v["expires_in"].as_u64()), (Some("ABCD-EFGH"), Some(600)));
    assert_eq!(v["verify_url"], json!(format!("{}/connect?code=ABCD-EFGH", stub.origin)));
    assert!(v.get("device_code").is_none());
    let start = stub.requests().into_iter().find(|r| r.path_and_query == "/gw/api/device/start").unwrap();
    let body: Value = serde_json::from_slice(&start.body).unwrap();
    assert_eq!(body["scope"], "community");
    assert!(!body["name"].as_str().unwrap().is_empty());
    assert!(start.headers.contains(&("origin".into(), stub.origin.clone())));
    assert_eq!(status_until(&s, "pending").await, "pending");
}

#[tokio::test]
async fn approval_stores_the_key_owner_only_and_signs_in() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    stub.set_pairing(PairingMode::Approve);
    assert_eq!(status_until(&s, "approved").await, "approved");
    let c = creds::load(s.home.path()).expect("the key is saved");
    assert_eq!(c.origin, stub.origin);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(s.home.path().join(creds::FILE)).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let (_, v) = get(&s.router, "/api/community-link").await;
    assert_eq!((v["signed_in"].as_bool(), v["account"]["name"].as_str()), (Some(true), Some("You Example")));
}

#[tokio::test]
async fn a_denied_pairing_reports_denied_and_stores_nothing() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    stub.set_pairing(PairingMode::Deny);
    assert_eq!(status_until(&s, "denied").await, "denied");
    assert!(creds::load(s.home.path()).is_none());
}

#[tokio::test]
async fn an_expired_pairing_reports_expired() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    stub.set_pairing(PairingMode::Expire);
    assert_eq!(status_until(&s, "expired").await, "expired");
    assert!(creds::load(s.home.path()).is_none());
}

#[tokio::test]
async fn pairing_while_off_is_community_off() {
    let (s, stub) = with_stub().await;
    let (st, v) = call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    assert_eq!((st.as_u16(), v["error"]["code"].as_str()), (409, Some("community_off")));
    assert!(stub.requests().is_empty());
}

#[tokio::test]
async fn a_second_pair_replaces_the_first() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    stub.set_pairing(PairingMode::Approve);
    assert_eq!(status_until(&s, "approved").await, "approved");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let tokens = stub.requests().into_iter().filter(|r| r.path_and_query == "/gw/api/device/token").count();
    let files = std::fs::read_dir(s.home.path()).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("community.json")).count();
    assert_eq!(files, 1, "exactly one community.json");
    assert_eq!(stub.state.lock().unwrap().issued, 1, "one device saved");
    assert!(tokens >= 1);
}

#[tokio::test]
async fn turning_off_during_a_pairing_stops_it() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    call(&s.router, "POST", "/api/community-link/pair", json!({})).await;
    call(&s.router, "DELETE", "/api/community-link", json!({})).await;
    stub.set_pairing(PairingMode::Approve);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(creds::load(s.home.path()).is_none(), "an approval after turning off is not kept");
}
