//! C5: the allow-listed `/api/community/*` proxy, against the stub Sculptured server.
mod common;
use axum::body::Body;
use axum::http::Request;
use controld::community::creds;
use serde_json::json;
use std::time::{Duration, Instant};

fn req(method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "test")
}

#[tokio::test]
async fn only_allow_listed_methods_and_paths_reach_the_server() {
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    assert_eq!(
        crate::common::m2::get(&s.router, "/api/community/blocks?q=x&offset=0")
            .await
            .0
            .as_u16(),
        200
    );
    for (m, p) in [
        ("GET", "/api/community/login"),
        ("GET", "/api/community/callback"),
        ("POST", "/api/community/logout"),
        ("POST", "/api/community/sessions/revoke"),
        ("POST", "/api/community/me"),
        ("GET", "/api/community/blocks/a%2Fb"),
        ("GET", "/api/community/users/x/../../../gw/api/devices"),
        ("GET", "/api/community/"),
        ("PUT", "/api/community/blocks"),
        ("GET", "/api/community/blocks/a/b/c"),
        ("GET", "/api/community/users/%2e%2e"),
    ] {
        let (st, v) = crate::common::m2::call(&s.router, m, p, json!({})).await;
        assert_eq!(
            (st.as_u16(), v["error"]["code"].as_str()),
            (404, Some("not_found")),
            "{m} {p}"
        );
    }
    assert_eq!(
        stub.requests().len(),
        1,
        "only the allowed GET went upstream: {:?}",
        stub.requests()
    );
    assert_eq!(
        stub.requests()[0].path_and_query,
        "/api/community/blocks?q=x&offset=0"
    );
}

