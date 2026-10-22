mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use controld::AppState;
use controld::store::Store;
use host_proto::HostEvent;
use http_body_util::BodyExt;
use model::{Actor, EventBody, Provenance, SeatLifecycle, Subject};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::broadcast;
use tower::ServiceExt;

const GOOD: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: stub, cwd: "." }
    edges: [{ kind: delegates_to, from: a, to: b }]
"#;

fn setup() -> (AppState, Router) {
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    );
    let router = controld::app(st.clone(), None);
    (st, router)
}

fn spec_file(dir: &tempfile::TempDir, yaml: &str) -> String {
    let p = dir.path().join("rig.yaml");
    std::fs::write(&p, yaml).unwrap();
    p.display().to_string()
}

async fn call_with(
    router: &Router,
    method: &str,
    uri: &str,
    host: &str,
    origin: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host);
    if let Some(o) = origin {
        req = req.header("origin", o);
    }
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let resp = router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    call_with(router, method, uri, "test", None, body).await
}

#[tokio::test]
async fn invalid_spec_returns_422_with_diagnostic_paths() {
    let (_st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = spec_file(
        &dir,
        &GOOD.replace(
            "runtime: stub, cwd: \".\" }\n      - { id: b",
            "runtime: gpt, cwd: \".\" }\n      - { id: b",
        ),
    );
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "r1", "spec_path": path })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "spec_invalid");
    let paths: Vec<&str> = body["error"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"pods[0].members[0].runtime"), "{paths:?}");
}

#[tokio::test]
async fn unreadable_spec_is_400() {
    let (_st, router) = setup();
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "r1", "spec_path": "/no/such/rig.yaml" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "spec_unreadable");
}

#[tokio::test]
async fn missing_request_id_is_400() {
    let (_st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "spec_path": spec_file(&dir, GOOD) })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "request_id_required");
}

#[tokio::test]
async fn foreign_host_or_origin_is_403() {
    let (_st, router) = setup();
    let (status, body) = call_with(&router, "GET", "/api/rigs", "evil.example", None, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "forbidden_host");
    let (status, _) = call_with(
        &router,
        "GET",
        "/api/rigs",
        "test",
        Some("http://evil.example"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call_with(
        &router,
        "GET",
        "/api/rigs",
        "test",
        Some("http://test"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn unknown_rig_is_404() {
    let (_st, router) = setup();
    let (status, body) = call(&router, "GET", "/api/rigs/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
    // An unknown API path gets the JSON envelope, not the web UI fallback.
    let (status, body) = call(&router, "GET", "/api/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn boot_replays_same_request_and_conflicts_while_running() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = spec_file(&dir, GOOD);
    let req = json!({ "request_id": "boot-1", "spec_path": path });
    let first = call(&router, "POST", "/api/rigs", Some(req.clone())).await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(first.1["rig"], "r");
    assert_eq!(call(&router, "POST", "/api/rigs", Some(req)).await, first);
    let booted = st
        .store
        .lock()
        .unwrap()
        .events_for_subject(&Subject::Rig("r".into()), 0, 10)
        .unwrap();
    assert_eq!(
        booted
            .iter()
            .filter(|e| matches!(e.body, EventBody::RigBooted { .. }))
            .count(),
        1
    );

    // No host is connected, so the background boot marks seats attention; wait for it to finish.
    crate::common::wait_for("boot settled", || {
        crate::common::lifecycle(&st, "dev.b@r") == Some(SeatLifecycle::Attention)
    })
    .await;
    st.store
        .lock()
        .unwrap()
        .ingest(
            "local",
            HostEvent {
                host_seq: 1,
                at_ms: 1,
                actor: Actor::System("hostd".into()),
                subject: Subject::Seat("dev.a@r".into()),
                provenance: Provenance::ProtocolObserved,
                body: EventBody::SeatLifecycleChanged {
                    lifecycle: SeatLifecycle::Running,
                    reason: None,
                },
            },
        )
        .unwrap();
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "boot-2", "spec_path": path })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "rig_running");

    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "boot-1", "spec_path": "/other.yaml" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "request_id_reused");
}

#[tokio::test]
async fn concurrent_duplicate_send_executes_once() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let (status, _) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "b", "spec_path": spec_file(&dir, GOOD) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body =
        json!({ "request_id": "same", "text": "hi", "intent": "inform", "interrupt": false });
    let (one, two) = tokio::join!(
        call(
            &router,
            "POST",
            "/api/seats/dev.a@r/send",
            Some(body.clone())
        ),
        call(
            &router,
            "POST",
            "/api/seats/dev.a@r/send",
            Some(body.clone())
        ),
    );
    assert_eq!(one, two);
    assert_eq!(
        one.0,
        StatusCode::BAD_GATEWAY,
        "no host is connected: {:?}",
        one.1
    );
    assert_eq!(one.1["error"]["code"], "host_unreachable");
    let queued = st
        .store
        .lock()
        .unwrap()
        .events_for_seat("dev.a@r", 0, 100)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.body, EventBody::MessageQueued { .. }))
        .count();
    assert_eq!(queued, 1);
}

