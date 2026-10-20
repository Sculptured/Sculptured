//! C5: turning Community on and off, and the signed-in account, against the stub server.
mod common;
use common::community::{on, signed_in, with_stub};
use common::m2::{call, get};
use controld::community::creds;
use serde_json::json;

#[tokio::test]
async fn community_is_off_until_turned_on_and_off_makes_no_request() {
    let (s, stub) = with_stub().await;
    let (st, v) = get(&s.router, "/api/community-link").await;
    assert_eq!((st.as_u16(), v), (200, json!({"enabled":false,"origin":null,"signed_in":false,"account":null})));
    for (m, p) in [("GET", "/api/community/blocks"), ("GET", "/api/community/me"), ("POST", "/api/community/blocks")] {
        let (st, v) = call(&s.router, m, p, json!({})).await;
        assert_eq!((st.as_u16(), v["error"]["code"].as_str()), (409, Some("community_off")), "{m} {p}");
    }
    assert!(stub.requests().is_empty(), "nothing reached the upstream: {:?}", stub.requests());
}

#[tokio::test]
async fn turning_on_uses_the_default_origin_or_a_given_https_one() {
    let (s, stub) = with_stub().await;
    let (_, v) = call(&s.router, "POST", "/api/community-link/enable", json!({})).await;
    assert_eq!((v["enabled"].as_bool(), v["origin"].as_str(), v["signed_in"].as_bool()), (Some(true), Some(stub.origin.as_str()), Some(false)));
    let (_, v) = call(&s.router, "POST", "/api/community-link/enable", json!({"origin": "https://app.other.org/"})).await;
    assert_eq!(v["origin"], "https://app.other.org");
    let (_, set) = get(&s.router, "/api/settings").await;
    assert_eq!(set["sculptured_origin"], "https://app.other.org");
    assert!(stub.requests().is_empty(), "turning on sends nothing");
}

#[tokio::test]
async fn turning_on_refuses_plain_http_except_loopback() {
    let (s, _stub) = with_stub().await;
    let (st, v) = call(&s.router, "POST", "/api/community-link/enable", json!({"origin": "http://example.com"})).await;
    assert_eq!((st.as_u16(), v["error"]["code"].as_str()), (400, Some("community_origin_invalid")));
    let (_, set) = get(&s.router, "/api/settings").await;
    assert!(set.get("sculptured_origin").is_none(), "settings unchanged: {set}");
}

#[tokio::test]
async fn signed_in_reports_the_account_from_upstream_me_and_never_the_key() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    let (id, key) = signed_in(&s, &stub);
    stub.state.lock().unwrap().you.handle = Some("you-ex".into());
    let (st, v) = get(&s.router, "/api/community-link").await;
    assert_eq!(st.as_u16(), 200);
    assert_eq!((v["signed_in"].as_bool(), &v["account"]), (Some(true), &json!({"name": "You Example", "handle": "you-ex"})));
    let text = v.to_string();
    assert!(!text.contains(&id) && !text.contains(&key));
    let me = stub.requests().into_iter().find(|r| r.path_and_query == "/api/community/me").expect("checked with me");
    assert!(me.headers.contains(&("authorization".into(), format!("Bearer {id}.{key}"))));
}

#[cfg(unix)]
#[tokio::test]
async fn credentials_file_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    creds::save(dir.path(), &creds::Credentials { origin: "https://a".into(), device_id: "d".into(), key: "k".into() }).unwrap();
    let mode = std::fs::metadata(dir.path().join(creds::FILE)).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(creds::load(dir.path()).unwrap().bearer(), "d.k");
}

#[tokio::test]
async fn credentials_for_another_origin_count_as_signed_out() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    creds::save(s.home.path(), &creds::Credentials { origin: "https://app.other.org".into(), device_id: "d".into(), key: "k".into() }).unwrap();
    let (_, v) = get(&s.router, "/api/community-link").await;
    assert_eq!((v["enabled"].as_bool(), v["signed_in"].as_bool()), (Some(true), Some(false)));
    assert!(stub.requests().is_empty());
}

#[tokio::test]
async fn a_key_the_server_no_longer_accepts_signs_out_and_keeps_the_origin() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    signed_in(&s, &stub);
    stub.revoke_all();
    let (_, v) = get(&s.router, "/api/community-link").await;
    assert_eq!((v["enabled"].as_bool(), v["signed_in"].as_bool(), v["origin"].as_str()), (Some(true), Some(false), Some(stub.origin.as_str())));
    assert!(creds::load(s.home.path()).is_none(), "community.json is gone");
    let paths: Vec<String> = stub.requests().into_iter().map(|r| r.path_and_query).collect();
    assert_eq!(paths, vec!["/api/community/me".to_string()], "checked with me only, never /gw/api/device/check");
}