#[tokio::test]
async fn cookies_origin_and_csrf_are_stripped_and_the_bearer_added() {
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (id, key) = crate::common::community::signed_in(&s, &stub);
    let r = req("PATCH", "/api/community/me")
        .header("content-type", "application/json")
        .header("cookie", "rc_session=x")
        .header("origin", "http://test")
        .header("x-csrf-token", "t")
        .header("x-other", "o")
        .body(Body::from(r#"{"bio":"hi"}"#))
        .unwrap();
    let (st, _, _) = crate::common::community::raw(&s.router, r).await;
    assert_eq!(st.as_u16(), 200);
    let seen = stub
        .requests()
        .into_iter()
        .find(|r| r.method == "PATCH")
        .unwrap();
    let names: Vec<&str> = seen.headers.iter().map(|(k, _)| k.as_str()).collect();
    for gone in ["cookie", "origin", "x-csrf-token", "x-other"] {
        assert!(
            !names.contains(&gone),
            "{gone} reached the server: {names:?}"
        );
    }
    assert!(
        seen.headers
            .contains(&("authorization".into(), format!("Bearer {id}.{key}")))
    );
    assert_eq!(seen.body, br#"{"bio":"hi"}"#);
}

#[tokio::test]
async fn a_revoked_key_on_any_request_signs_out_and_keeps_the_origin() {
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    crate::common::community::signed_in(&s, &stub);
    stub.revoke_all();
    let (st, v) = crate::common::m2::get(&s.router, "/api/community/blocks").await;
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (401, Some("device_unknown"))
    );
    assert!(
        creds::load(s.home.path()).is_none(),
        "community.json is gone"
    );
    let (_, link) = crate::common::m2::get(&s.router, "/api/community-link").await;
    assert_eq!(
        (link["enabled"].as_bool(), link["signed_in"].as_bool()),
        (Some(true), Some(false))
    );
}

#[tokio::test]
async fn writes_signed_out_are_refused_locally_with_signed_out() {
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (st, v) =
        crate::common::m2::call(&s.router, "POST", "/api/community/blocks/x/save", json!({})).await;
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (401, Some("signed_out"))
    );
    assert!(stub.requests().is_empty());
}

#[tokio::test]
async fn reads_signed_out_go_without_credentials() {
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    assert_eq!(
        crate::common::m2::get(&s.router, "/api/community/people")
            .await
            .0
            .as_u16(),
        200
    );
    assert!(
        stub.requests()[0]
            .headers
            .iter()
            .all(|(k, _)| k != "authorization")
    );
}

#[tokio::test]
async fn upstream_cookies_and_unknown_headers_never_reach_the_browser() {
    let (s, _stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (_, h, _) = crate::common::community::raw(
        &s.router,
        req("GET", "/api/community/blocks")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert!(
        h.get("set-cookie").is_none() && h.get("x-upstream-secret").is_none(),
        "{h:?}"
    );
    assert_eq!(h["cache-control"], "no-store");
    let (st, h, _) = crate::common::community::raw(
        &s.router,
        req("GET", "/api/community/users/ana/avatar")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st.as_u16(), 200);
    assert_eq!(
        (
            h["etag"].to_str().unwrap(),
            h["cache-control"].to_str().unwrap()
        ),
        ("\"av-1\"", "public, max-age=3600")
    );
    assert!(h.get("set-cookie").is_none());
}

#[tokio::test]
async fn every_proxied_answer_is_sandboxed_and_nosniff() {
    let (s, _stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    for path in [
        "/api/community/users/ana/avatar",
        "/api/community/blocks",
        "/api/community/blocks/nope",
    ] {
        let (_, h, _) =
            crate::common::community::raw(&s.router, req("GET", path).body(Body::empty()).unwrap())
                .await;
        let csp = h["content-security-policy"].to_str().unwrap();
        assert!(
            csp.contains("sandbox")
                && csp.contains("default-src 'none'")
                && csp.contains("img-src 'self' data:"),
            "{path}: {csp}"
        );
        assert_eq!(h["x-content-type-options"], "nosniff", "{path}");
    }
}

#[tokio::test]
async fn a_request_body_over_communitys_own_limit_is_refused_before_sending() {
    assert_eq!(
        controld::community::proxy::MAX_REQUEST_BYTES,
        1_100_000,
        "the same limit as Community's"
    );
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    crate::common::community::signed_in(&s, &stub);
    let put = |n: usize| {
        req("PUT", "/api/community/me/avatar")
            .header("content-type", "image/png")
            .body(Body::from(vec![0u8; n]))
            .unwrap()
    };
    let (st, _, body) = crate::common::community::raw(&s.router, put(1_200_000)).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (413, Some("community_too_large"))
    );
    assert!(
        stub.requests().iter().all(|r| r.method != "PUT"),
        "nothing was sent"
    );
    // A 600 KB body (a large group) goes through; here the server itself refuses it as a picture.
    let (st, _, body) = crate::common::community::raw(&s.router, put(600_000)).await;
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (413, Some("avatar_too_large"))
    );
    assert!(
        stub.requests()
            .iter()
            .any(|r| r.method == "PUT" && r.body.len() == 600_000)
    );
    assert_eq!(
        crate::common::community::raw(&s.router, put(262_144))
            .await
            .0
            .as_u16(),
        200
    );
}

#[tokio::test]
async fn an_answer_over_2_mb_is_community_bad_answer() {
    let (s, _stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (st, v) = crate::common::m2::get(&s.router, "/api/community/blocks?q=huge").await;
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (502, Some("community_bad_answer"))
    );
}

#[tokio::test]
async fn a_non_json_non_image_answer_is_community_bad_answer() {
    let (s, _stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (st, v) = crate::common::m2::get(&s.router, "/api/community/blocks?q=html").await;
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (502, Some("community_bad_answer"))
    );
}

#[tokio::test]
async fn a_slow_server_times_out_as_community_timeout() {
    assert_eq!(controld::community::PROXY_TIMEOUT, Duration::from_secs(15));
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    stub.set_delay(Duration::from_secs(1));
    let t = Instant::now();
    let (st, v) = crate::common::m2::get(&s.router, "/api/community/blocks").await;
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (504, Some("community_timeout"))
    );
    assert!(t.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn a_server_that_is_down_is_community_unreachable() {
    let (s, _stub) = crate::common::community::with_stub().await;
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    crate::common::m2::call(
        &s.router,
        "POST",
        "/api/community-link/enable",
        json!({"origin": format!("http://127.0.0.1:{closed}")}),
    )
    .await;
    let (st, v) = crate::common::m2::get(&s.router, "/api/community/blocks").await;
    assert_eq!(
        (st.as_u16(), v["error"]["code"].as_str()),
        (502, Some("community_unreachable"))
    );
}

#[tokio::test]
async fn an_old_handle_redirect_stays_on_this_origin() {
    let (s, _stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (st, h, _) = crate::common::community::raw(
        &s.router,
        req("GET", "/api/community/users/old-ana")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st.as_u16(), 301);
    assert_eq!(h["location"], "/api/community/users/ana");
}

#[tokio::test]
async fn credentials_never_appear_in_any_browser_response() {
    let (s, stub) = crate::common::community::with_stub().await;
    crate::common::community::on(&s).await;
    let (id, key) = crate::common::community::signed_in(&s, &stub);
    let probes = [
        ("GET", "/api/community-link"),
        ("POST", "/api/community-link/enable"),
        ("POST", "/api/community-link/pair"),
        ("GET", "/api/community-link/pair"),
        ("GET", "/api/community/me"),
        ("PATCH", "/api/community/me"),
        ("GET", "/api/community/blocks"),
        ("GET", "/api/community/people"),
        ("GET", "/api/community/users/ana"),
        ("DELETE", "/api/community/blocks/b-review-pair/save"),
        ("DELETE", "/api/community-link"),
    ];
    for (m, p) in probes {
        let (_, h, body) = crate::common::community::raw(
            &s.router,
            req(m, p)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        let text = String::from_utf8_lossy(&body);
        let headers: Vec<String> = h
            .values()
            .map(|v| v.to_str().unwrap_or_default().to_string())
            .collect();
        for secret in [id.as_str(), key.as_str(), "Bearer"] {
            assert!(
                !text.contains(secret) && headers.iter().all(|v| !v.contains(secret)),
                "{m} {p} shows {secret}"
            );
        }
    }
}