#[tokio::test]
async fn list_get_and_seat_events() {
    let (_st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "b", "spec_path": spec_file(&dir, GOOD) })),
    )
    .await;
    let (status, rigs) = call(&router, "GET", "/api/rigs", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rigs[0]["name"], "r");
    assert_eq!(rigs[0]["seat_count"], 2);
    let (status, snap) = call(&router, "GET", "/api/rigs/r", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snap["seats"][0]["id"], "dev.a@r");
    assert_eq!(snap["rig"]["launch_layers"][0][0], "dev.a@r");
    let (status, events) = call(
        &router,
        "GET",
        "/api/seats/dev.a@r/events?after=0&limit=10",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(events.is_array());
    let (status, newest) = call(&router, "GET", "/api/seats/dev.a@r/events?tail=10", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(newest.is_array());
}

#[tokio::test]
async fn stream_starts_with_a_snapshot() {
    let (_st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "b", "spec_path": spec_file(&dir, GOOD) })),
    )
    .await;
    let req = Request::builder()
        .uri("/api/stream?rig=r")
        .header("host", "test")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    let mut body = resp.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(text.starts_with("event: snapshot\n"), "{text}");
    assert!(text.contains("\"launch_layers\""));

    let (status, _) = call(&router, "GET", "/api/stream?rig=nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

async fn boot_r(router: &Router, dir: &tempfile::TempDir, request_id: &str) {
    let (status, _) = call(
        router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": request_id, "spec_path": spec_file(dir, GOOD) })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

fn rigbooted_count(st: &AppState) -> usize {
    st.store
        .lock()
        .unwrap()
        .events_for_subject(&Subject::Rig("r".into()), 0, 100)
        .unwrap()
        .iter()
        .filter(|e| matches!(e.body, EventBody::RigBooted { .. }))
        .count()
}

#[tokio::test]
async fn boot_while_a_boot_is_claimed_is_409_and_leaves_the_spec_alone() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    boot_r(&router, &dir, "b1").await;
    crate::common::wait_for("first boot released its claim", || !st.is_booting("r")).await;
    let (_, before) = call(&router, "GET", "/api/rigs/r", None).await;

    let claim = st.try_begin_boot("r").expect("claim is free");
    let changed = GOOD.replace("label: Dev", "label: Changed");
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "b2", "spec_path": spec_file(&dir, &changed) })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "rig_running");
    let (_, after) = call(&router, "GET", "/api/rigs/r", None).await;
    assert_eq!(before["rig"]["spec_hash"], after["rig"]["spec_hash"]);
    assert_eq!(before["rig"], after["rig"]);
    assert_eq!(rigbooted_count(&st), 1);
    drop(claim);
    assert!(
        st.try_begin_boot("r").is_some(),
        "dropping the guard releases the claim"
    );
}

#[tokio::test]
async fn concurrent_boots_with_different_request_ids_only_one_wins() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = spec_file(&dir, GOOD);
    let (one, two) = tokio::join!(
        call(
            &router,
            "POST",
            "/api/rigs",
            Some(json!({ "request_id": "x1", "spec_path": path }))
        ),
        call(
            &router,
            "POST",
            "/api/rigs",
            Some(json!({ "request_id": "x2", "spec_path": path }))
        ),
    );
    let mut statuses = vec![one.0, two.0];
    statuses.sort();
    assert_eq!(
        statuses,
        vec![StatusCode::OK, StatusCode::CONFLICT],
        "{one:?} {two:?}"
    );
    assert_eq!(rigbooted_count(&st), 1);
}

fn local(st: &AppState, subject: Subject, text: &str) -> controld::views::StreamItem {
    st.local_event(
        Actor::Human("local".into()),
        subject,
        Provenance::Operator,
        EventBody::MessageQueued {
            message_id: text.into(),
            intent: model::MessageIntent::Inform,
            text: text.into(),
        },
    )
    .unwrap()
}

async fn next_frame(body: &mut Body) -> String {
    let frame = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .expect("frame in time")
        .unwrap()
        .unwrap();
    String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap()
}

