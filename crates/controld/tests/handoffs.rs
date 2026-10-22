//! Real handoffs along "hands work to" connections, with Practice (stub) agents whose
//! turns complete straight away, and fake-claude where a real driver's turn shapes matter.
mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use controld::AppState;
use controld::handoffs::LOOP_ROUNDS;
use controld::reconciler::{HANDOFF_ACTOR, TEAM_START_ACTOR};
use controld::store::{HandoffWrite, RigSource, Store};
use drivers::Driver;
use drivers::claude::ClaudeStreamDriver;
use drivers::stub::StubDriver;
use hostd::journal::Journal;
use hostd::supervisor::Supervisor;
use http_body_util::BodyExt;
use model::{
    Actor, EventBody, EventRecord, HandoffSource, MessageState, Provenance, SeatLifecycle, Subject,
    TurnStatus,
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tower::ServiceExt;

const TOKEN: &str = "test-token";

async fn serve_with(
    home: &std::path::Path,
    store: Store,
) -> (AppState, Router, String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let st =
        AppState::new(store, TOKEN.into(), vec![addr.to_string(), "test".into()]).with_home(home);
    let served = controld::app(st.clone(), None);
    let server = tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    (
        st.clone(),
        controld::app(st, None),
        format!("ws://{addr}/host"),
        server,
    )
}

async fn serve(home: &std::path::Path) -> (AppState, Router, String) {
    let (st, router, url, _) = serve_with(home, Store::open_in_memory().unwrap()).await;
    (st, router, url)
}

fn supervisor(driver: Arc<dyn Driver>) -> Supervisor {
    Supervisor::new(
        "local".into(),
        vec![driver],
        Journal::open_in_memory(10_000).unwrap(),
    )
}

async fn link(st: &AppState, url: String, sup: Supervisor) -> tokio::task::JoinHandle<()> {
    let driver = sup
        .driver_infos()
        .first()
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "stub".into());
    let task = tokio::spawn(async move {
        let _ = hostd::link::run_link(&url, TOKEN, &sup).await;
    });
    crate::common::wait_for("host connected", || st.hosts.has_driver("local", &driver)).await;
    task
}

/// A served controld whose local host runs `driver`, in a fresh home folder.
async fn served(driver: Arc<dyn Driver>) -> (tempfile::TempDir, AppState, Router) {
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    link(&st, url, supervisor(driver)).await;
    (home, st, router)
}

async fn call(router: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "test")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A team from `(id, name, job)` agents of `kind` in `folder`, and `(from, to, kind)` connections.
async fn team_of(
    router: &Router,
    name: &str,
    kind: &str,
    folder: Option<&std::path::Path>,
    agents: &[(&str, &str, &str)],
    links: &[(&str, &str, &str)],
) -> String {
    let (status, rec) =
        call(router, "POST", "/api/teams", json!({ "request_id": format!("create-{name}"), "template": "blank", "practice": true, "name": name })).await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = agents
        .iter()
        .enumerate()
        .map(|(i, (id, name, job))| {
            json!({ "id": id, "group_id": null, "name": name, "kind": kind, "job": job,
                    "folder": folder.map(|f| f.display().to_string()), "x": 40 + 260 * i as i32, "y": 40 })
        })
        .collect();
    draft["connections"] =
        links.iter().map(|(from, to, kind)| json!({ "id": format!("c-{from}-{to}"), "from": from, "to": to, "kind": kind })).collect();
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = call(
        router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": format!("put-{name}"), "revision": 0, "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

async fn team(
    router: &Router,
    name: &str,
    agents: &[(&str, &str, &str)],
    links: &[(&str, &str, &str)],
) -> String {
    team_of(router, name, "practice", None, agents, links).await
}

async fn start(router: &Router, id: &str, request: &str) {
    let (status, body) = call(
        router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": request }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn stop(router: &Router, id: &str, request: &str) {
    let (status, body) = call(
        router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": request }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

async fn send(router: &Router, seat: &str, request: &str, text: &str) {
    let (status, body) = call(
        router,
        "POST",
        &format!("/api/seats/{seat}/send"),
        json!({ "request_id": request, "text": text, "intent": "instruct" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// "Hand over again" on `seat`.
async fn hand_over(router: &Router, seat: &str, request: &str) -> (StatusCode, Value) {
    call(
        router,
        "POST",
        &format!("/api/seats/{seat}/handover"),
        json!({ "request_id": request }),
    )
    .await
}

fn seat(id: &str, agent: &str) -> String {
    format!("team.{agent}@{id}")
}

fn events(st: &AppState, seat: &str) -> Vec<EventRecord> {
    st.store
        .lock()
        .unwrap()
        .events_for_seat(seat, 0, 5000)
        .unwrap()
}

/// `(seq, message id, text)` of each message queued by `actor`.
fn messages_by(st: &AppState, seat: &str, actor: &str) -> Vec<(i64, String, String)> {
    events(st, seat)
        .into_iter()
        .filter(|e| e.actor == Actor::System(actor.into()))
        .filter_map(|e| match e.body {
            EventBody::MessageQueued {
                message_id, text, ..
            } => Some((e.seq, message_id, common::work_message(&text))),
            _ => None,
        })
        .collect()
}

fn handoffs(st: &AppState, seat: &str) -> Vec<(i64, String, String)> {
    messages_by(st, seat, HANDOFF_ACTOR)
}

fn state(st: &AppState, message: &str) -> Option<MessageState> {
    st.store.lock().unwrap().message_state(message).unwrap()
}

fn consumed(st: &AppState, message: &str) -> bool {
    state(st, message) == Some(MessageState::Consumed)
}

fn failed_reason(st: &AppState, seat: &str, message: &str) -> Option<String> {
    events(st, seat).into_iter().find_map(|e| match e.body {
        EventBody::MessageFailed { message_id, reason } if message_id == message => Some(reason),
        _ => None,
    })
}

fn waiting_for(st: &AppState, seat: &str) -> Option<Vec<String>> {
    st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .and_then(|s| s.waiting_for)
}

fn stuck(st: &AppState, seat: &str) -> Option<(String, bool)> {
    st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .and_then(|s| s.handoff_stuck)
        .map(|s| (s.reason, s.can_resend))
}

fn handed_over_count(st: &AppState, seat: &str) -> usize {
    events(st, seat)
        .iter()
        .filter(|e| matches!(e.body, EventBody::HandedOver { .. }))
        .count()
}

fn completed_turns(st: &AppState, seat: &str) -> usize {
    events(st, seat)
        .iter()
        .filter(|e| {
            matches!(
                e.body,
                EventBody::TurnEnded {
                    status: TurnStatus::Completed,
                    ..
                }
            )
        })
        .count()
}

fn turns_ended(st: &AppState, seats: &[String]) -> usize {
    seats
        .iter()
        .map(|s| {
            events(st, s)
                .iter()
                .filter(|e| matches!(e.body, EventBody::TurnEnded { .. }))
                .count()
        })
        .sum()
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

#[test]
fn the_handoff_actor_serializes_as_the_web_expects() {
    assert_eq!(
        serde_json::to_value(Actor::System(HANDOFF_ACTOR.into())).unwrap(),
        json!({ "kind": "system", "id": "handoff" })
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_chain_runs_in_order_and_only_work_turns_hand_over() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Chain",
        &[
            ("planner", "Planner", "Plan it"),
            ("builder", "Builder", "Build it"),
            ("reviewer", "Reviewer", "Review it"),
        ],
        &[
            ("planner", "builder", "hands-work-to"),
            ("builder", "reviewer", "hands-work-to"),
        ],
    )
    .await;
    let (planner, builder, reviewer) = (
        seat(&id, "planner"),
        seat(&id, "builder"),
        seat(&id, "reviewer"),
    );
    start(&router, &id, "s1").await;
    crate::common::wait_for("the reviewer's handoff read", || {
        handoffs(&st, &reviewer)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;

    let jobs = messages_by(&st, &planner, TEAM_START_ACTOR);
    assert_eq!(jobs.len(), 1);
    assert!(
        messages_by(&st, &builder, TEAM_START_ACTOR).is_empty(),
        "the builder gets its Job with its handoff"
    );
    assert!(
        messages_by(&st, &reviewer, TEAM_START_ACTOR).is_empty(),
        "the reviewer gets its Job with its handoff"
    );
    let b = handoffs(&st, &builder);
    let r = handoffs(&st, &reviewer);
    assert_eq!((b.len(), r.len()), (1, 1), "{b:?} {r:?}");
    assert_eq!(
        b[0].2,
        "Handed over by Planner\n\nBuild it\n\n--- Planner's result ---\nstub received: Plan it"
    );
    assert!(r[0].2.starts_with("Handed over by Builder\n\nReview it\n\n--- Builder's result ---\nstub received: Handed over by Planner"), "{}", r[0].2);
    assert!(
        jobs[0].0 < b[0].0 && b[0].0 < r[0].0,
        "planner, then builder, then reviewer"
    );
    for (s, up) in [(&builder, &planner), (&reviewer, &builder)] {
        assert!(
            events(&st, s).iter().any(|e| matches!(&e.body, EventBody::HandoffWaiting { waiting_for, .. } if waiting_for == &vec![up.clone()])),
            "{s} waited for {up}"
        );
        assert_eq!(waiting_for(&st, s), None);
    }
    let handed = events(&st, &builder)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::HandedOver {
                from,
                message_id,
                start_seq,
                ..
            } => Some((from, message_id, start_seq)),
            _ => None,
        });
    let (from, message_id, start_seq) = handed.expect("a handed-over record");
    assert_eq!(message_id, b[0].1);
    assert!(start_seq > 0, "the record names its start");
    assert_eq!(
        from.iter().map(|f| f.seat.as_str()).collect::<Vec<_>>(),
        [planner.as_str()]
    );

    // A turn answering the operator is not work: it hands nothing over.
    send(&router, &planner, "chat-1", "How is it going?").await;
    send(&router, &builder, "chat-2", "Any questions?").await;
    crate::common::wait_for("both chat turns ended", || {
        completed_turns(&st, &planner) == 2 && completed_turns(&st, &builder) == 2
    })
    .await;
    settle().await;
    assert_eq!(
        (
            handoffs(&st, &builder).len(),
            handoffs(&st, &reviewer).len()
        ),
        (1, 1),
        "chat never hands over"
    );

    // "Hand over again" on the planner: one "More work" to the builder, whose work turn
    // on it then passes "More work" on to the reviewer by itself.
    let (status, body) = hand_over(&router, &planner, "h1").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        hand_over(&router, &planner, "h1").await.0,
        StatusCode::OK,
        "the same press replays"
    );
    crate::common::wait_for("more work reaches the reviewer", || {
        handoffs(&st, &reviewer).len() == 2
    })
    .await;
    let b = handoffs(&st, &builder);
    assert_eq!(b.len(), 2);
    assert_eq!(
        b[1].2,
        "More work from Planner\n\n--- Planner's result ---\nstub received: How is it going?"
    );
    assert!(handoffs(&st, &reviewer)[1].2.starts_with(
        "More work from Builder\n\n--- Builder's result ---\nstub received: More work from Planner"
    ));
    settle().await;
    assert_eq!(
        (
            handoffs(&st, &builder).len(),
            handoffs(&st, &reviewer).len()
        ),
        (2, 2),
        "one press, one message per downstream"
    );

    let (status, body) = hand_over(&router, &reviewer, "h2").await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("no_downstream")),
        "{body}"
    );
}

/// The project's Input box: the agents it points to get it with their Job, a starting agent at
/// start and a downstream agent with its first handoff.
#[tokio::test]
async fn the_input_reaches_the_agents_it_points_to() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Input",
        &[
            ("planner", "Planner", "Plan it"),
            ("builder", "Builder", "Build it"),
        ],
        &[("planner", "builder", "hands-work-to")],
    )
    .await;
    let (_, rec) = call(&router, "GET", &format!("/api/teams/{id}"), json!(null)).await;
    let mut draft = rec["draft"].clone();
    draft["input"] =
        json!({ "text": "Make the landing page", "x": 0, "y": 0, "to": ["planner", "builder"] });
    let (status, body) = call(
        &router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": "put-input", "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    start(&router, &id, "s1").await;
    let builder = seat(&id, "builder");
    crate::common::wait_for("the builder's handoff", || {
        !handoffs(&st, &builder).is_empty()
    })
    .await;
    let jobs = messages_by(&st, &seat(&id, "planner"), TEAM_START_ACTOR);
    assert_eq!(jobs[0].2, "Plan it\n\n--- Input ---\nMake the landing page");
    assert!(
        handoffs(&st, &builder)[0].2.starts_with(
            "Handed over by Planner\n\nBuild it\n\n--- Input ---\nMake the landing page"
        ),
        "{}",
        handoffs(&st, &builder)[0].2
    );
}

/// Builder → Reviewer ⇄ CAD: the loop has a starting point (the Builder), so the Reviewer's
/// first handoff waits only for the Builder. Work then goes round the loop, and stops after
/// LOOP_ROUNDS rounds of the same work, saying so; Hand over again goes round again.
#[tokio::test]
async fn a_loop_with_a_starting_point_goes_round_until_its_round_limit() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Loop",
        &[
            ("builder", "Builder", "Build it"),
            ("reviewer", "Reviewer", "Review it"),
            ("cad", "CAD", "Integrate it"),
        ],
        &[
            ("builder", "reviewer", "hands-work-to"),
            ("reviewer", "cad", "hands-work-to"),
            ("cad", "reviewer", "hands-work-to"),
        ],
    )
    .await;
    let (reviewer, cad) = (seat(&id, "reviewer"), seat(&id, "cad"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("the loop stops at its limit", || {
        stuck(&st, &reviewer).is_some_and(|(r, _)| r.contains("rounds"))
    })
    .await;
    settle().await;
    let (r, c) = (handoffs(&st, &reviewer), handoffs(&st, &cad));
    assert!(
        r[0].2.starts_with("Handed over by Builder\n\nReview it"),
        "{}",
        r[0].2
    );
    assert!(r[1].2.starts_with("More work from CAD"), "{}", r[1].2);
    assert_eq!(
        (r.len(), c.len()),
        (1 + LOOP_ROUNDS, 1 + LOOP_ROUNDS),
        "{r:?}"
    );
    let (reason, _) = stuck(&st, &reviewer).unwrap();
    assert!(
        reason.contains(&format!("{LOOP_ROUNDS} rounds")) && reason.contains("Hand over again"),
        "{reason}"
    );

    let (status, body) = hand_over(&router, &cad, "again").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("another round", || {
        handoffs(&st, &reviewer).len() > 1 + LOOP_ROUNDS
    })
    .await;
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn fan_in_waits_for_every_upstream_and_a_jobless_heads_first_message_is_its_work() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Fan in",
        &[
            ("planner", "Planner", "Plan"),
            ("designer", "Designer", ""),
            ("builder", "Builder", "Build"),
        ],
        &[
            ("planner", "builder", "hands-work-to"),
            ("designer", "builder", "hands-work-to"),
            ("planner", "designer", "watches"),
        ],
    )
    .await;
    let (designer, builder) = (seat(&id, "designer"), seat(&id, "builder"));
    start(&router, &id, "s1").await;
    // The designer has no Job: the builder waits for it (waiting, not stuck).
    crate::common::wait_for("builder waits for the designer", || {
        crate::common::lifecycle(&st, &builder) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &builder) == Some(vec![designer.clone()])
    })
    .await;
    settle().await;
    assert_eq!(stuck(&st, &builder), None);
    assert!(
        messages_by(&st, &designer, TEAM_START_ACTOR).is_empty(),
        "an empty Job sends nothing"
    );
    let (status, body) = hand_over(&router, &designer, "early").await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("nothing_to_hand_over")),
        "{body}"
    );

    // Its first message is its work: when it finishes, the fan-in completes.
    send(&router, &designer, "d1", "Draw it").await;
    crate::common::wait_for("builder's first handoff", || {
        !handoffs(&st, &builder).is_empty()
    })
    .await;
    assert!(
        handoffs(&st, &designer).is_empty(),
        "watches does not hand over"
    );
    assert_eq!(
        handoffs(&st, &builder)[0].2,
        "Handed over by Planner, Designer\n\nBuild\n\n--- Planner's result ---\nstub received: Plan\n\n--- Designer's result ---\nstub received: Draw it"
    );
    crate::common::wait_for("no longer waiting or stuck", || {
        waiting_for(&st, &builder).is_none() && stuck(&st, &builder).is_none()
    })
    .await;
    // Later messages to it are conversation: nothing more is handed over.
    send(&router, &designer, "d2", "Thanks").await;
    crate::common::wait_for("the designer answered again", || {
        completed_turns(&st, &designer) == 2
    })
    .await;
    settle().await;
    assert_eq!(handoffs(&st, &builder).len(), 1);
}

#[tokio::test]
async fn stop_ends_waiting_and_a_new_start_does_not_reuse_old_results() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Stops",
        &[("a", "Asker", ""), ("b", "Checker", "Check")],
        &[("a", "b", "hands-work-to")],
    )
    .await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));

    start(&router, &id, "s1").await;
    crate::common::wait_for("b waits for a", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &b) == Some(vec![a.clone()])
    })
    .await;
    send(&router, &a, "a1", "one").await;
    crate::common::wait_for("first handoff", || handoffs(&st, &b).len() == 1).await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    stop(&router, &id, "x1").await;
    crate::common::wait_for("stopped", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let (status, body) = hand_over(&router, &a, "h-stopped").await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("team_not_running")),
        "{body}"
    );

    // A new start: a's turn and the request from the first start do not count.
    start(&router, &id, "s2").await;
    crate::common::wait_for("b waits for a again", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &b) == Some(vec![a.clone()])
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    settle().await;
    assert_eq!(
        handoffs(&st, &b).len(),
        1,
        "nothing handed over from the earlier start"
    );

    // A stop while b waits ends the wait (and the stuck state), with nothing handed over.
    stop(&router, &id, "x2").await;
    crate::common::wait_for("b no longer waits", || {
        waiting_for(&st, &b).is_none() && stuck(&st, &b).is_none()
    })
    .await;
    crate::common::wait_for("stopped", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Stopped)
    })
    .await;
    assert_eq!(handoffs(&st, &b).len(), 1);

    start(&router, &id, "s3").await;
    crate::common::wait_for("b waits for a", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &b) == Some(vec![a.clone()])
    })
    .await;
    send(&router, &a, "a2", "two").await;
    crate::common::wait_for("second first-handoff", || handoffs(&st, &b).len() == 2).await;
    assert_eq!(
        handoffs(&st, &b)[1].2,
        "Handed over by Asker\n\nCheck\n\n--- Asker's result ---\nstub received: two"
    );
    settle().await;
    assert_eq!(handed_over_count(&st, &b), 2);
}