#[tokio::test]
async fn turning_off_forgets_origin_and_credentials_and_tries_to_revoke() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    let (id, key) = signed_in(&s, &stub);
    let (st, v) = call(&s.router, "DELETE", "/api/community-link", json!({})).await;
    assert_eq!((st.as_u16(), v), (200, json!({"enabled": false})));
    let revoke = stub.requests().into_iter().find(|r| r.path_and_query == "/gw/api/device/revoke").expect("revoke tried");
    assert!(revoke.headers.contains(&("authorization".into(), format!("Bearer {id}.{key}"))));
    assert!(revoke.headers.contains(&("origin".into(), stub.origin.clone())));
    assert!(creds::load(s.home.path()).is_none());
    let (_, set) = get(&s.router, "/api/settings").await;
    assert!(set.get("sculptured_origin").is_none(), "{set}");
}

#[tokio::test]
async fn turning_off_works_when_the_server_is_down() {
    let (s, _stub) = with_stub().await;
    let closed = { let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().port() };
    let origin = format!("http://127.0.0.1:{closed}");
    call(&s.router, "POST", "/api/community-link/enable", json!({"origin": origin})).await;
    creds::save(s.home.path(), &creds::Credentials { origin, device_id: "d".into(), key: "k".into() }).unwrap();
    let t = std::time::Instant::now();
    let (st, _) = call(&s.router, "DELETE", "/api/community-link", json!({})).await;
    assert_eq!(st.as_u16(), 200);
    assert!(t.elapsed() < std::time::Duration::from_secs(6));
    assert!(creds::load(s.home.path()).is_none());
}

#[tokio::test]
async fn an_old_community_url_only_suggests_a_server_and_nothing_is_contacted() {
    let (s, stub) = with_stub().await;
    let row = format!(r#"{{"community_url":"{}","posture":"unrestricted","question_timeout_min":10}}"#, stub.origin);
    s.st.store.lock().unwrap().connection().execute("INSERT INTO settings (k, json) VALUES ('global', ?1)", [row]).unwrap();
    let (_, v) = get(&s.router, "/api/community-link").await;
    assert_eq!((v["enabled"].as_bool(), v["origin"].as_str(), v["suggested_origin"].as_str()), (Some(false), None, Some(stub.origin.as_str())));
    let (st, v) = get(&s.router, "/api/community/blocks").await;
    assert_eq!((st.as_u16(), v["error"]["code"].as_str()), (409, Some("community_off")));
    assert!(stub.requests().is_empty(), "nothing reached the server");
    let (_, v) = call(&s.router, "POST", "/api/community-link/enable", json!({"origin": stub.origin})).await;
    assert_eq!((v["enabled"].as_bool(), v["suggested_origin"].as_str()), (Some(true), None), "turning on uses it up");
}

fn revokes(stub: &fixtures::community_stub::Stub) -> usize {
    stub.requests().iter().filter(|r| r.path_and_query == "/gw/api/device/revoke").count()
}

#[tokio::test]
async fn switching_to_another_server_revokes_the_old_key() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    signed_in(&s, &stub);
    call(&s.router, "POST", "/api/community-link/enable", json!({"origin": "https://app.other.org"})).await;
    assert_eq!((revokes(&stub), stub.live_keys()), (1, 0), "the old server no longer accepts the key");
    assert!(creds::load(s.home.path()).is_none());
}

#[tokio::test]
async fn a_key_the_server_could_not_revoke_is_reported_until_dismissed() {
    let (s, stub) = with_stub().await;
    on(&s).await;
    signed_in(&s, &stub);
    stub.set_revoke_failing(true);
    call(&s.router, "DELETE", "/api/community-link", json!({})).await;
    assert_eq!(revokes(&stub), 1);
    let (_, v) = get(&s.router, "/api/community-link").await;
    assert_eq!((v["enabled"].as_bool(), v["key_left_on"].as_str()), (Some(false), Some(stub.origin.as_str())), "{v}");
    let (st, v) = call(&s.router, "DELETE", "/api/community-link/key-left", json!({})).await;
    assert_eq!((st.as_u16(), v.get("key_left_on")), (200, None), "{v}");
}

#[tokio::test]
async fn turning_off_with_the_server_down_says_a_key_may_remain() {
    let (s, _stub) = with_stub().await;
    let closed = { let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().port() };
    let origin = format!("http://127.0.0.1:{closed}");
    call(&s.router, "POST", "/api/community-link/enable", json!({"origin": origin})).await;
    creds::save(s.home.path(), &creds::Credentials { origin: origin.clone(), device_id: "d".into(), key: "k".into() }).unwrap();
    call(&s.router, "DELETE", "/api/community-link", json!({})).await;
    let (_, v) = get(&s.router, "/api/community-link").await;
    assert_eq!(v["key_left_on"].as_str(), Some(origin.as_str()));
}
