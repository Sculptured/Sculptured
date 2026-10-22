//! Hosted analytics from a user's Sculptured: shapes, starts, stops, runs and checks, signed, and
//! never names or text. Local mode sends nothing.
mod common;
use axum::http::StatusCode;

use common::m2::{Served, rid};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

type Seen = Arc<Mutex<Vec<(axum::http::HeaderMap, Value)>>>;

/// A stand-in gateway that keeps every report it receives.
async fn fake_gateway() -> (String, Seen) {
    let seen: Seen = Default::default();
    let keep = seen.clone();
    let app = axum::Router::new().route(
        "/internal/analytics",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, axum::Json(body): axum::Json<Value>| {
                let keep = keep.clone();
                async move {
                    keep.lock().unwrap().push((headers, body));
                    StatusCode::NO_CONTENT
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

const SECRET: &[u8] = b"instance-secret";
const USER: &str = "6f1c0b3e-1111-4222-8333-944455556666";

async fn hosted_with_analytics(url: &str) -> Served {
    let mut s = crate::common::m2::serve().await;
    let a = controld::analytics::Analytics::start(
        url.to_string(),
        USER.to_string(),
        Arc::new(SECRET.to_vec()),
    );
    s.st = s.st.clone().with_analytics(a);
    controld::analytics::watch(&s.st);
    s.router = controld::app(s.st.clone(), None);
    s
}

async fn names(seen: &Seen, want: &[&str]) -> Vec<String> {
    for _ in 0..100 {
        let got: Vec<String> = seen
            .lock()
            .unwrap()
            .iter()
            .map(|(_, b)| b["name"].as_str().unwrap_or("").to_string())
            .collect();
        if want.iter().all(|w| got.iter().any(|g| g == w)) {
            return got;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!(
        "missing some of {want:?}: {:?}",
        seen.lock()
            .unwrap()
            .iter()
            .map(|(_, b)| b["name"].clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_hosted_team_reports_its_shape_start_stop_and_run_without_names() {
    let (url, seen) = fake_gateway().await;
    let s = hosted_with_analytics(&url).await;
    let _host = crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_chain(
        &s,
        "Top Secret Plan",
        &[("a", "Agent Alpha", "launch codes")],
        &[],
    )
    .await;
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft["agents"][0]["advanced"]["check"] = json!("echo hidden");
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::m2::start_team(&s, &id).await;
    crate::common::m2::stop_team(&s, &id).await;
    let got = names(
        &seen,
        &[
            "team.created",
            "team.started",
            "team.stopped",
            "run.summary",
        ],
    )
    .await;
    assert!(got.contains(&"team.saved".to_string()), "{got:?}");
    let all = seen.lock().unwrap().clone();
    for (headers, body) in &all {
        let text = body.to_string();
        for secret in [
            "Top Secret",
            "top-secret",
            "Agent Alpha",
            "launch codes",
            "hidden",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        let h = |k: &str| headers.get(k).unwrap().to_str().unwrap().to_string();
        let ts: i64 = h("x-sculptured-ts").parse().unwrap();
        assert_eq!(h("x-sculptured-user"), USER);
        assert!(
            signing::verify(
                SECRET,
                "POST",
                "/internal/analytics",
                USER,
                ts,
                &h("x-sculptured-sig"),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            ),
            "signed"
        );
    }
    let started = all
        .iter()
        .find(|(_, b)| b["name"] == "team.started")
        .unwrap()
        .1
        .clone();
    assert_eq!(started["props"]["agents"], 1);
    assert_eq!(started["props"]["runtimes"], json!(["stub"]));
    assert_eq!(started["props"]["shape"]["checks"], true);
}

#[tokio::test]
async fn local_mode_sends_nothing() {
    let (url, seen) = fake_gateway().await;
    let _ = url;
    let s = crate::common::m2::serve().await;
    let _host = crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_chain(&s, "Plain", &[("a", "A", "job")], &[]).await;
    crate::common::m2::start_team(&s, &id).await;
    crate::common::m2::stop_team(&s, &id).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn saves_are_reported_at_most_once_per_ten_minutes() {
    let (url, seen) = fake_gateway().await;
    let s = hosted_with_analytics(&url).await;
    let id = crate::common::m2::practice_chain(&s, "Saved", &[("a", "A", "job")], &[]).await;
    for name in ["One", "Two", "Three"] {
        let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
        let mut draft = rec["draft"].clone();
        draft["name"] = json!(name);
        let (status, body) = crate::common::m2::call(
            &s.router,
            "PUT",
            &format!("/api/teams/{id}"),
            json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    names(&seen, &["team.saved"]).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let saved = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, b)| b["name"] == "team.saved")
        .count();
    assert_eq!(saved, 1);
}