/// Stub driver whose `slow` seat takes a while to launch and whose `broken` seat fails
/// its first `fail_times` launches.
struct TestStub {
    slow: Option<&'static str>,
    broken: Option<&'static str>,
    fail_times: u32,
    failed: AtomicU32,
}

impl TestStub {
    fn slow(seat: &'static str) -> Self {
        TestStub {
            slow: Some(seat),
            broken: None,
            fail_times: 0,
            failed: AtomicU32::new(0),
        }
    }
    fn broken(seat: &'static str, times: u32) -> Self {
        TestStub {
            slow: None,
            broken: Some(seat),
            fail_times: times,
            failed: AtomicU32::new(0),
        }
    }
}

#[async_trait::async_trait]
impl Driver for TestStub {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> model::Caps {
        StubDriver.capabilities()
    }
    async fn launch(
        &self,
        spec: drivers::LaunchSpec,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        let s = spec.seat.to_string();
        if self.slow.is_some_and(|p| s.starts_with(p)) {
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
        if self.broken.is_some_and(|p| s.starts_with(p))
            && self.failed.fetch_add(1, Ordering::SeqCst) < self.fail_times
        {
            return Err(drivers::DriverError::Cwd("/nowhere".into()));
        }
        StubDriver.launch(spec).await
    }
    async fn resume(
        &self,
        spec: drivers::LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        StubDriver.resume(spec, native_id).await
    }
}

/// Stub driver whose `seat` hangs, on its first launch only, on any message containing
/// `HANG-ONCE` (the stub hangs on "hang"); later launches run every message normally.
struct HangOnce {
    seat: &'static str,
    launches: AtomicU32,
}

struct HangOnceSession {
    inner: Box<dyn drivers::Session>,
    hang: bool,
}

#[async_trait::async_trait]
impl drivers::Session for HangOnceSession {
    async fn send(
        &mut self,
        mut msg: drivers::OutboundMessage,
    ) -> Result<(), drivers::DriverError> {
        if self.hang && msg.text.contains("HANG-ONCE") {
            msg.text = "hang".into();
        }
        self.inner.send(msg).await
    }
    async fn interrupt(&mut self) -> Result<(), drivers::DriverError> {
        self.inner.interrupt().await
    }
    async fn stop(&mut self, grace: Duration) -> Result<(), drivers::DriverError> {
        self.inner.stop(grace).await
    }
    fn take_events(&mut self) -> Option<futures::stream::BoxStream<'static, drivers::DriverEvent>> {
        self.inner.take_events()
    }
}

#[async_trait::async_trait]
impl Driver for HangOnce {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> model::Caps {
        StubDriver.capabilities()
    }
    async fn launch(
        &self,
        spec: drivers::LaunchSpec,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        let hang = spec.seat.to_string().starts_with(self.seat)
            && self.launches.fetch_add(1, Ordering::SeqCst) == 0;
        Ok(Box::new(HangOnceSession {
            inner: StubDriver.launch(spec).await?,
            hang,
        }))
    }
    async fn resume(
        &self,
        spec: drivers::LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        StubDriver.resume(spec, native_id).await
    }
}