async fn open_stream(router: &Router) -> Body {
    let req = Request::builder()
        .uri("/api/stream?rig=r")
        .header("host", "test")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut body = resp.into_body();
    assert!(next_frame(&mut body).await.starts_with("event: snapshot\n"));
    body
}

#[tokio::test]
async fn stream_delivers_this_rigs_events_with_seq_ids_and_filters_the_rest() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    boot_r(&router, &dir, "b").await;
    crate::common::wait_for("boot settled", || !st.is_booting("r")).await;
    let covered = local(&st, Subject::Seat("dev.a@r".into()), "covered");
    let mut body = open_stream(&router).await;

    // Not delivered: another rig's events, and an event the snapshot already covers.
    local(&st, Subject::Seat("x.y@other".into()), "foreign-seat");
    local(&st, Subject::Rig("other".into()), "foreign-rig");
    let _ = st.hub.send(covered.clone());
    let fresh = local(&st, Subject::Seat("dev.a@r".into()), "fresh");
    assert!(fresh.event.seq > covered.event.seq);

    let frame = next_frame(&mut body).await;
    assert!(frame.starts_with("event: event\n"), "{frame}");
    assert!(
        frame.contains(&format!("id: {}\n", fresh.event.seq)),
        "{frame}"
    );
    assert!(frame.contains("\"fresh\""), "{frame}");
    assert!(
        !frame.contains("foreign") && !frame.contains("covered"),
        "{frame}"
    );
}

#[tokio::test]
async fn stream_sends_resync_when_the_client_lags() {
    let mut st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    );
    st.hub = broadcast::channel(2).0;
    let router = controld::app(st.clone(), None);
    let dir = tempfile::tempdir().unwrap();
    boot_r(&router, &dir, "b").await;
    crate::common::wait_for("boot settled", || !st.is_booting("r")).await;
    let mut body = open_stream(&router).await;
    for i in 0..6 {
        local(&st, Subject::Seat("dev.a@r".into()), &format!("m{i}"));
    }
    let frame = next_frame(&mut body).await;
    assert!(frame.starts_with("event: resync\n"), "{frame}");
}

async fn open_all(router: &Router) -> Body {
    let req = Request::builder()
        .uri("/api/stream?all=true")
        .header("host", "test")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    resp.into_body()
}

fn frame_json(frame: &str) -> Value {
    let data = frame
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .unwrap_or_else(|| panic!("no data in {frame}"));
    serde_json::from_str(data).unwrap()
}

fn named_spec(dir: &tempfile::TempDir, name: &str, yaml: &str) -> String {
    let p = dir.path().join(format!("{name}.yaml"));
    std::fs::write(&p, yaml.replace("name: r\n", &format!("name: {name}\n"))).unwrap();
    p.display().to_string()
}

