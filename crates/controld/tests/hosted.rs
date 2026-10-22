use axum::body::Body;
use axum::http::{Request, StatusCode};
use controld::AppState;
use controld::auth::AuthPolicy;
use controld::store::Store;
use std::sync::Arc;
use tower::ServiceExt;

fn hosted() -> axum::Router {
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        "t".into(),
        vec!["127.0.0.1:1".into()],
    )
    .with_auth(AuthPolicy::Gateway {
        secret: Arc::new(b"s3cret".to_vec()),
    });
    controld::app(st, None)
}

fn req(
    method: &str,
    path: &str,
    user: Option<&str>,
    secret: &[u8],
    ts_shift: i64,
) -> Request<Body> {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:1");
    if let Some(u) = user {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            + ts_shift;
        b = b
            .header(signing::USER_HEADER, u)
            .header(signing::TS_HEADER, ts.to_string())
            .header(
                signing::SIG_HEADER,
                signing::sign(secret, method, path, u, ts),
            );
    }
    b.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn hosted_mode_accepts_only_fresh_gateway_signed_requests() {
    let r = hosted();
    assert_eq!(
        r.clone()
            .oneshot(req("GET", "/api/teams", Some("u1"), b"s3cret", 0))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        r.clone()
            .oneshot(req("GET", "/api/teams", None, b"", 0))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        r.clone()
            .oneshot(req("GET", "/api/teams", Some("u1"), b"wrong", 0))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        r.clone()
            .oneshot(req("GET", "/api/teams", Some("u1"), b"s3cret", -120))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        r.clone()
            .oneshot(req("GET", "/host", None, b"", 0))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED,
        "no exemption for /host in hosted mode"
    );
}

#[tokio::test]
async fn with_no_computer_connected_nothing_touches_the_servers_own_files() {
    let r = hosted();
    for path in ["/api/folders", "/api/clis"] {
        let res = r
            .clone()
            .oneshot(req("GET", path, Some("u1"), b"s3cret", 0))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::CONFLICT,
            "{path}: needs a computer"
        );
    }
    for path in ["/api/teams/x/git", "/api/teams/x/copies"] {
        let res = r
            .clone()
            .oneshot(req("GET", path, Some("u1"), b"s3cret", 0))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{path}: no such team");
    }
    let res = r
        .clone()
        .oneshot(req("GET", "/api/plan-usage", Some("u1"), b"s3cret", 0))
        .await
        .unwrap();
    assert_eq!(
        res.status(),
        StatusCode::OK,
        "agent-session readings only, never this server's Codex files"
    );
    let res = r
        .clone()
        .oneshot(req(
            "POST",
            "/api/seats/a@x/check",
            Some("u1"),
            b"s3cret",
            0,
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn hosted_mode_refuses_reading_spec_files_textbook_setup_and_host_metrics() {
    let r = hosted();
    for (method, path) in [
        ("POST", "/api/rigs"),
        ("POST", "/api/teams/x/textbook/prepare"),
        ("GET", "/api/system/metrics"),
    ] {
        let res = r
            .clone()
            .oneshot(req(method, path, Some("u1"), b"s3cret", 0))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::CONFLICT, "{method} {path}");
    }
    let res = r
        .clone()
        .oneshot(req("GET", "/api/rigs", Some("u1"), b"s3cret", 0))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK, "listing teams still works");
}

#[test]
fn hosted_mode_never_runs_check_commands_on_the_server() {
    let local = AppState::new(Store::open_in_memory().unwrap(), "t".into(), vec![]);
    assert!(controld::checks::allowed(&local));
    let hosted = local.with_auth(AuthPolicy::Gateway {
        secret: Arc::new(b"s".to_vec()),
    });
    assert!(
        !controld::checks::allowed(&hosted),
        "a finished turn must not run `sh -c` on the hosted server"
    );
}

fn signed_json(method: &str, path: &str, body: serde_json::Value) -> Request<Body> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:1")
        .header("content-type", "application/json")
        .header(signing::USER_HEADER, "u1")
        .header(signing::TS_HEADER, ts.to_string())
        .header(
            signing::SIG_HEADER,
            signing::sign(b"s3cret", method, path, "u1", ts),
        )
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn json_of(res: axum::response::Response) -> serde_json::Value {
    use http_body_util::BodyExt;
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap_or_default()
}

#[tokio::test]
async fn hosted_teams_may_use_folders_on_the_users_computer_not_the_servers_home() {
    let r = hosted();
    let rec = json_of(
        r.clone()
            .oneshot(signed_json(
                "POST",
                "/api/teams",
                serde_json::json!({"request_id": "r1", "template": "solo", "practice": true}),
            ))
            .await
            .unwrap(),
    )
    .await;
    let mut draft = rec["draft"].clone();
    let id = draft["id"].as_str().unwrap().to_string();
    draft["folder"] = serde_json::json!("/Users/someone/proj");
    let res = r
        .clone()
        .oneshot(signed_json(
            "PUT",
            &format!("/api/teams/{id}"),
            serde_json::json!({"request_id": "r2", "revision": draft["revision"], "draft": draft}),
        ))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = r
        .clone()
        .oneshot(signed_json(
            "POST",
            &format!("/api/teams/{id}/start"),
            serde_json::json!({"request_id": "r3"}),
        ))
        .await
        .unwrap();
    let body = json_of(res).await;
    assert_eq!(
        body["error"]["code"], "needs_computer",
        "the folder is the computer's to check, not the server's: {body}"
    );
}

#[tokio::test]
async fn with_several_computers_and_no_choice_start_asks_which_one() {
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        "t".into(),
        vec!["127.0.0.1:1".into()],
    )
    .with_auth(AuthPolicy::Gateway {
        secret: Arc::new(b"s3cret".to_vec()),
    });
    st.hosts.test_mark_connected("dev-laptop");
    st.hosts.test_mark_connected("dev-desktop");
    let r = controld::app(st, None);
    let rec = json_of(
        r.clone()
            .oneshot(signed_json(
                "POST",
                "/api/teams",
                serde_json::json!({"request_id": "r1", "template": "solo", "practice": true}),
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = rec["draft"]["id"].as_str().unwrap().to_string();
    let body = json_of(
        r.clone()
            .oneshot(signed_json(
                "POST",
                &format!("/api/teams/{id}/start"),
                serde_json::json!({"request_id": "r2"}),
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(body["error"]["code"], "needs_computer", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Choose which computer"),
        "{body}"
    );
}

/// In the hosted beta, folders and agents are on the user's computer: without one connected, the
/// board says so in Things to fix (the team needs setup), naming where to connect it.
#[tokio::test]
async fn a_hosted_team_without_a_connected_computer_needs_setup() {
    let r = hosted();
    let rec = json_of(
        r.clone()
            .oneshot(signed_json(
                "POST",
                "/api/teams",
                serde_json::json!({"request_id": "c1", "template": "solo", "practice": true}),
            ))
            .await
            .unwrap(),
    )
    .await;
    let issue = rec["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["code"] == "computer-not-connected")
        .cloned();
    let issue = issue.unwrap_or_else(|| panic!("no computer issue in {}", rec["issues"]));
    assert_eq!(issue["element"]["kind"], "team");
    assert_eq!(issue["severity"], "error");
    assert!(
        issue["message"].as_str().unwrap().contains("Computers"),
        "{issue}"
    );
}

/// Running Sculptured on your own computer needs no connected computer.
#[tokio::test]
async fn a_local_team_never_asks_for_a_computer() {
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        "t".into(),
        vec!["127.0.0.1:1".into()],
    );
    let r = controld::app(st, None);
    let req = Request::builder()
        .method("POST")
        .uri("/api/teams")
        .header("host", "127.0.0.1:1")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"request_id": "l1", "template": "solo", "practice": true})
                .to_string(),
        ))
        .unwrap();
    let rec = json_of(r.oneshot(req).await.unwrap()).await;
    assert!(
        rec["issues"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["code"] != "computer-not-connected"),
        "{}",
        rec["issues"]
    );
}
