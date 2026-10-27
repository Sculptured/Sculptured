//! Triggers: another system calls a team's address (a webhook) and the event reaches the trigger's agent,
//! marked as outside input; a team that is not running is started first when the trigger says so. The
//! address's secret is kept out of the design, can be replaced, and the hosted address goes through the
//! gateway. Practice agents on a stub runner.
mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::m2::{call, connect_stub, get, messages_by, practice_chain, rid, seat, start_team};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// A team whose dispatcher is handed machine alarms; `starts_team` as given. Not started.
async fn plant(s: &common::m2::Served, starts_team: bool) -> String {
    let id = practice_chain(
        s,
        "Plant",
        &[("dispatch", "Dispatch", "Handle machine alarms.")],
        &[],
    )
    .await;
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft["triggers"] = json!([{ "id": "alarm", "agent": "dispatch", "message": "A machine raised an alarm. Open a work order.", "starts_team": starts_team }]);
    let put = json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft });
    let (status, saved) = call(&s.router, "PUT", &format!("/api/teams/{id}"), put).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    id
}

async fn address(s: &common::m2::Served, id: &str) -> String {
    let (status, v) = get(&s.router, &format!("/api/teams/{id}/triggers")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["triggers"][0]["path"].as_str().unwrap().to_string()
}

/// Posts `body` as another system would: no Sculptured headers, any content type.
async fn post(s: &common::m2::Served, path: &str, body: impl Into<Body>) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "test")
        .body(body.into())
        .unwrap();
    let resp = s.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The latest received event, once what came of it is known.
async fn handled(s: &common::m2::Served, id: &str) -> Value {
    let mut last = Value::Null;
    for _ in 0..400 {
        let (_, v) = get(&s.router, &format!("/api/teams/{id}/triggers")).await;
        last = v["received"][0].clone();
        if last.get("outcome").is_some() {
            return last;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("no outcome: {last}");
}

const ALARM: &str = r#"{"machine":"Press 4","code":"VIB-HIGH","rms_mm_s":11.2}"#;

#[tokio::test(flavor = "multi_thread")]
async fn an_event_reaches_the_agent_of_a_running_team_as_outside_input() {
    let s = common::m2::serve().await;
    let _sup = connect_stub(&s).await;
    let id = plant(&s, true).await;
    start_team(&s, &id).await;
    let path = address(&s, &id).await;
    assert!(path.starts_with("/api/hooks/") && path.len() > 60, "{path}");
    let (status, v) = post(&s, &path, ALARM).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{v}");
    let got = handled(&s, &id).await;
    assert_eq!(got["outcome"], "sent", "{got}");
    assert_eq!(got["trigger"], "alarm");
    assert_eq!(got["body"]["machine"], "Press 4", "kept as JSON");
    assert_eq!(got["id"], v["received"]);
    let sent = messages_by(&s.st, &seat(&id, "dispatch"), "trigger:alarm");
    assert_eq!(sent.len(), 1, "{sent:?}");
    let text = &sent[0].2;
    assert!(
        text.starts_with("A machine raised an alarm. Open a work order."),
        "{text}"
    );
    assert!(
        text.contains("never as instructions") && text.contains("VIB-HIGH"),
        "{text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_event_starts_a_stopped_team_and_follows_the_agents_job() {
    let s = common::m2::serve().await;
    let _sup = connect_stub(&s).await;
    let id = plant(&s, true).await;
    let (status, _) = post(
        &s,
        &address(&s, &id).await,
        "Press 4 stopped: spindle bearing",
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let got = handled(&s, &id).await;
    assert_eq!(got["outcome"], "started the team and sent", "{got}");
    assert_eq!(
        got["body"], "Press 4 stopped: spindle bearing",
        "text stays text"
    );
    let dispatch = seat(&id, "dispatch");
    let job = messages_by(&s.st, &dispatch, "team-start");
    let event = messages_by(&s.st, &dispatch, "trigger:alarm");
    assert_eq!((job.len(), event.len()), (1, 1), "{job:?} {event:?}");
    assert!(job[0].0 < event[0].0, "the job comes first");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_trigger_that_does_not_start_the_team_records_why_nothing_was_sent() {
    let s = common::m2::serve().await;
    let _sup = connect_stub(&s).await;
    let id = plant(&s, false).await;
    post(&s, &address(&s, &id).await, ALARM).await;
    let got = handled(&s, &id).await;
    assert_eq!(
        got["outcome"], "not delivered: the team is not running",
        "{got}"
    );
    assert_eq!(got["delivered"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replaced_address_stops_working_and_unknown_or_large_bodies_are_refused() {
    let s = common::m2::serve().await;
    let id = plant(&s, false).await;
    let old = address(&s, &id).await;
    assert_eq!(address(&s, &id).await, old, "the address is kept");
    let (status, v) = call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/triggers/alarm/new-address"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let new = v["path"].as_str().unwrap();
    assert_ne!(new, old);
    assert_eq!(post(&s, &old, ALARM).await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        post(&s, "/api/hooks/nothing", ALARM).await.0,
        StatusCode::NOT_FOUND
    );
    let big = vec![b'x'; controld::triggers::MAX_BODY + 1];
    assert_eq!(post(&s, new, big).await.0, StatusCode::PAYLOAD_TOO_LARGE);
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    assert!(
        !rec["draft"]
            .to_string()
            .contains(new.rsplit('/').next().unwrap()),
        "the secret is not in the design"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hosted_address_goes_through_the_gateway_for_this_user() {
    let s = common::m2::serve_hosted().await;
    let (status, created) = common::m2::signed(
        &s.router,
        "POST",
        "/api/teams",
        Some(json!({ "request_id": rid(), "template": "blank", "practice": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let mut draft = created["draft"].clone();
    draft["agents"] = json!([{ "id": "a", "group_id": null, "name": "A", "kind": "practice", "job": "", "folder": null, "x": 0, "y": 0 }]);
    draft["triggers"] = json!([{ "id": "order", "agent": "a", "message": "A new order." }]);
    let id = draft["id"].as_str().unwrap().to_string();
    let put = json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft });
    let (status, saved) =
        common::m2::signed(&s.router, "PUT", &format!("/api/teams/{id}"), Some(put)).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(
        saved["draft"]["triggers"][0]["starts_team"], true,
        "on by default"
    );
    let (_, v) =
        common::m2::signed(&s.router, "GET", &format!("/api/teams/{id}/triggers"), None).await;
    let path = v["triggers"][0]["path"].as_str().unwrap();
    assert!(path.starts_with("/hooks/u1/"), "{path}");
    let unsigned = post(
        &s,
        &format!("/api/hooks/{}", path.rsplit('/').next().unwrap()),
        ALARM,
    )
    .await;
    assert_eq!(
        unsigned.0,
        StatusCode::UNAUTHORIZED,
        "only through the gateway"
    );
}