async fn boot_named(router: &Router, dir: &tempfile::TempDir, name: &str, request_id: &str) {
    let path = named_spec(dir, name, GOOD);
    let (status, body) = call(
        router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": request_id, "spec_path": path })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn one_stream_for_every_team_sends_a_snapshot_per_rig_then_ready() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let indexed_items = ["r", "r2", "r3"].iter().enumerate();
    for (i, name) in indexed_items {
        boot_named(&router, &dir, name, &format!("b{i}")).await;
        crate::common::wait_for("boot settled", || !st.is_booting(name)).await;
    }
    let mut body = open_all(&router).await;
    let mut names = Vec::new();
    for _ in 0..3 {
        let frame = next_frame(&mut body).await;
        assert!(frame.starts_with("event: snapshot\n"), "{frame}");
        names.push(
            frame_json(&frame)["rig"]["name"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    assert_eq!(names, ["r", "r2", "r3"]);
    let frame = next_frame(&mut body).await;
    assert!(frame.starts_with("event: ready\n"), "{frame}");
}

#[tokio::test]
async fn one_stream_for_every_team_tags_each_event_with_its_rig_and_skips_covered_ones() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    boot_named(&router, &dir, "r", "b1").await;
    boot_named(&router, &dir, "r2", "b2").await;
    crate::common::wait_for("boots settled", || {
        !st.is_booting("r") && !st.is_booting("r2")
    })
    .await;
    let covered = local(&st, Subject::Seat("dev.a@r".into()), "covered");
    let mut body = open_all(&router).await;
    for _ in 0..2 {
        assert!(next_frame(&mut body).await.starts_with("event: snapshot\n"));
    }
    assert!(next_frame(&mut body).await.starts_with("event: ready\n"));

    let _ = st.hub.send(covered.clone());
    let a = local(&st, Subject::Seat("dev.a@r".into()), "on-r");
    let b = local(&st, Subject::Rig("r2".into()), "on-r2");
    let h = local(&st, Subject::Host("h1".into()), "on-host");
    for (want, rig, text) in [
        (a, json!("r"), "on-r"),
        (b, json!("r2"), "on-r2"),
        (h, Value::Null, "on-host"),
    ] {
        let frame = next_frame(&mut body).await;
        assert!(frame.starts_with("event: event\n"), "{frame}");
        assert!(
            frame.contains(&format!("id: {}\n", want.event.seq)),
            "{frame}"
        );
        assert!(!frame.contains("\"covered\""), "{frame}");
        let v = frame_json(&frame);
        assert_eq!(v["rig"], rig, "{frame}");
        assert_eq!(v["event"]["body"]["text"], text, "{frame}");
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn one_stream_for_every_team_sends_a_snapshot_when_a_team_starts_and_restarts() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let mut body = open_all(&router).await;
    assert!(
        next_frame(&mut body).await.starts_with("event: ready\n"),
        "no teams: ready at once"
    );

    boot_named(&router, &dir, "r", "b1").await;
    let frame = next_frame(&mut body).await;
    assert!(
        frame.starts_with("event: snapshot\n"),
        "a new team arrives as a snapshot: {frame}"
    );
    let snap = frame_json(&frame);
    assert_eq!(snap["rig"]["name"], "r");
    assert_eq!(snap["seats"].as_array().unwrap().len(), 2);
    crate::common::wait_for("boot settled", || !st.is_booting("r")).await;
    // Events after that snapshot follow, tagged; the boot itself is in the snapshot.
    let later = local(&st, Subject::Seat("dev.a@r".into()), "later");
    loop {
        let frame = next_frame(&mut body).await;
        assert!(
            !frame.contains("\"rig-booted\""),
            "the snapshot covers the boot: {frame}"
        );
        if frame.contains("\"later\"") {
            assert_eq!(frame_json(&frame)["rig"], "r");
            assert!(
                frame.contains(&format!("id: {}\n", later.event.seq)),
                "{frame}"
            );
            break;
        }
    }

    // Without a host the seats end in attention (not live), so the team can start again.
    // A restart with a changed spec sends the new shape as a fresh snapshot.
    let one_agent = GOOD
        .replace("      - { id: b, agent_ref: \"local:b\", profile: default, runtime: stub, cwd: \".\" }\n", "")
        .replace("    edges: [{ kind: delegates_to, from: a, to: b }]\n", "");
    let path = named_spec(&dir, "r", &one_agent);
    let (status, v) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "b2", "spec_path": path })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    loop {
        let frame = next_frame(&mut body).await;
        if frame.starts_with("event: snapshot\n") {
            let snap = frame_json(&frame);
            assert_eq!(snap["rig"]["name"], "r");
            assert_eq!(snap["seats"].as_array().unwrap().len(), 1, "{frame}");
            break;
        }
    }
}

#[tokio::test]
async fn one_stream_for_every_team_sends_resync_when_the_client_lags() {
    let mut st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    );
    st.hub = broadcast::channel(2).0;
    let router = controld::app(st.clone(), None);
    let dir = tempfile::tempdir().unwrap();
    boot_r(&router, &dir, "b").await;
    crate::common::wait_for("boot settled", || !st.is_booting("r")).await;
    let mut body = open_all(&router).await;
    assert!(next_frame(&mut body).await.starts_with("event: snapshot\n"));
    assert!(next_frame(&mut body).await.starts_with("event: ready\n"));
    for i in 0..6 {
        local(&st, Subject::Seat("dev.a@r".into()), &format!("m{i}"));
    }
    let frame = next_frame(&mut body).await;
    assert!(frame.starts_with("event: resync\n"), "{frame}");
}

#[tokio::test]
async fn the_stream_takes_one_rig_or_all_but_not_both() {
    let (_st, router) = setup();
    for uri in [
        "/api/stream?rig=r&all=true",
        "/api/stream?all=false",
        "/api/stream?all=maybe",
    ] {
        let (status, v) = raw(&router, "GET", uri, None, String::new()).await;
        assert_eq!(
            (status, v["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("bad_query")),
            "{uri}: {v}"
        );
    }
}

#[tokio::test]
async fn tail_returns_exactly_the_newest_n_oldest_first() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    boot_r(&router, &dir, "b").await;
    crate::common::wait_for("boot settled", || !st.is_booting("r")).await;
    let seqs: Vec<i64> = (0..6)
        .map(|i| {
            local(&st, Subject::Seat("dev.a@r".into()), &format!("m{i}"))
                .event
                .seq
        })
        .collect();
    let (status, tail) = call(&router, "GET", "/api/seats/dev.a@r/events?tail=3", None).await;
    assert_eq!(status, StatusCode::OK);
    let got: Vec<i64> = tail
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["seq"].as_i64().unwrap())
        .collect();
    assert_eq!(got, seqs[3..].to_vec(), "newest three, oldest first");
}

#[tokio::test]
async fn request_locks_are_released_after_requests_finish() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    boot_r(&router, &dir, "b").await;
    let body =
        json!({ "request_id": "same", "text": "hi", "intent": "inform", "interrupt": false });
    let _ = tokio::join!(
        call(
            &router,
            "POST",
            "/api/seats/dev.a@r/send",
            Some(body.clone())
        ),
        call(
            &router,
            "POST",
            "/api/seats/dev.a@r/send",
            Some(body.clone())
        ),
    );
    call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "bad", "spec_path": "/no/such" })),
    )
    .await;
    assert!(
        st.request_locks.lock().unwrap().is_empty(),
        "{:?}",
        st.request_locks.lock().unwrap().keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn dropped_client_still_completes_action_and_saves_receipt() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    boot_r(&router, &dir, "b").await;
    let body =
        json!({ "request_id": "drop-1", "text": "hi", "intent": "inform", "interrupt": false });
    let req = Request::builder()
        .method("POST")
        .uri("/api/seats/dev.a@r/send")
        .header("host", "test")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let mut fut = Box::pin(router.clone().oneshot(req));
    // One poll takes the lock and spawns the action; the spawned task has not run yet.
    assert!(
        futures::poll!(fut.as_mut()).is_pending(),
        "request must still be in flight when dropped"
    );
    drop(fut);
    crate::common::wait_for("receipt saved", || {
        st.store
            .lock()
            .unwrap()
            .receipt("drop-1")
            .unwrap()
            .is_some()
    })
    .await;
    let queued = st
        .store
        .lock()
        .unwrap()
        .events_for_seat("dev.a@r", 0, 100)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.body, EventBody::MessageQueued { .. }))
        .count();
    assert_eq!(queued, 1);
    crate::common::wait_for("lock released", || {
        st.request_locks.lock().unwrap().is_empty()
    })
    .await;
}