/// An agent mid-work on its first handoff has a wave message delivered but not read yet
/// when it stops. Restart re-sends the unfinished handoff (asking it to continue) and the
/// wave message it never read, so its turn for the wave still happens.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn restart_resends_the_unfinished_handoff_and_the_wave_message_it_never_read() {
    let (_home, st, router) = served(Arc::new(HangOnce {
        seat: "team.b@",
        launches: AtomicU32::new(0),
    }))
    .await;
    let id = team(
        &router,
        "Unread",
        &[
            ("a", "Source", "Start"),
            ("b", "Left", "HANG-ONCE"),
            ("c", "Right", "R"),
            ("d", "Join", "J"),
        ],
        &[
            ("a", "b", "hands-work-to"),
            ("a", "c", "hands-work-to"),
            ("b", "d", "hands-work-to"),
            ("c", "d", "hands-work-to"),
        ],
    )
    .await;
    let (a, b, c) = (seat(&id, "a"), seat(&id, "b"), seat(&id, "c"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("left is mid-work on its first handoff", || {
        handoffs(&st, &b)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("right finished its first handoff", || {
        completed_turns(&st, &c) == 1
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    assert_eq!(completed_turns(&st, &b), 0, "left hangs");

    // A wave from the source: left's wave message is delivered (left is busy) but not read.
    assert_eq!(hand_over(&router, &a, "h1").await.0, StatusCode::OK);
    crate::common::wait_for("left's wave message delivered", || {
        handoffs(&st, &b)
            .get(1)
            .is_some_and(|(_, m, _)| state(&st, m) == Some(MessageState::Delivered))
    })
    .await;
    crate::common::wait_for("right ran the wave", || completed_turns(&st, &c) == 2).await;
    let (first, wave) = (
        handoffs(&st, &b)[0].2.clone(),
        handoffs(&st, &b)[1].2.clone(),
    );
    assert!(wave.starts_with("More work from Source"), "{wave}");

    // Left stops before reading it, and is restarted.
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/stop"),
        json!({ "request_id": "x-b" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("left stopped", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/restart"),
        json!({ "request_id": "rs-b" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    crate::common::wait_for("left read the re-sent wave message", || {
        handoffs(&st, &b)
            .iter()
            .skip(2)
            .any(|(_, m, t)| *t == wave && consumed(&st, m))
    })
    .await;
    let copies = handoffs(&st, &b);
    assert_eq!(
        copies.len(),
        4,
        "the two originals, the continuation and the wave message again: {copies:?}"
    );
    assert_eq!(
        copies[2].2,
        format!("{}\n\n{first}", controld::handoffs::CONTINUE_PREFIX),
        "the unfinished work comes first"
    );
    assert_eq!(copies[3].2, wave);
    crate::common::wait_for("left ran both", || completed_turns(&st, &b) == 2).await;
    crate::common::wait_for("nobody is stuck or waiting", || {
        [&a, &b, &c, &seat(&id, "d")]
            .iter()
            .all(|s| stuck(&st, s).is_none() && waiting_for(&st, s).is_none())
    })
    .await;
}

/// The agent runner (hostd) dies while a first agent is mid-Job. No TenureEnded is ever
/// recorded for that process: the host comes back without it and the agent needs
/// attention. Restart then sends the Job again with the continue prefix (the new process
/// is a new tenure, so the work the old one read is unfinished), and the chain completes.
#[tokio::test]
async fn work_lost_with_the_agent_runner_is_continued_by_restart() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    let first = link(&st, url.clone(), supervisor(Arc::new(StubDriver))).await;
    let id = team(
        &router,
        "Lost host",
        &[("lead", "Lead", "hang"), ("next", "Next", "Go on")],
        &[("lead", "next", "hands-work-to")],
    )
    .await;
    let (lead, next) = (seat(&id, "lead"), seat(&id, "next"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("lead is mid-Job", || {
        messages_by(&st, &lead, TEAM_START_ACTOR)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("the boot finished", || {
        !st.is_booting(&id) && crate::common::lifecycle(&st, &next) == Some(SeatLifecycle::Running)
    })
    .await;

    // The runner dies with its agents; a fresh one comes back with nothing running.
    first.abort();
    crate::common::wait_for("agents stale", || {
        crate::common::lifecycle(&st, &lead) == Some(SeatLifecycle::Stale)
    })
    .await;
    let _second = tokio::spawn({
        let sup = supervisor(Arc::new(StubDriver));
        async move {
            let _ = hostd::link::run_link(&url, TOKEN, &sup).await;
        }
    });
    crate::common::wait_for("both need attention", || {
        [&lead, &next]
            .iter()
            .all(|s| crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Attention))
    })
    .await;
    assert!(
        !events(&st, &lead)
            .iter()
            .any(|e| matches!(e.body, EventBody::TenureEnded { .. })),
        "no TenureEnded for the lost process"
    );
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{next}/restart"),
        json!({ "request_id": "rs-next" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("next runs again", || {
        crate::common::lifecycle(&st, &next) == Some(SeatLifecycle::Running)
    })
    .await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{lead}/restart"),
        json!({ "request_id": "rs-lead" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the chain completes", || {
        handoffs(&st, &next)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    let jobs = messages_by(&st, &lead, TEAM_START_ACTOR);
    assert_eq!(jobs.len(), 2, "the Job and its continuation: {jobs:?}");
    assert_eq!(
        jobs[1].2,
        format!("{}\n\nhang", controld::handoffs::CONTINUE_PREFIX)
    );
    crate::common::wait_for("nobody is stuck or waiting", || {
        [&lead, &next]
            .iter()
            .all(|s| stuck(&st, s).is_none() && waiting_for(&st, s).is_none())
    })
    .await;
}

/// A wave the join waits for, on an agent that read both wave messages in one turn (so
/// only the later wave counts as its work) and is then stopped: it has nothing left to
/// do and owes nothing. Restarting it sends nothing, and the join then says so: "stuck:
/// waiting on Left, who has nothing left to do", not a plain wait for ever.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_restart_that_owes_nothing_still_shows_the_stall() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "No-op",
        &[
            ("a", "Source", "Start"),
            ("b", "Left", "L"),
            ("c", "Right", "R"),
            ("d", "Join", "J"),
        ],
        &[
            ("a", "b", "hands-work-to"),
            ("a", "c", "hands-work-to"),
            ("b", "d", "hands-work-to"),
            ("c", "d", "hands-work-to"),
        ],
    )
    .await;
    let (a, b, c, d) = (
        seat(&id, "a"),
        seat(&id, "b"),
        seat(&id, "c"),
        seat(&id, "d"),
    );
    start(&router, &id, "s1").await;
    crate::common::wait_for("the join read its first handoff", || {
        handoffs(&st, &d)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    settle().await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/stop"),
        json!({ "request_id": "x-b" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("left stopped", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Stopped)
    })
    .await;

    // Two waves from the source, written to the store (the dispatcher does not see them):
    // Right runs each; Left reads both in one turn.
    let (a_turn, a_seq) = events(&st, &a)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::TurnEnded {
                turn_id,
                status: TurnStatus::Completed,
            } => Some((turn_id, e.seq)),
            _ => None,
        })
        .unwrap();
    let start_seq = st
        .store
        .lock()
        .unwrap()
        .latest_of_types(&Subject::Rig(id.clone()), &["rig-booted"], 1)
        .unwrap()[0]
        .seq;
    let mut s = Script {
        st: &st,
        host_seq: 0,
    };
    let other = |s: &mut Script, seat: &str, body: EventBody| {
        s.host_seq += 1;
        let ev = host_proto::HostEvent {
            host_seq: s.host_seq,
            at_ms: model::now_ms(),
            actor: Actor::System("hostd".into()),
            subject: Subject::Seat(seat.to_string()),
            provenance: Provenance::ProtocolObserved,
            body,
        };
        st.store.lock().unwrap().ingest("other", ev).unwrap();
    };
    for (w, tag) in [(1, "x1"), (2, "x2")] {
        let req = s.local(
            Actor::Human("local".into()),
            Subject::Seat(a.clone()),
            EventBody::HandoverRequested {
                turn_id: a_turn.clone(),
                turn_seq: a_seq,
            },
        );
        let wave = model::Wave {
            id: req,
            origin: a.clone(),
        };
        let from = vec![source(&a, &a_turn, a_seq, Some(req))];
        s.hand(
            &c,
            start_seq,
            false,
            &format!("c-w{w}"),
            from.clone(),
            Some(wave.clone()),
        );
        s.hand(&b, start_seq, false, &format!("b-w{w}"), from, Some(wave));
        other(
            &mut s,
            &c,
            EventBody::TurnStarted {
                turn_id: tag.into(),
            },
        );
        other(
            &mut s,
            &c,
            EventBody::MessageConsumed {
                message_id: format!("c-w{w}"),
                turn_id: tag.into(),
            },
        );
        other(
            &mut s,
            &c,
            EventBody::TurnEnded {
                turn_id: tag.into(),
                status: TurnStatus::Completed,
            },
        );
    }
    other(
        &mut s,
        &b,
        EventBody::TurnStarted {
            turn_id: "y1".into(),
        },
    );
    other(
        &mut s,
        &b,
        EventBody::MessageConsumed {
            message_id: "b-w1".into(),
            turn_id: "y1".into(),
        },
    );
    other(
        &mut s,
        &b,
        EventBody::MessageConsumed {
            message_id: "b-w2".into(),
            turn_id: "y1".into(),
        },
    );
    other(
        &mut s,
        &b,
        EventBody::TurnEnded {
            turn_id: "y1".into(),
            status: TurnStatus::Completed,
        },
    );
    controld::handoffs::process_rig(&st, &id, false).await;
    crate::common::wait_for("the join read the second wave", || {
        handoffs(&st, &d)
            .get(1)
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("the join waits on the stopped agent", || {
        reason(&st, &d) == "Left stopped"
    })
    .await;
    settle().await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/restart"),
        json!({ "request_id": "rs-b" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the join says why it waits", || {
        reason(&st, &d) == "waiting on Left, who has nothing left to do"
    })
    .await;
    assert_eq!(
        handoffs(&st, &b).len(),
        3,
        "nothing was owed, so nothing was re-sent"
    );
}

/// Stub driver whose session fails to take the first message containing `FAIL-ONCE`.
struct FailOnce {
    failed: Arc<AtomicU32>,
}

struct FailOnceSession {
    inner: Box<dyn drivers::Session>,
    failed: Arc<AtomicU32>,
}

#[async_trait::async_trait]
impl drivers::Session for FailOnceSession {
    async fn send(&mut self, msg: drivers::OutboundMessage) -> Result<(), drivers::DriverError> {
        if msg.text.contains("FAIL-ONCE") && self.failed.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(drivers::DriverError::Closed);
        }
        self.inner.send(msg).await
    }
    async fn interrupt(&mut self) -> Result<(), drivers::DriverError> {
        self.inner.interrupt().await
    }
    async fn stop(&mut self, grace: Duration) -> Result<(), drivers::DriverError> {
        self.inner.stop(grace).await
    }
    fn take_events(&mut self) -> Option<futures::stream::BoxStream<'static, drivers::DriverEvent>> {
        self.inner.take_events()
    }
}

#[async_trait::async_trait]
impl Driver for FailOnce {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> model::Caps {
        StubDriver.capabilities()
    }
    async fn launch(
        &self,
        spec: drivers::LaunchSpec,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        Ok(Box::new(FailOnceSession {
            inner: StubDriver.launch(spec).await?,
            failed: self.failed.clone(),
        }))
    }
    async fn resume(
        &self,
        spec: drivers::LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        StubDriver.resume(spec, native_id).await
    }
}

/// A first agent's Job could not be sent although the agent runs: it says so ("stuck:
/// Lead's Job was not sent", with Resend), the agent below says it waits on a stuck agent,
/// and Resend sends the Job again once; the chain then completes.
#[tokio::test]
async fn a_first_agents_job_that_was_not_sent_is_stuck_until_resent() {
    let (_home, st, router) = served(Arc::new(FailOnce {
        failed: Arc::default(),
    }))
    .await;
    let id = team(
        &router,
        "Unsent",
        &[
            ("lead", "Lead", "Plan FAIL-ONCE"),
            ("next", "Next", "Go on"),
        ],
        &[("lead", "next", "hands-work-to")],
    )
    .await;
    let (lead, next) = (seat(&id, "lead"), seat(&id, "next"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("the Job failed", || {
        messages_by(&st, &lead, TEAM_START_ACTOR)
            .first()
            .is_some_and(|(_, m, _)| state(&st, m) == Some(MessageState::Failed))
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    crate::common::wait_for("lead is stuck on its Job", || {
        stuck(&st, &lead) == Some(("Lead's Job was not sent".into(), true))
    })
    .await;
    crate::common::wait_for("next waits on a stuck agent", || {
        reason(&st, &next) == "waiting on Lead (stuck)"
    })
    .await;
    assert_eq!(
        crate::common::lifecycle(&st, &lead),
        Some(SeatLifecycle::Running)
    );

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{lead}/resend"),
        json!({ "request_id": "r1" }),
    )
    .await;
    assert_eq!(
        (status, body["resent"].clone()),
        (StatusCode::OK, json!(1)),
        "{body}"
    );
    crate::common::wait_for("the chain completes", || {
        handoffs(&st, &next)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    let jobs = messages_by(&st, &lead, TEAM_START_ACTOR);
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    assert_eq!(jobs[1].2, "Plan FAIL-ONCE");
    crate::common::wait_for("nobody is stuck", || {
        stuck(&st, &lead).is_none() && stuck(&st, &next).is_none()
    })
    .await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{lead}/resend"),
        json!({ "request_id": "r2" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("nothing_to_resend")),
        "{body}"
    );
}

/// Every stuck reason ever recorded for `seat`.
fn stuck_reasons(st: &AppState, seat: &str) -> Vec<String> {
    events(st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::HandoffWaiting { stuck: Some(s), .. } => Some(s.reason),
            _ => None,
        })
        .collect()
}

/// An agent restarted while it owes a wave message (delivered to its old process, never
/// read) is never shown as having nothing left to do in the moment between running again
/// and that message being sent again: the join below it waits, then runs once.
#[tokio::test]
async fn a_restarted_agent_with_owed_work_is_never_taken_for_one_with_nothing_left() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Owed",
        &[
            ("a", "Source", "Start"),
            ("b", "Left", "L"),
            ("c", "Right", "R"),
            ("d", "Join", "J"),
        ],
        &[
            ("a", "b", "hands-work-to"),
            ("a", "c", "hands-work-to"),
            ("b", "d", "hands-work-to"),
            ("c", "d", "hands-work-to"),
        ],
    )
    .await;
    let (a, b, c, d) = (
        seat(&id, "a"),
        seat(&id, "b"),
        seat(&id, "c"),
        seat(&id, "d"),
    );
    start(&router, &id, "s1").await;
    crate::common::wait_for("the join read its first handoff", || {
        handoffs(&st, &d)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    // Left is busy answering you (the stub hangs on "hang") when a wave reaches it.
    send(&router, &b, "chat", "hang").await;
    assert_eq!(hand_over(&router, &a, "h1").await.0, StatusCode::OK);
    crate::common::wait_for("left's wave message delivered", || {
        handoffs(&st, &b)
            .get(1)
            .is_some_and(|(_, m, _)| state(&st, m) == Some(MessageState::Delivered))
    })
    .await;
    crate::common::wait_for("right ran the wave", || completed_turns(&st, &c) == 2).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/stop"),
        json!({ "request_id": "x-b" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("left stopped", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/restart"),
        json!({ "request_id": "rs-b" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the join read the wave", || {
        handoffs(&st, &d)
            .get(1)
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    settle().await;
    let reasons = stuck_reasons(&st, &d);
    assert!(
        !reasons.iter().any(|r| r.contains("nothing left to do")),
        "{reasons:?}"
    );
    assert_eq!(handoffs(&st, &d).len(), 2, "the join ran once for the wave");
}

/// A handoff due while the reviewer is still starting waits for it; a stop then ends
/// that wait and records the handoff as not sent.
#[tokio::test]
async fn a_handoff_waits_for_a_starting_agent_and_a_stop_records_it_as_not_sent() {
    let (_home, st, router) = served(Arc::new(TestStub::slow("team.reviewer@"))).await;
    let id = team(
        &router,
        "Slow",
        &[
            ("builder", "Builder", "Build"),
            ("reviewer", "Reviewer", "Review"),
        ],
        &[("builder", "reviewer", "hands-work-to")],
    )
    .await;
    let (builder, reviewer) = (seat(&id, "builder"), seat(&id, "reviewer"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("builder finished", || completed_turns(&st, &builder) == 1).await;
    assert!(
        handoffs(&st, &reviewer).is_empty(),
        "the reviewer is still starting"
    );
    assert_eq!(
        waiting_for(&st, &reviewer),
        None,
        "it waits to start, not for the builder"
    );
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/rigs/{id}/stop"),
        json!({ "request_id": "x1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the handoff recorded as not sent", || {
        handoffs(&st, &reviewer).len() == 1
    })
    .await;
    let (_, m, text) = &handoffs(&st, &reviewer)[0];
    assert!(
        text.starts_with("Handed over by Builder\n\nReview"),
        "{text}"
    );
    assert_eq!(
        failed_reason(&st, &reviewer, m).as_deref(),
        Some("not sent: the team was stopped before Reviewer got this handoff")
    );
    crate::common::wait_for("the boot ended", || !st.is_booting(&id)).await;
    settle().await;
    assert_eq!(handoffs(&st, &reviewer).len(), 1);
}

/// A previous start's "not sent" record, written after a newer start began, belongs to
/// that previous start: it must not count as the newer start's first handoff.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn records_of_a_previous_start_do_not_count_in_the_next() {
    let (_home, st, router) = served(Arc::new(TestStub::slow("team.reviewer@"))).await;
    let id = team(
        &router,
        "Leak",
        &[
            ("builder", "Builder", "Build"),
            ("reviewer", "Reviewer", "Review"),
        ],
        &[("builder", "reviewer", "hands-work-to")],
    )
    .await;
    let (builder, reviewer) = (seat(&id, "builder"), seat(&id, "reviewer"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("builder finished", || completed_turns(&st, &builder) == 1).await;
    // A newer start of the same team (the second RigBooted) while the reviewer still starts.
    let newer = st
        .local_event(
            Actor::System("test".into()),
            Subject::Rig(id.clone()),
            Provenance::Operator,
            EventBody::RigBooted {
                spec_hash: "newer".into(),
                seat_count: 2,
            },
        )
        .unwrap()
        .event
        .seq;
    crate::common::wait_for("the older handoff recorded as not sent", || {
        handoffs(&st, &reviewer).iter().any(|(seq, m, _)| {
            *seq > newer
                && failed_reason(&st, &reviewer, m).as_deref()
                    == Some("not sent: the team was started again before Reviewer got this handoff")
        })
    })
    .await;
    // In the newer start the builder has not worked yet: the reviewer waits for it.
    crate::common::wait_for("the reviewer waits in the newer start", || {
        crate::common::lifecycle(&st, &reviewer) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &reviewer) == Some(vec![builder.clone()])
    })
    .await;
    settle().await;
    assert_eq!(handoffs(&st, &reviewer).len(), 1);
    // The builder's turn from the older start is not this start's: nothing to hand over yet.
    let (status, body) = hand_over(&router, &builder, "h-old").await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("nothing_to_hand_over")),
        "{body}"
    );
    crate::common::wait_for("the boot ended", || !st.is_booting(&id)).await;
}

#[tokio::test]
async fn a_handoff_to_an_agent_that_cannot_start_is_not_sent_then_restart_delivers_it() {
    let (_home, st, router) = served(Arc::new(TestStub::broken("team.reviewer@", 1))).await;
    let id = team(
        &router,
        "Broken",
        &[
            ("builder", "Builder", "Build"),
            ("reviewer", "Reviewer", "Review"),
        ],
        &[("builder", "reviewer", "hands-work-to")],
    )
    .await;
    let reviewer = seat(&id, "reviewer");
    start(&router, &id, "s1").await;
    crate::common::wait_for("the reviewer's handoff recorded", || {
        handoffs(&st, &reviewer).len() == 1
    })
    .await;
    let (_, m, _) = handoffs(&st, &reviewer)[0].clone();
    crate::common::wait_for("a reason", || failed_reason(&st, &reviewer, &m).is_some()).await;
    let reason = failed_reason(&st, &reviewer, &m).unwrap();
    assert!(
        reason.starts_with("not sent: Reviewer is not running ("),
        "{reason}"
    );
    assert!(
        reason.contains("/nowhere"),
        "the host's reason is kept: {reason}"
    );
    assert!(
        !reason.contains(&reviewer),
        "names, never seat ids: {reason}"
    );
    crate::common::wait_for("the reviewer is stuck", || {
        stuck(&st, &reviewer) == Some(("the handoff from Builder was not sent".into(), true))
    })
    .await;
    crate::common::wait_for("the boot ended", || !st.is_booting(&id)).await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{reviewer}/resend"),
        json!({ "request_id": "r0" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("agent_not_running")),
        "{body}"
    );

    // Restart this agent: it relaunches and gets its first handoff once.
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{reviewer}/restart"),
        json!({ "request_id": "rs1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the re-sent handoff is read", || {
        handoffs(&st, &reviewer)
            .iter()
            .any(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("not stuck any more", || stuck(&st, &reviewer).is_none()).await;
    settle().await;
    let copies = handoffs(&st, &reviewer);
    assert_eq!(copies.len(), 2, "the original and one re-send: {copies:?}");
    assert_eq!(copies[0].2, copies[1].2, "the same handoff");
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{reviewer}/resend"),
        json!({ "request_id": "r1" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("nothing_to_resend")),
        "{body}"
    );
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{reviewer}/restart"),
        json!({ "request_id": "rs2" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("agent_not_restartable")),
        "{body}"
    );
}

/// An upstream that cannot start leaves its downstream stuck, in words; restarting it
/// sends its Job (it never read it), and its work then hands over.
#[tokio::test]
async fn a_stuck_upstream_is_shown_and_restarting_it_runs_the_chain() {
    let (_home, st, router) = served(Arc::new(TestStub::broken("team.builder@", 1))).await;
    let id = team(
        &router,
        "Upstream",
        &[
            ("builder", "Builder", "Build"),
            ("reviewer", "Reviewer", "Review"),
        ],
        &[("builder", "reviewer", "hands-work-to")],
    )
    .await;
    let (builder, reviewer) = (seat(&id, "builder"), seat(&id, "reviewer"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("the reviewer is stuck on the builder", || {
        waiting_for(&st, &reviewer) == Some(vec![builder.clone()])
            && stuck(&st, &reviewer) == Some(("Builder needs attention".into(), false))
    })
    .await;
    crate::common::wait_for("the boot ended", || !st.is_booting(&id)).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{builder}/restart"),
        json!({ "request_id": "rs1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the reviewer's handoff is read", || {
        handoffs(&st, &reviewer)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    let jobs = messages_by(&st, &builder, TEAM_START_ACTOR);
    assert_eq!(
        jobs.iter().filter(|(_, m, _)| consumed(&st, m)).count(),
        1,
        "the Job was read once: {jobs:?}"
    );
    assert!(stuck(&st, &reviewer).is_none());
}

/// A handoff recorded but never sent (controld stopped between the two) shows as stuck
/// "not delivered"; Resend sends it once, and never again once it was delivered.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_handoff_lost_between_recording_and_sending_can_be_resent_once() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Lost",
        &[("a", "Asker", ""), ("b", "Checker", "Check")],
        &[("a", "b", "hands-work-to")],
    )
    .await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("b runs and waits", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &b).is_some()
    })
    .await;
    crate::common::wait_for("the boot ended", || !st.is_booting(&id)).await;
    let start_seq = {
        let store = st.store.lock().unwrap();
        store
            .latest_of_types(&Subject::Rig(id.clone()), &["rig-booted"], 1)
            .unwrap()[0]
            .seq
    };
    let write = HandoffWrite {
        seat: b.clone(),
        start_seq,
        first: true,
        actor: Actor::System(HANDOFF_ACTOR.into()),
        message_id: "lost-1".into(),
        intent: model::MessageIntent::Inform,
        text: "Handed over by Asker\n\nCheck\n\n--- Asker's result ---\nlost".into(),
        from: vec![HandoffSource {
            seat: a.clone(),
            turn_id: "turn-9".into(),
            turn_seq: 1,
            requested_seq: None,
        }],
        wave: None,
        failed: None,
    };
    let items = st
        .store
        .lock()
        .unwrap()
        .record_handoff(&write)
        .unwrap()
        .expect("recorded");
    st.publish(items);
    controld::handoffs::process_rig(&st, &id, true).await;
    crate::common::wait_for("stuck: not delivered", || {
        stuck(&st, &b) == Some(("the handoff from Asker was not delivered".into(), true))
    })
    .await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/resend"),
        json!({ "request_id": "r1" }),
    )
    .await;
    assert_eq!(
        (status, body["resent"].clone()),
        (StatusCode::OK, json!(1)),
        "{body}"
    );
    crate::common::wait_for("the re-sent copy is read", || {
        handoffs(&st, &b).iter().any(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("not stuck any more", || stuck(&st, &b).is_none()).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/resend"),
        json!({ "request_id": "r2" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("nothing_to_resend")),
        "{body}"
    );
    settle().await;
    assert_eq!(
        handoffs(&st, &b).len(),
        2,
        "the lost original and exactly one re-send"
    );
}

/// A handoff the run budget holds until a slot frees (most agents working at once) is on its way,
/// not stuck: the agent's view says it waits for a slot.
#[tokio::test]
async fn a_handoff_waiting_for_a_slot_is_not_stuck() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Slots",
        &[("a", "Asker", ""), ("b", "Checker", "Check")],
        &[("a", "b", "hands-work-to")],
    )
    .await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("b runs and waits", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &b).is_some()
    })
    .await;
    crate::common::wait_for("the boot ended", || !st.is_booting(&id)).await;
    let start_seq = {
        let store = st.store.lock().unwrap();
        store
            .latest_of_types(&Subject::Rig(id.clone()), &["rig-booted"], 1)
            .unwrap()[0]
            .seq
    };
    let write = HandoffWrite {
        seat: b.clone(),
        start_seq,
        first: true,
        actor: Actor::System(HANDOFF_ACTOR.into()),
        message_id: "held-1".into(),
        intent: model::MessageIntent::Inform,
        text: "Handed over by Asker\n\nCheck\n\n--- Asker's result ---\nheld".into(),
        from: vec![HandoffSource {
            seat: a.clone(),
            turn_id: "turn-9".into(),
            turn_seq: 1,
            requested_seq: None,
        }],
        wave: None,
        failed: None,
    };
    let items = st
        .store
        .lock()
        .unwrap()
        .record_handoff(&write)
        .unwrap()
        .expect("recorded");
    st.publish(items);
    st.store.lock().unwrap().connection()
        .execute("INSERT INTO budget_queue (seat, message_id, json, at_ms) VALUES (?1, 'held-1', '{}', 1)", [&b]).unwrap();
    controld::handoffs::process_rig(&st, &id, true).await;
    settle().await;
    assert_eq!(
        stuck(&st, &b),
        None,
        "a handoff waiting for a slot is not stuck"
    );
    assert!(
        st.store
            .lock()
            .unwrap()
            .seat(&b)
            .unwrap()
            .unwrap()
            .slot_wait,
        "its view says it waits for a slot"
    );
}

/// controld restarts while hostd (and its agents) keep running. Nothing is handed over
/// twice, and a fan-in that was half done before the restart completes after it.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn handoffs_survive_a_controld_restart_without_duplicates() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("controld.sqlite");
    let (st1, router1, url1, server1) = serve_with(home.path(), Store::open(&path).unwrap()).await;
    let sup = supervisor(Arc::new(StubDriver));
    let link1 = link(&st1, url1, sup.clone()).await;
    let id = team(
        &router1,
        "Restart",
        &[
            ("planner", "Planner", "Plan"),
            ("designer", "Designer", ""),
            ("builder", "Builder", "Build"),
            ("checker", "Checker", "Check"),
        ],
        &[
            ("planner", "builder", "hands-work-to"),
            ("designer", "builder", "hands-work-to"),
            ("builder", "checker", "hands-work-to"),
        ],
    )
    .await;
    let (designer, builder, checker) = (
        seat(&id, "designer"),
        seat(&id, "builder"),
        seat(&id, "checker"),
    );
    start(&router1, &id, "s1").await;
    crate::common::wait_for("builder waits for the designer", || {
        waiting_for(&st1, &builder) == Some(vec![designer.clone()])
    })
    .await;
    crate::common::wait_for("the boot finished", || {
        !st1.is_booting(&id)
            && crate::common::lifecycle(&st1, &checker) == Some(SeatLifecycle::Running)
    })
    .await;

    // Restart controld on the same store; hostd keeps its agents and reconnects.
    server1.abort();
    link1.abort();
    drop(router1);
    let (st2, router2, url2, _server2) = serve_with(home.path(), Store::open(&path).unwrap()).await;
    link(&st2, url2, sup).await;
    settle().await;
    assert_eq!(
        waiting_for(&st2, &builder),
        Some(vec![designer.clone()]),
        "still waiting after the restart"
    );
    assert!(handoffs(&st2, &builder).is_empty());

    send(&router2, &designer, "d1", "Draw").await;
    crate::common::wait_for("the checker's handoff", || {
        handoffs(&st2, &checker).len() == 1
    })
    .await;
    let b = handoffs(&st2, &builder);
    assert_eq!(b.len(), 1);
    assert!(b[0].2.starts_with("Handed over by Planner, Designer\n\nBuild\n\n--- Planner's result ---\nstub received: Plan"), "{}", b[0].2);

    // Another restart replays and re-plans everything: still exactly one handoff each.
    let (st3, _router3, _url3, _server3) =
        serve_with(home.path(), Store::open(&path).unwrap()).await;
    settle().await;
    assert_eq!(
        (
            handed_over_count(&st3, &builder),
            handed_over_count(&st3, &checker)
        ),
        (1, 1)
    );
    assert_eq!(
        (
            handoffs(&st3, &builder).len(),
            handoffs(&st3, &checker).len()
        ),
        (1, 1)
    );
}

/// A store from before handoffs, with a started team still running and a finished work
/// turn: upgrading must not plan that start (no phantom handoff, no catch-up send).
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn upgrading_a_store_with_a_running_team_sends_no_handoffs() {
    let home = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("controld.sqlite");
    let draft =
        rigspec::templates::template("builder-reviewer", "old-team", "Old team", true).unwrap();
    let (a, b) = (
        draft.seat_id("builder").unwrap(),
        draft.seat_id("reviewer").unwrap(),
    );
    {
        let mut store = Store::open(&path).unwrap();
        store.insert_draft(&draft).unwrap();
        let rig = rigspec::from_draft(&draft).unwrap().rig;
        store
            .upsert_rig(
                &rig,
                &RigSource::Team {
                    id: "old-team".into(),
                    revision: 0,
                },
                model::Desired::Running,
            )
            .unwrap();
        store
            .append_local(
                Actor::Human("local".into()),
                Subject::Rig("old-team".into()),
                Provenance::Operator,
                EventBody::RigBooted {
                    spec_hash: "x".into(),
                    seat_count: 2,
                },
            )
            .unwrap();
        store
            .append_local(
                Actor::System(TEAM_START_ACTOR.into()),
                Subject::Seat(a.clone()),
                Provenance::Operator,
                EventBody::MessageQueued {
                    message_id: "job-1".into(),
                    intent: model::MessageIntent::Inform,
                    text: "Build".into(),
                },
            )
            .unwrap();
        let mut host_seq = 0;
        let mut host = |store: &mut Store, seat: &str, body: EventBody| {
            host_seq += 1;
            let ev = host_proto::HostEvent {
                host_seq,
                at_ms: model::now_ms(),
                actor: Actor::System("hostd".into()),
                subject: Subject::Seat(seat.to_string()),
                provenance: Provenance::ProtocolObserved,
                body,
            };
            store.ingest("local", ev).unwrap();
        };
        for s in [&a, &b] {
            host(
                &mut store,
                s,
                EventBody::SeatLifecycleChanged {
                    lifecycle: SeatLifecycle::Running,
                    reason: None,
                },
            );
        }
        host(
            &mut store,
            &a,
            EventBody::TurnStarted {
                turn_id: "turn-1".into(),
            },
        );
        host(
            &mut store,
            &a,
            EventBody::MessageConsumed {
                message_id: "job-1".into(),
                turn_id: "turn-1".into(),
            },
        );
        host(
            &mut store,
            &a,
            EventBody::AgentText {
                turn_id: Some("turn-1".into()),
                text: "built".into(),
            },
        );
        host(
            &mut store,
            &a,
            EventBody::TurnEnded {
                turn_id: "turn-1".into(),
                status: TurnStatus::Completed,
            },
        );
    }
    // Make it the store an M1.5 build wrote: schema 4, before handoffs.
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "ALTER TABLE seats DROP COLUMN waiting_for; ALTER TABLE seats DROP COLUMN handoff_stuck;
             DROP TABLE handoff_cutoff; PRAGMA user_version = 4;",
        )
        .unwrap();
    }
    let (st, _router, _url, _server) = serve_with(home.path(), Store::open(&path).unwrap()).await;
    settle().await;
    let planned = events(&st, &b).into_iter().filter(|e| {
        matches!(
            e.body,
            EventBody::HandedOver { .. } | EventBody::HandoffWaiting { .. }
        ) || e.actor == Actor::System(HANDOFF_ACTOR.into())
    });
    assert_eq!(
        planned.count(),
        0,
        "nothing planned for a start from before handoffs: {:?}",
        events(&st, &b)
    );
    assert_eq!(waiting_for(&st, &b), None);
}

// ---- fake-claude: real driver turn shapes ----

fn claude_supervisor() -> Supervisor {
    supervisor(Arc::new(ClaudeStreamDriver {
        program: crate::common::fake_bin("fake-claude"),
    }))
}

fn reason(st: &AppState, seat: &str) -> String {
    stuck(st, seat).map(|(r, _)| r).unwrap_or_default()
}

/// fake-claude: a first agent's Job is interrupted. Its work is unfinished (it says so,
/// and so do the agents waiting on it, transitively). Telling it "go" finishes the work,
/// and the pipeline completes.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn an_interrupted_job_is_stuck_until_the_operator_says_go() {
    let home = tempfile::tempdir().unwrap();
    let work = home.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let (st, router, url) = serve(home.path()).await;
    link(&st, url, claude_supervisor()).await;
    let id = team_of(
        &router,
        "Hang",
        "claude",
        Some(&work),
        &[
            ("lead", "Lead", "hang"),
            ("mid", "Mid", "Build"),
            ("last", "Last", "Check"),
        ],
        &[
            ("lead", "mid", "hands-work-to"),
            ("mid", "last", "hands-work-to"),
        ],
    )
    .await;
    let (lead, mid, last) = (seat(&id, "lead"), seat(&id, "mid"), seat(&id, "last"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("the lead is in its turn", || {
        events(&st, &lead)
            .iter()
            .any(|e| matches!(e.body, EventBody::ToolCalled { .. }))
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{lead}/interrupt"),
        json!({ "request_id": "i1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the lead is stuck on its unfinished work", || {
        reason(&st, &lead) == "Lead's work did not finish (interrupted)"
    })
    .await;
    crate::common::wait_for("mid and last are stuck behind it", || {
        reason(&st, &mid) == "waiting on Lead (stuck)"
            && reason(&st, &last) == "waiting on Mid (stuck)"
    })
    .await;
    assert!(
        handoffs(&st, &mid).is_empty(),
        "an interrupted turn hands nothing over"
    );

    send(&router, &lead, "go", "go").await;
    crate::common::wait_for("the pipeline completes", || {
        handoffs(&st, &last)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    assert!(
        handoffs(&st, &mid)[0]
            .2
            .ends_with("--- Lead's result ---\nfake: go"),
        "{}",
        handoffs(&st, &mid)[0].2
    );
    crate::common::wait_for("nobody is stuck", || {
        [&lead, &mid, &last].iter().all(|s| stuck(&st, s).is_none())
    })
    .await;
    stop(&router, &id, "x1").await;
}

/// fake-claude: a first agent crashes after reading its Job. Restart this agent sends the
/// Job again, asking it to continue, and the pipeline completes.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_job_that_crashed_mid_way_is_continued_by_restart() {
    let home = tempfile::tempdir().unwrap();
    let work = home.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let (st, router, url) = serve(home.path()).await;
    link(&st, url, claude_supervisor()).await;
    let id = team_of(
        &router,
        "Crash",
        "claude",
        Some(&work),
        &[("lead", "Lead", "crash"), ("next", "Next", "Go on")],
        &[("lead", "next", "hands-work-to")],
    )
    .await;
    let (lead, next) = (seat(&id, "lead"), seat(&id, "next"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("the lead is stuck on its unfinished work", || {
        reason(&st, &lead) == "Lead's work did not finish (the agent stopped)"
    })
    .await;
    crate::common::wait_for("the next agent is stuck behind it", || {
        reason(&st, &next).starts_with("Lead ")
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    assert!(handoffs(&st, &next).is_empty());

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{lead}/restart"),
        json!({ "request_id": "rs1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the pipeline completes", || {
        handoffs(&st, &next)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    let jobs = messages_by(&st, &lead, TEAM_START_ACTOR);
    assert_eq!(jobs.len(), 2, "the Job and its continuation: {jobs:?}");
    assert_eq!(
        jobs[1].2,
        "Your previous attempt did not finish. Continue from where you left off; check what is already written before redoing work.\n\ncrash"
    );
    assert!(
        handoffs(&st, &next)[0]
            .2
            .contains("--- Lead's result ---\nfake: Your previous attempt did not finish.")
    );
    crate::common::wait_for("nobody is stuck", || {
        stuck(&st, &lead).is_none() && stuck(&st, &next).is_none()
    })
    .await;
    stop(&router, &id, "x1").await;
}

/// "More work" moves in waves: in a diamond, the agent where both paths meet runs once.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_wave_runs_each_downstream_agent_once() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Diamond",
        &[
            ("a", "Source", "Start"),
            ("b", "Left", "L"),
            ("c", "Right", "R"),
            ("d", "Join", "J"),
        ],
        &[
            ("a", "b", "hands-work-to"),
            ("a", "c", "hands-work-to"),
            ("b", "d", "hands-work-to"),
            ("c", "d", "hands-work-to"),
        ],
    )
    .await;
    let (a, b, c, d) = (
        seat(&id, "a"),
        seat(&id, "b"),
        seat(&id, "c"),
        seat(&id, "d"),
    );
    start(&router, &id, "s1").await;
    crate::common::wait_for("the join read its first handoff", || {
        handoffs(&st, &d)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    assert!(
        handoffs(&st, &d)[0]
            .2
            .starts_with("Handed over by Left, Right")
    );
    settle().await;
    let before: usize = [&a, &b, &c, &d]
        .iter()
        .map(|s| completed_turns(&st, s))
        .sum();

    assert_eq!(hand_over(&router, &a, "h1").await.0, StatusCode::OK);
    crate::common::wait_for("the join read the wave", || {
        handoffs(&st, &d)
            .get(1)
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    settle().await;
    let d_msgs = handoffs(&st, &d);
    assert_eq!(d_msgs.len(), 2, "one wave message for the join: {d_msgs:?}");
    assert!(
        d_msgs[1]
            .2
            .starts_with("More work from Left, Right\n\n--- Left's result ---"),
        "{}",
        d_msgs[1].2
    );
    let after: usize = [&a, &b, &c, &d]
        .iter()
        .map(|s| completed_turns(&st, s))
        .sum();
    assert_eq!(after - before, 3, "Left, Right and Join each ran once");
    let waves: Vec<_> = events(&st, &d)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::HandedOver { wave, .. } => wave,
            _ => None,
        })
        .collect();
    assert_eq!(waves.len(), 1);
    assert_eq!(waves[0].origin, a, "the wave started at the source");
}

/// An agent whose last lifecycle change belongs to an earlier start (stopped, not yet
/// relaunched in this one) is still starting, not "stopped": a handoff for it waits.
#[tokio::test]
async fn a_handoff_waits_for_an_agent_stopped_before_this_start() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Again",
        &[
            ("builder", "Builder", "Build"),
            ("reviewer", "Reviewer", "Review"),
        ],
        &[("builder", "reviewer", "hands-work-to")],
    )
    .await;
    let (builder, reviewer) = (seat(&id, "builder"), seat(&id, "reviewer"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("first run done", || {
        handoffs(&st, &reviewer)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{reviewer}/stop"),
        json!({ "request_id": "x-rev" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("reviewer stopped", || {
        crate::common::lifecycle(&st, &reviewer) == Some(SeatLifecycle::Stopped)
    })
    .await;
    // A newer start, as at Start team: the reviewer is not relaunched yet.
    st.local_event(
        Actor::System("test".into()),
        Subject::Rig(id.clone()),
        Provenance::Operator,
        EventBody::RigBooted {
            spec_hash: "again".into(),
            seat_count: 2,
        },
    )
    .unwrap();
    send(&router, &builder, "b1", "Build again").await;
    crate::common::wait_for("the builder answered", || {
        completed_turns(&st, &builder) == 2
    })
    .await;
    assert_eq!(hand_over(&router, &builder, "h1").await.0, StatusCode::OK);
    settle().await;
    assert_eq!(
        handoffs(&st, &reviewer).len(),
        1,
        "the new start's handoff waits; it is not recorded as not sent"
    );
    assert_eq!(stuck(&st, &reviewer), None);

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{reviewer}/restart"),
        json!({ "request_id": "rs" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the new start's handoff is read", || {
        handoffs(&st, &reviewer)
            .get(1)
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    assert!(
        handoffs(&st, &reviewer)
            .iter()
            .all(|(_, m, _)| failed_reason(&st, &reviewer, m).is_none())
    );
}

/// A wave waits at the join until every branch it reaches has run, even when one branch
/// is held up (stopped, then restarted): the join still runs once.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_wave_waits_at_the_join_for_a_held_up_branch() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Held",
        &[
            ("a", "Source", "Start"),
            ("b", "Left", "L"),
            ("c", "Right", "R"),
            ("d", "Join", "J"),
        ],
        &[
            ("a", "b", "hands-work-to"),
            ("a", "c", "hands-work-to"),
            ("b", "d", "hands-work-to"),
            ("c", "d", "hands-work-to"),
        ],
    )
    .await;
    let (a, b, c, d) = (
        seat(&id, "a"),
        seat(&id, "b"),
        seat(&id, "c"),
        seat(&id, "d"),
    );
    start(&router, &id, "s1").await;
    crate::common::wait_for("the join read its first handoff", || {
        handoffs(&st, &d)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    crate::common::wait_for("the boot finished", || !st.is_booting(&id)).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{c}/stop"),
        json!({ "request_id": "x-c" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("right stopped", || {
        crate::common::lifecycle(&st, &c) == Some(SeatLifecycle::Stopped)
    })
    .await;

    assert_eq!(hand_over(&router, &a, "h1").await.0, StatusCode::OK);
    crate::common::wait_for("left ran the wave", || {
        handoffs(&st, &b).len() == 2 && completed_turns(&st, &b) == 2
    })
    .await;
    crate::common::wait_for("the join waits for the right branch", || {
        waiting_for(&st, &d) == Some(vec![c.clone()]) && reason(&st, &d) == "Right stopped"
    })
    .await;
    crate::common::wait_for("right says its handoff was not sent", || {
        reason(&st, &c) == "the handoff from Source was not sent"
    })
    .await;
    settle().await;
    assert_eq!(
        handoffs(&st, &d).len(),
        1,
        "no wave message for the join yet"
    );

    // Restart the right branch: it gets the wave message it never read, runs, and the join runs once.
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{c}/restart"),
        json!({ "request_id": "rs" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the join read the wave", || {
        handoffs(&st, &d)
            .get(1)
            .is_some_and(|(_, m, _)| consumed(&st, m))
    })
    .await;
    settle().await;
    let d_msgs = handoffs(&st, &d);
    assert_eq!(d_msgs.len(), 2, "{d_msgs:?}");
    assert!(
        d_msgs[1].2.starts_with("More work from Left, Right"),
        "{}",
        d_msgs[1].2
    );
    assert_eq!(
        completed_turns(&st, &d),
        2,
        "the join ran once for the wave"
    );
}

/// A handoff recorded as failed here, which the host reports the agent read anyway (the
/// send timed out but got through), is never re-sent and is not stuck.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_handoff_the_host_reports_read_is_never_resent() {
    let (_home, st, router) = served(Arc::new(StubDriver)).await;
    let id = team(
        &router,
        "Read",
        &[("a", "Asker", ""), ("b", "Checker", "Check")],
        &[("a", "b", "hands-work-to")],
    )
    .await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    start(&router, &id, "s1").await;
    crate::common::wait_for("b runs and waits", || {
        crate::common::lifecycle(&st, &b) == Some(SeatLifecycle::Running)
            && waiting_for(&st, &b).is_some()
    })
    .await;
    crate::common::wait_for("the boot ended", || !st.is_booting(&id)).await;
    let start_seq = st
        .store
        .lock()
        .unwrap()
        .latest_of_types(&Subject::Rig(id.clone()), &["rig-booted"], 1)
        .unwrap()[0]
        .seq;
    let write = HandoffWrite {
        seat: b.clone(),
        start_seq,
        first: true,
        actor: Actor::System(HANDOFF_ACTOR.into()),
        message_id: "timed-out".into(),
        intent: model::MessageIntent::Inform,
        text: "Handed over by Asker\n\nCheck\n\n--- Asker's result ---\nx".into(),
        from: vec![HandoffSource {
            seat: a.clone(),
            turn_id: "turn-9".into(),
            turn_seq: 1,
            requested_seq: None,
        }],
        wave: None,
        failed: Some("not sent: Checker could not be reached (timed out)".into()),
    };
    let items = st
        .store
        .lock()
        .unwrap()
        .record_handoff(&write)
        .unwrap()
        .expect("recorded");
    st.publish(items);
    let read = host_proto::HostEvent {
        host_seq: 1,
        at_ms: model::now_ms(),
        actor: Actor::System("hostd".into()),
        subject: Subject::Seat(b.clone()),
        provenance: Provenance::ProtocolObserved,
        body: EventBody::MessageConsumed {
            message_id: "timed-out".into(),
            turn_id: "turn-7".into(),
        },
    };
    let items = st.store.lock().unwrap().ingest("other", read).unwrap();
    st.publish(items);
    assert_eq!(
        state(&st, "timed-out"),
        Some(MessageState::Failed),
        "the store still says failed"
    );
    controld::handoffs::process_rig(&st, &id, true).await;
    settle().await;
    assert_eq!(stuck(&st, &b), None, "the host says it was read: not stuck");
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/seats/{b}/resend"),
        json!({ "request_id": "r1" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("nothing_to_resend")),
        "{body}"
    );
    assert_eq!(handoffs(&st, &b).len(), 1);
}

/// The airplane team (15 Claude agents, fake-claude): after the pipeline has run, one
/// operator message to Lead adds exactly one turn to the whole team.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn on_the_airplane_team_one_message_to_lead_is_one_turn() {
    let home = tempfile::tempdir().unwrap();
    let work = home.path().join("work");
    std::fs::create_dir(&work).unwrap();
    let (st, router, url) = serve(home.path()).await;
    link(&st, url, claude_supervisor()).await;
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/teams/airplane-c172.json"
    );
    let example: Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    let (status, rec) =
        call(&router, "POST", "/api/teams", json!({ "request_id": "create-air", "template": "blank", "practice": false, "name": "Airplane" })).await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    let id = draft["id"].as_str().unwrap().to_string();
    draft["groups"] = example["groups"].clone();
    draft["connections"] = example["connections"].clone();
    draft["agents"] = example["agents"].clone();
    let matching_items = draft["agents"].as_array_mut().unwrap();
    for a in matching_items {
        a["folder"] = json!(work.display().to_string());
    }
    let (status, body) = call(
        &router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": "put-air", "revision": 0, "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rec: Value = call(&router, "GET", &format!("/api/teams/{id}"), Value::Null)
        .await
        .1;
    let seat_of = |agent: &str| {
        let a = rec["draft"]["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == agent)
            .unwrap();
        format!("{}.{agent}@{id}", a["group_id"].as_str().unwrap_or("team"))
    };
    let seats: Vec<String> = rec["draft"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| seat_of(a["id"].as_str().unwrap()))
        .collect();
    let downstream: Vec<String> = rec["draft"]["connections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| seat_of(c["to"].as_str().unwrap()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    start(&router, &id, "s1").await;

    // The whole pipeline runs: every downstream agent reads its first handoff.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    while !downstream.iter().all(|s| {
        handoffs(&st, s)
            .first()
            .is_some_and(|(_, m, _)| consumed(&st, m))
    }) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the pipeline did not finish"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Quiet: no turn ends for a while.
    let mut last = turns_ended(&st, &seats);
    loop {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let now = turns_ended(&st, &seats);
        if now == last {
            break;
        }
        last = now;
    }
    let lead = seat_of("lead");
    send(&router, &lead, "to-lead", "Status?").await;
    crate::common::wait_for("Lead answered", || turns_ended(&st, &seats) > last).await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert_eq!(
        turns_ended(&st, &seats) - last,
        1,
        "one message to Lead is one turn, not a cascade"
    );

    // Hand over again on Lead: one wave, every agent downstream runs exactly once.
    let last = turns_ended(&st, &seats);
    let before: Vec<usize> = downstream.iter().map(|s| handoffs(&st, s).len()).collect();
    assert_eq!(hand_over(&router, &lead, "wave-1").await.0, StatusCode::OK);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    while !downstream.iter().zip(&before).all(|(s, n)| {
        handoffs(&st, s)
            .get(*n)
            .is_some_and(|(_, m, _)| consumed(&st, m))
    }) {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the wave did not reach everyone"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert_eq!(downstream.len(), 14);
    assert_eq!(
        turns_ended(&st, &seats) - last,
        14,
        "one wave from Lead runs each of the 14 agents downstream once"
    );
    let paired_downstream = downstream.iter().zip(&before);
    for (s, n) in paired_downstream {
        assert_eq!(handoffs(&st, s).len(), n + 1, "{s} got one wave message");
    }
    stop(&router, &id, "x-air").await;
}

// ---- store-level: events written as hostd and controld would, planned without the dispatcher ----

/// Writes a team's events straight into the store (nothing is published, so the dispatcher
/// does not react); `controld::handoffs::process_rig` then plans from them.
struct Script<'a> {
    st: &'a AppState,
    host_seq: u64,
}

impl Script<'_> {
    fn host(&mut self, seat: &str, body: EventBody) -> i64 {
        self.host_seq += 1;
        let ev = host_proto::HostEvent {
            host_seq: self.host_seq,
            at_ms: model::now_ms(),
            actor: Actor::System("hostd".into()),
            subject: Subject::Seat(seat.to_string()),
            provenance: Provenance::ProtocolObserved,
            body,
        };
        self.st
            .store
            .lock()
            .unwrap()
            .ingest("local", ev)
            .unwrap()
            .last()
            .unwrap()
            .event
            .seq
    }

    fn local(&mut self, actor: Actor, subject: Subject, body: EventBody) -> i64 {
        self.st
            .store
            .lock()
            .unwrap()
            .append_local(actor, subject, Provenance::Operator, body)
            .unwrap()
            .event
            .seq
    }

    fn running(&mut self, seats: &[&String]) {
        for s in seats {
            self.host(
                s,
                EventBody::SeatLifecycleChanged {
                    lifecycle: SeatLifecycle::Running,
                    reason: None,
                },
            );
        }
    }

    /// One turn that reads `message` and completes with `text`; returns its TurnEnded seq.
    fn turn(&mut self, seat: &str, turn: &str, message: &str, text: &str) -> i64 {
        self.host(
            seat,
            EventBody::MessageDelivered {
                message_id: message.into(),
            },
        );
        self.host(
            seat,
            EventBody::TurnStarted {
                turn_id: turn.into(),
            },
        );
        self.host(
            seat,
            EventBody::MessageConsumed {
                message_id: message.into(),
                turn_id: turn.into(),
            },
        );
        self.host(
            seat,
            EventBody::AgentText {
                turn_id: Some(turn.into()),
                text: text.into(),
            },
        );
        self.host(
            seat,
            EventBody::TurnEnded {
                turn_id: turn.into(),
                status: TurnStatus::Completed,
            },
        )
    }

    /// A handoff message recorded as the dispatcher records it.
    fn hand(
        &mut self,
        seat: &str,
        start_seq: i64,
        first: bool,
        id: &str,
        from: Vec<HandoffSource>,
        wave: Option<model::Wave>,
    ) {
        let write = HandoffWrite {
            seat: seat.to_string(),
            start_seq,
            first,
            actor: Actor::System(HANDOFF_ACTOR.into()),
            message_id: id.into(),
            intent: model::MessageIntent::Inform,
            text: format!("handoff {id}"),
            from,
            wave,
            failed: None,
        };
        self.st
            .store
            .lock()
            .unwrap()
            .record_handoff(&write)
            .unwrap()
            .expect("recorded");
    }
}

fn source(seat: &str, turn: &str, turn_seq: i64, requested_seq: Option<i64>) -> HandoffSource {
    HandoffSource {
        seat: seat.to_string(),
        turn_id: turn.into(),
        turn_seq,
        requested_seq,
    }
}

/// A wave waits at the join for an upstream agent that, after its process ended before
/// reading its wave message, is running again and idle with nothing left to do (nothing
/// unread, unfinished, undelivered or in a turn). That is a hidden stall: the join says
/// "stuck", naming the agent, instead of waiting for ever. While the agent still has
/// something to do (a message delivered and unread, or a turn in progress) it is not stuck.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_wait_on_an_agent_with_nothing_left_to_do_is_stuck() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    let id = team(
        &router,
        "Stall",
        &[
            ("a", "Source", "Start"),
            ("b", "Left", "L"),
            ("c", "Right", "R"),
            ("d", "Join", "J"),
        ],
        &[
            ("a", "b", "hands-work-to"),
            ("a", "c", "hands-work-to"),
            ("b", "d", "hands-work-to"),
            ("c", "d", "hands-work-to"),
        ],
    )
    .await;
    let (a, b, c, d) = (
        seat(&id, "a"),
        seat(&id, "b"),
        seat(&id, "c"),
        seat(&id, "d"),
    );
    {
        let mut store = st.store.lock().unwrap();
        let draft = store.draft(&id).unwrap().unwrap();
        let rig = rigspec::from_draft(&draft).unwrap().rig;
        store
            .upsert_rig(
                &rig,
                &RigSource::Team {
                    id: id.clone(),
                    revision: draft.revision,
                },
                model::Desired::Running,
            )
            .unwrap();
    }
    let mut s = Script {
        st: &st,
        host_seq: 0,
    };
    let start = s.local(
        Actor::Human("local".into()),
        Subject::Rig(id.clone()),
        EventBody::RigBooted {
            spec_hash: "x".into(),
            seat_count: 4,
        },
    );
    s.running(&[&a, &b, &c, &d]);
    s.local(
        Actor::System(TEAM_START_ACTOR.into()),
        Subject::Seat(a.clone()),
        EventBody::MessageQueued {
            message_id: "job-a".into(),
            intent: model::MessageIntent::Inform,
            text: "Start".into(),
        },
    );
    let ta = s.turn(&a, "t1", "job-a", "a done");
    s.hand(
        &b,
        start,
        true,
        "b-1",
        vec![source(&a, "t1", ta, None)],
        None,
    );
    let tb = s.turn(&b, "t1", "b-1", "b done");
    s.hand(
        &c,
        start,
        true,
        "c-1",
        vec![source(&a, "t1", ta, None)],
        None,
    );
    let tc = s.turn(&c, "t1", "c-1", "c done");
    s.hand(
        &d,
        start,
        true,
        "d-1",
        vec![source(&b, "t1", tb, None), source(&c, "t1", tc, None)],
        None,
    );
    s.turn(&d, "t1", "d-1", "d done");
    // "Hand over again" on the source: a wave. Right runs it; Left's message is delivered.
    let req = s.local(
        Actor::Human("local".into()),
        Subject::Seat(a.clone()),
        EventBody::HandoverRequested {
            turn_id: "t1".into(),
            turn_seq: ta,
        },
    );
    let wave = model::Wave {
        id: req,
        origin: a.clone(),
    };
    s.hand(
        &c,
        start,
        false,
        "c-2",
        vec![source(&a, "t1", ta, Some(req))],
        Some(wave.clone()),
    );
    s.turn(&c, "t2", "c-2", "c again");
    s.hand(
        &b,
        start,
        false,
        "b-2",
        vec![source(&a, "t1", ta, Some(req))],
        Some(wave),
    );
    s.host(
        &b,
        EventBody::MessageDelivered {
            message_id: "b-2".into(),
        },
    );

    controld::handoffs::process_rig(&st, &id, false).await;
    assert_eq!(waiting_for(&st, &d), Some(vec![b.clone()]));
    assert_eq!(stuck(&st, &d), None, "Left has its wave message to read");

    // Left's process ends before reading it; it runs again, idle, and nothing re-sends it.
    s.host(
        &b,
        EventBody::TenureEnded {
            tenure: 1,
            exit_code: Some(1),
            signal: None,
            stderr_tail: String::new(),
        },
    );
    s.host(
        &b,
        EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Stopped,
            reason: None,
        },
    );
    s.running(&[&b]);
    controld::handoffs::process_rig(&st, &id, false).await;
    assert_eq!(waiting_for(&st, &d), Some(vec![b.clone()]));
    assert_eq!(
        stuck(&st, &d),
        Some(("waiting on Left, who has nothing left to do".into(), false))
    );
    assert_eq!(stuck(&st, &b), None, "Left itself is not stuck");

    // A turn in progress is something to do; a chat turn that ends is not the wave's work.
    s.local(
        Actor::Human("local".into()),
        Subject::Seat(b.clone()),
        EventBody::MessageQueued {
            message_id: "chat-1".into(),
            intent: model::MessageIntent::Instruct,
            text: "How is it going?".into(),
        },
    );
    s.host(
        &b,
        EventBody::MessageDelivered {
            message_id: "chat-1".into(),
        },
    );
    s.host(
        &b,
        EventBody::TurnStarted {
            turn_id: "t9".into(),
        },
    );
    s.host(
        &b,
        EventBody::MessageConsumed {
            message_id: "chat-1".into(),
            turn_id: "t9".into(),
        },
    );
    controld::handoffs::process_rig(&st, &id, false).await;
    assert_eq!(stuck(&st, &d), None, "Left is in a turn");
    s.host(
        &b,
        EventBody::TurnEnded {
            turn_id: "t9".into(),
            status: TurnStatus::Completed,
        },
    );
    controld::handoffs::process_rig(&st, &id, false).await;
    assert_eq!(
        stuck(&st, &d),
        Some(("waiting on Left, who has nothing left to do".into(), false))
    );
}

/// Planning reads only the current start: an agent with about 20,000 events from earlier
/// starts (handoffs, deliveries, reads and turns) still plans in well under 100 ms.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn an_agent_with_a_long_history_in_earlier_starts_plans_quickly() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    let id = team(
        &router,
        "Long",
        &[("a", "Source", "Start"), ("b", "Worker", "Work")],
        &[("a", "b", "hands-work-to")],
    )
    .await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    {
        let mut store = st.store.lock().unwrap();
        let draft = store.draft(&id).unwrap().unwrap();
        let rig = rigspec::from_draft(&draft).unwrap().rig;
        store
            .upsert_rig(
                &rig,
                &RigSource::Team {
                    id: id.clone(),
                    revision: draft.revision,
                },
                model::Desired::Running,
            )
            .unwrap();
    }
    let mut s = Script {
        st: &st,
        host_seq: 0,
    };
    let human = || Actor::Human("local".into());
    // Four earlier starts, each with about 5,000 events on the worker.
    let mut n = 0;
    for run in 0..4 {
        let boot = s.local(
            human(),
            Subject::Rig(id.clone()),
            EventBody::RigBooted {
                spec_hash: "x".into(),
                seat_count: 2,
            },
        );
        s.running(&[&a, &b]);
        for i in 0..715 {
            let m = format!("old-{run}-{i}");
            s.local(
                Actor::System(HANDOFF_ACTOR.into()),
                Subject::Seat(b.clone()),
                EventBody::MessageQueued {
                    message_id: m.clone(),
                    intent: model::MessageIntent::Inform,
                    text: "More work from Source".into(),
                },
            );
            s.local(
                Actor::System(HANDOFF_ACTOR.into()),
                Subject::Seat(b.clone()),
                EventBody::HandedOver {
                    start_seq: boot,
                    message_id: m.clone(),
                    from: vec![source(&a, "t", i, Some(i))],
                    resend_of: None,
                    wave: None,
                },
            );
            s.turn(&b, &format!("t{i}"), &m, "worked");
            n += 7;
        }
        s.local(
            human(),
            Subject::Rig(id.clone()),
            EventBody::RigDesiredChanged {
                desired: model::Desired::Stopped,
            },
        );
    }
    assert!(n >= 20_000, "{n}");
    // The current start: the source worked; the worker read its first handoff and a wave.
    let start = s.local(
        human(),
        Subject::Rig(id.clone()),
        EventBody::RigBooted {
            spec_hash: "x".into(),
            seat_count: 2,
        },
    );
    s.running(&[&a, &b]);
    s.local(
        Actor::System(TEAM_START_ACTOR.into()),
        Subject::Seat(a.clone()),
        EventBody::MessageQueued {
            message_id: "job-a".into(),
            intent: model::MessageIntent::Inform,
            text: "Start".into(),
        },
    );
    let ta = s.turn(&a, "n1", "job-a", "a done");
    s.hand(
        &b,
        start,
        true,
        "new-1",
        vec![source(&a, "n1", ta, None)],
        None,
    );
    s.turn(&b, "n1", "new-1", "b done");
    let req = s.local(
        human(),
        Subject::Seat(a.clone()),
        EventBody::HandoverRequested {
            turn_id: "n1".into(),
            turn_seq: ta,
        },
    );
    s.hand(
        &b,
        start,
        false,
        "new-2",
        vec![source(&a, "n1", ta, Some(req))],
        Some(model::Wave {
            id: req,
            origin: a.clone(),
        }),
    );
    s.turn(&b, "n2", "new-2", "b again");

    controld::handoffs::process_rig(&st, &id, false).await;
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let t = std::time::Instant::now();
        controld::handoffs::process_rig(&st, &id, false).await;
        best = best.min(t.elapsed());
    }
    assert!(best < Duration::from_millis(100), "planning took {best:?}");
    assert_eq!((waiting_for(&st, &b), stuck(&st, &b)), (None, None));
}

/// Planning cost inside one long start: an agent with about 20,000 events in the current
/// start (turns with tool calls, results, replies and usage) plans in under 20 ms in a
/// release build (100 ms in a debug build): the planner reads only the event types it
/// needs, through the subject-and-type index.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn an_agent_with_a_long_current_start_plans_quickly() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    let id = team(
        &router,
        "Busy",
        &[("a", "Source", "Start"), ("b", "Worker", "Work")],
        &[("a", "b", "hands-work-to")],
    )
    .await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    {
        let mut store = st.store.lock().unwrap();
        let draft = store.draft(&id).unwrap().unwrap();
        let rig = rigspec::from_draft(&draft).unwrap().rig;
        store
            .upsert_rig(
                &rig,
                &RigSource::Team {
                    id: id.clone(),
                    revision: draft.revision,
                },
                model::Desired::Running,
            )
            .unwrap();
    }
    let mut s = Script {
        st: &st,
        host_seq: 0,
    };
    let human = || Actor::Human("local".into());
    let start = s.local(
        human(),
        Subject::Rig(id.clone()),
        EventBody::RigBooted {
            spec_hash: "x".into(),
            seat_count: 2,
        },
    );
    s.running(&[&a, &b]);
    s.local(
        Actor::System(TEAM_START_ACTOR.into()),
        Subject::Seat(a.clone()),
        EventBody::MessageQueued {
            message_id: "job-a".into(),
            intent: model::MessageIntent::Inform,
            text: "Start".into(),
        },
    );
    let ta = s.turn(&a, "n1", "job-a", "a done");
    s.hand(
        &b,
        start,
        true,
        "new-1",
        vec![source(&a, "n1", ta, None)],
        None,
    );
    s.turn(&b, "n1", "new-1", "b done");
    // A long session in this start: chat turns, each with tool calls, results, replies and usage.
    let mut n = 0;
    for i in 0..1000 {
        let (turn, m) = (format!("c{i}"), format!("chat-{i}"));
        s.local(
            human(),
            Subject::Seat(b.clone()),
            EventBody::MessageQueued {
                message_id: m.clone(),
                intent: model::MessageIntent::Instruct,
                text: "go on".into(),
            },
        );
        s.host(
            &b,
            EventBody::MessageDelivered {
                message_id: m.clone(),
            },
        );
        s.host(
            &b,
            EventBody::TurnStarted {
                turn_id: turn.clone(),
            },
        );
        s.host(
            &b,
            EventBody::MessageConsumed {
                message_id: m,
                turn_id: turn.clone(),
            },
        );
        for k in 0..5 {
            let call = format!("{turn}-{k}");
            let t = Some(turn.clone());
            s.host(
                &b,
                EventBody::ToolCalled {
                    turn_id: t.clone(),
                    call_id: call.clone(),
                    name: "Bash".into(),
                    input: json!({ "command": "ls" }),
                    parent_call_id: None,
                },
            );
            s.host(
                &b,
                EventBody::ToolResulted {
                    turn_id: t.clone(),
                    call_id: call,
                    output: "a\nb".into(),
                    is_error: false,
                },
            );
            s.host(
                &b,
                EventBody::UsageReported {
                    in_turn: false,
                    turn_id: t,
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    cost: None,
                },
            );
        }
        s.host(
            &b,
            EventBody::AgentText {
                turn_id: Some(turn.clone()),
                text: "working".into(),
            },
        );
        s.host(
            &b,
            EventBody::TurnEnded {
                turn_id: turn,
                status: TurnStatus::Completed,
            },
        );
        n += 21;
    }
    assert!(n >= 20_000, "{n}");
    controld::handoffs::process_rig(&st, &id, false).await;
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let t = std::time::Instant::now();
        controld::handoffs::process_rig(&st, &id, false).await;
        best = best.min(t.elapsed());
    }
    eprintln!("planning with ~20,000 events in the current start: {best:?}");
    // About 9 ms in a release build; a debug build parses JSON several times slower.
    let bound = Duration::from_millis(if cfg!(debug_assertions) { 100 } else { 20 });
    assert!(best < bound, "planning took {best:?}");
}