#[tokio::test]
async fn guard_rejects_missing_host_null_origin_and_guards_the_fallback() {
    let (st, router) = setup();
    let req = Request::builder()
        .uri("/api/rigs")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let (status, _) = call_with(&router, "GET", "/api/rigs", "test", Some("null"), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let fallback = Router::new().fallback(|| async { "web ui" });
    let with_ui = controld::app(st, Some(fallback));
    let req = Request::builder()
        .uri("/some/deep/link")
        .header("host", "evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        with_ui.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let req = Request::builder()
        .uri("/some/deep/link")
        .header("host", "test")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        with_ui.clone().oneshot(req).await.unwrap().status(),
        StatusCode::OK
    );
    // API paths never reach the fallback, including the bare prefix.
    for uri in ["/api", "/api/", "/api/nope"] {
        let req = Request::builder()
            .uri(uri)
            .header("host", "test")
            .body(Body::empty())
            .unwrap();
        let resp = with_ui.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{uri}");
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        assert_eq!(v["error"]["code"], "not_found", "{uri}");
    }
}

// ---- final fix wave ----

/// I1: a spec path pointed at a secret file returns 422 without echoing it, and the
/// stored receipt (replayed on retry) does not hold it either.
#[tokio::test]
async fn spec_path_at_a_secret_file_does_not_echo_or_store_it() {
    let files = [
        (
            "env",
            "API_KEY=sk-live-SECRET123\nDB_PASSWORD=hunter2\n",
            vec!["sk-live-SECRET123", "hunter2"],
        ),
        (
            "ssh",
            "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\nQyNTUxOQAAACBTOPSECRETKEYMATERIALxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxAAA\n-----END OPENSSH PRIVATE KEY-----\n",
            vec!["b3BlbnNzaC1rZXktdjEAAAAA", "SECRETKEYMATERIAL"],
        ),
    ];
    for (name, content, secrets) in files {
        let (st, router) = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = spec_file(&dir, content);
        let request_id = format!("secret-{name}");
        let (status, body) = call(
            &router,
            "POST",
            "/api/rigs",
            Some(json!({ "request_id": request_id, "spec_path": path })),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(
            body["error"]["diagnostics"][0]["message"],
            "not a team spec (expected a YAML mapping)"
        );
        let receipt = st
            .store
            .lock()
            .unwrap()
            .receipt(&request_id)
            .unwrap()
            .expect("422 stored as a receipt");
        for secret in secrets {
            assert!(
                !body.to_string().contains(secret),
                "{name}: 422 body echoes {secret}: {body}"
            );
            assert!(
                !receipt.body.contains(secret),
                "{name}: stored receipt holds {secret}: {}",
                receipt.body
            );
        }
    }
}

/// Stopping while the runner is offline is not a failure: each agent waits in `stopping` and is
/// stopped when the runner reconnects (`offline_computer.rs`).
#[tokio::test]
async fn stop_rig_with_the_runner_offline_stops_its_agents_here_at_once() {
    let (st, router) = setup();
    let rig = rigspec::compile(GOOD).unwrap();
    st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            model::Desired::Running,
        )
        .unwrap();
    for seat in ["dev.a@r", "dev.b@r"] {
        st.local_event(
            Actor::System("test".into()),
            Subject::Seat(seat.into()),
            Provenance::ProtocolObserved,
            EventBody::SeatLifecycleChanged {
                lifecycle: SeatLifecycle::Running,
                reason: None,
            },
        )
        .unwrap();
    }
    // No host is connected, so neither stop can reach a process yet.
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs/r/stop",
        Some(json!({ "request_id": "stop-1" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({ "failed": [] }));
    for seat in ["dev.a@r", "dev.b@r"] {
        let v = st.store.lock().unwrap().seat(seat).unwrap().unwrap();
        assert_eq!(
            v.lifecycle,
            SeatLifecycle::Stopped,
            "a computer that may never return cannot hold the stop open"
        );
        assert!(
            v.lifecycle_reason
                .as_deref()
                .is_some_and(|r| r.contains("its computer is offline")),
            "{:?}",
            v.lifecycle_reason
        );
    }
}

#[tokio::test]
async fn stop_rig_with_nothing_to_stop_reports_no_failures() {
    let (st, router) = setup();
    let rig = rigspec::compile(GOOD).unwrap();
    st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            model::Desired::Running,
        )
        .unwrap();
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs/r/stop",
        Some(json!({ "request_id": "stop-2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "failed": [] }));
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn the_auth_seam_covers_every_route_except_host() {
    let mut st = AppState::new(
        Store::open_in_memory().unwrap(),
        "tok".into(),
        vec!["test".into()],
    );
    st.auth = controld::auth::AuthPolicy::DenyAll;
    let router = controld::app(st, Some(Router::new().fallback(|| async { "web ui" })));
    for (method, uri) in [
        ("GET", "/api/rigs"),
        ("POST", "/api/rigs"),
        ("GET", "/api/rigs/r"),
        ("POST", "/api/rigs/r/stop"),
        ("GET", "/api/stream?rig=r"),
        ("GET", "/api/stream?all=true"),
        ("GET", "/api/seats/dev.a@r/events?tail=1"),
        ("POST", "/api/seats/dev.a@r/send"),
        ("POST", "/api/seats/dev.a@r/interrupt"),
        ("POST", "/api/seats/dev.a@r/stop"),
        ("GET", "/api/nope"),
        ("GET", "/"),
        ("GET", "/team/x/edit"),
    ] {
        let body = (method == "POST").then(|| json!({ "request_id": "x", "spec_path": "/x" }));
        let (status, body) = call(&router, method, uri, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(body["error"]["code"], "login_required", "{method} {uri}");
    }
    let req = Request::builder()
        .uri("/host")
        .header("host", "test")
        .header("authorization", "Bearer tok")
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "/host keeps its own bearer auth and skips the seam"
    );
}

#[tokio::test]
async fn a_spec_file_cannot_take_a_team_rigs_name() {
    let (st, router) = setup();
    let dir = tempfile::tempdir().unwrap();
    let rig = rigspec::compile(GOOD).unwrap();
    st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &controld::store::RigSource::Team {
                id: "r".into(),
                revision: 1,
            },
            model::Desired::Stopped,
        )
        .unwrap();
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        Some(json!({ "request_id": "t1", "spec_path": spec_file(&dir, GOOD) })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "rig_name_taken");
}

#[tokio::test]
async fn start_rig_claims_synchronously_and_conflicts_while_claimed() {
    let (st, _router) = setup();
    let rig = rigspec::compile(GOOD).unwrap();
    let source = controld::store::RigSource::File {
        path: "/r.yaml".into(),
    };
    let claim = st.try_begin_boot("r").expect("free");
    let err =
        controld::reconciler::start_rig(&st, rig.clone(), source.clone(), std::env::temp_dir())
            .unwrap_err();
    assert_eq!(err.code, "rig_running");
    drop(claim);
    let body = controld::reconciler::start_rig(&st, rig, source, std::env::temp_dir()).unwrap();
    assert_eq!(body["rig"], "r");
    assert_eq!(
        st.store.lock().unwrap().rig_source("r").unwrap(),
        Some(controld::store::RigSource::File {
            path: "/r.yaml".into()
        })
    );
}

// ---- Backend hardening: every rejection uses the error envelope ----

async fn raw(
    router: &Router,
    method: &str,
    uri: &str,
    content_type: Option<&str>,
    body: String,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "test");
    if let Some(ct) = content_type {
        req = req.header("content-type", ct);
    }
    let resp = router
        .clone()
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({ "not_json": String::from_utf8_lossy(&bytes) })),
    )
}

/// Every route that reads a JSON body.
const JSON_ROUTES: [(&str, &str); 12] = [
    ("POST", "/api/rigs"),
    ("POST", "/api/rigs/r/stop"),
    ("POST", "/api/seats/dev.a@r/send"),
    ("POST", "/api/seats/dev.a@r/interrupt"),
    ("POST", "/api/seats/dev.a@r/stop"),
    ("POST", "/api/teams"),
    ("PUT", "/api/teams/x"),
    ("POST", "/api/teams/x/validate"),
    ("POST", "/api/teams/import"),
    ("POST", "/api/teams/x/start"),
    ("POST", "/api/teams/x/stop"),
    ("POST", "/api/folders"),
];

#[tokio::test]
async fn every_body_rejection_is_an_api_error_with_a_code() {
    let (_st, router) = setup();
    let json = Some("application/json");
    let too_big = format!(
        "{{\"request_id\":\"big\",\"yaml\":\"{}\"}}",
        "a".repeat(3 * 1024 * 1024)
    );
    for (method, uri) in JSON_ROUTES {
        for (content_type, body, status, code) in [
            (
                json,
                "{not json".to_string(),
                StatusCode::BAD_REQUEST,
                "bad_json",
            ),
            (
                json,
                "\"a string, not an object\"".to_string(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "bad_body",
            ),
            (
                None,
                "{}".to_string(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "json_required",
            ),
            (
                Some("text/plain"),
                "{}".to_string(),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "json_required",
            ),
            (
                json,
                too_big.clone(),
                StatusCode::PAYLOAD_TOO_LARGE,
                "body_too_large",
            ),
        ] {
            let (got, v) = raw(&router, method, uri, content_type, body).await;
            assert_eq!(
                (got, v["error"]["code"].as_str()),
                (status, Some(code)),
                "{method} {uri} {content_type:?}: {v}"
            );
            assert!(
                v["error"]["message"]
                    .as_str()
                    .is_some_and(|m| !m.is_empty()),
                "{method} {uri}: {v}"
            );
        }
    }
}

#[tokio::test]
async fn every_query_rejection_is_an_api_error_with_a_code() {
    let (_st, router) = setup();
    for uri in [
        "/api/folders?path=%2Fa&path=%2Fb",
        "/api/stream",
        "/api/stream?rig=a&rig=b",
        "/api/seats/dev.a@r/events?after=soon",
        "/api/seats/dev.a@r/events?tail=-1",
    ] {
        let (status, v) = raw(&router, "GET", uri, None, String::new()).await;
        assert_eq!(
            (status, v["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("bad_query")),
            "{uri}: {v}"
        );
    }
}

#[tokio::test]
async fn path_rejections_use_the_envelope() {
    let (_st, router) = setup();
    for uri in [
        "/api/teams/%FF",
        "/api/rigs/%FF",
        "/api/seats/%FF/events",
        "/api/teams/%FF/spec",
    ] {
        let (status, v) = raw(&router, "GET", uri, None, String::new()).await;
        assert_eq!(
            (status, v["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("bad_url")),
            "{uri}: {v}"
        );
    }
}

#[tokio::test]
async fn a_wrong_method_is_405_with_the_envelope() {
    let (_st, router) = setup();
    for (method, uri) in [
        ("DELETE", "/api/teams"),
        ("PATCH", "/api/rigs"),
        ("GET", "/api/teams/x/start"),
        ("PUT", "/api/folders"),
    ] {
        let (status, v) = raw(&router, method, uri, None, String::new()).await;
        assert_eq!(
            (status, v["error"]["code"].as_str()),
            (StatusCode::METHOD_NOT_ALLOWED, Some("method_not_allowed")),
            "{method} {uri}: {v}"
        );
    }
}

#[tokio::test]
async fn a_teams_recent_events_come_in_one_request_with_long_text_shortened() {
    let (st, router) = setup();
    let rig = rigspec::compile(GOOD).unwrap();
    st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            model::Desired::Running,
        )
        .unwrap();
    let long = "x".repeat(5000);
    for (seat, text) in [
        ("dev.a@r", "short"),
        ("dev.a@r", long.as_str()),
        ("dev.b@r", "hello"),
    ] {
        st.local_event(
            Actor::System("test".into()),
            Subject::Seat(seat.into()),
            Provenance::AgentReported,
            EventBody::AgentText {
                turn_id: None,
                text: text.into(),
            },
        )
        .unwrap();
    }
    let (status, body) = call(&router, "GET", "/api/teams/r/recent?tail=60", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let a = body["dev.a@r"].as_array().expect("each agent's events");
    assert_eq!(a.len(), 2);
    let cut = a[1]["body"]["text"].as_str().unwrap();
    assert!(
        cut.chars().count() < 500 && cut.ends_with('…'),
        "a tool output or long reply is shortened"
    );
    assert_eq!(body["dev.b@r"][0]["body"]["text"], "hello");
}

#[tokio::test]
async fn a_teams_communications_leave_out_tool_events_and_include_usage_only_when_asked() {
    let (st, router) = setup();
    let rig = rigspec::compile(GOOD).unwrap();
    st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            model::Desired::Running,
        )
        .unwrap();
    let seat = || Subject::Seat("dev.a@r".into());
    let usage = EventBody::UsageReported {
        turn_id: None,
        input_tokens: 1,
        output_tokens: 2,
        cache_read_tokens: 3,
        cache_write_tokens: 4,
        cost: None,
        in_turn: true,
    };
    for body in [
        EventBody::MessageQueued {
            message_id: "m1".into(),
            intent: model::MessageIntent::Inform,
            text: "hi".into(),
        },
        EventBody::ToolCalled {
            turn_id: None,
            call_id: "c1".into(),
            name: "Bash".into(),
            input: json!({ "command": "ls" }),
            parent_call_id: None,
        },
        EventBody::ToolResulted {
            turn_id: None,
            call_id: "c1".into(),
            output: "x".repeat(5000),
            is_error: false,
        },
        usage,
        EventBody::MessageDelivered {
            message_id: "m1".into(),
        },
    ] {
        st.local_event(
            Actor::System("test".into()),
            seat(),
            Provenance::AgentReported,
            body,
        )
        .unwrap();
    }
    let types = |v: &Value| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|e| e["body"]["type"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let (status, body) = call(&router, "GET", "/api/teams/r/communications", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        types(&body),
        ["message-delivered", "message-queued"],
        "the Run board's arrows need only messages"
    );
    let (_, body) = call(
        &router,
        "GET",
        "/api/teams/r/communications?usage=true",
        None,
    )
    .await;
    assert_eq!(
        types(&body),
        ["message-delivered", "usage-reported", "message-queued"]
    );
}

#[tokio::test]
async fn the_run_boards_communications_are_the_latest_runs_with_long_text_cut() {
    let (st, router) = setup();
    let rig = rigspec::compile(GOOD).unwrap();
    st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            model::Desired::Running,
        )
        .unwrap();
    let say = |id: &str, text: String| {
        st.local_event(
            Actor::System("test".into()),
            Subject::Seat("dev.a@r".into()),
            Provenance::AgentReported,
            EventBody::MessageQueued {
                message_id: id.into(),
                intent: model::MessageIntent::Inform,
                text,
            },
        )
        .unwrap();
    };
    say("old", "before this run".into());
    st.local_event(
        Actor::System("test".into()),
        Subject::Rig("r".into()),
        Provenance::Operator,
        EventBody::RigBooted {
            spec_hash: "h".into(),
            seat_count: 1,
        },
    )
    .unwrap();
    say("new", "b".repeat(20_000));
    let (status, body) = call(
        &router,
        "GET",
        "/api/teams/r/communications?run=latest&brief=true",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = body.as_array().unwrap();
    assert_eq!(list.len(), 1, "only the latest run");
    assert!(
        list[0]["body"]["text"].as_str().unwrap().chars().count() < 400,
        "the briefing is cut"
    );
    let (_, full) = call(&router, "GET", "/api/teams/r/communications", None).await;
    assert_eq!(
        full.as_array().unwrap().len(),
        2,
        "without the options, every run's messages in full"
    );
}
