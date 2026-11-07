//! Starting and stopping teams. What needs no running agent (validation, conflicts, claims, an
//! agent runner that is not connected) is checked directly; what needs agents to run is a live test
//! on the real Claude Code CLI (`common::live`), asserting on what Sculptured records.
mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::lifecycle;
use controld::AppState;
use controld::reconciler::{HANDOFF_ACTOR, TEAM_START_ACTOR};
use controld::store::Store;
use hostd::journal::Journal;
use hostd::supervisor::Supervisor;
use http_body_util::BodyExt;
use model::{Actor, EventBody, EventRecord, SeatLifecycle};
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;

const TOKEN: &str = "test-token";
/// Every Job in these tests: a tiny, cheap turn.
const OK_JOB: &str = "Reply with the single word OK and nothing else.";

/// Serves controld on an ephemeral port for hostd, and returns a router for direct calls.
async fn serve(home: &std::path::Path) -> (AppState, Router, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        TOKEN.into(),
        vec![addr.to_string(), "test".into()],
    )
    .with_home(home);
    let served = controld::app(st.clone(), None);
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    (
        st.clone(),
        controld::app(st, None),
        format!("ws://{addr}/host"),
    )
}

/// A host with the real CLIs (`common::live::drivers`).
async fn connect_host(st: &AppState, url: String) {
    let sup = Supervisor::new(
        "local".into(),
        common::live::drivers(),
        Journal::open_in_memory(10_000).unwrap(),
    );
    tokio::spawn(async move { hostd::link::run_link(&url, TOKEN, &sup).await });
    crate::common::wait_for("host connected", || {
        st.hosts.has_driver("local", "claude-stream")
    })
    .await;
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

/// A builder-reviewer team as the template makes it: its agents have no folder, so it cannot start.
async fn team(router: &Router) -> String {
    let (status, rec) = call(
        router,
        "POST",
        "/api/teams",
        json!({ "request_id": uuid::Uuid::new_v4().to_string(), "template": "builder-reviewer" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    rec["draft"]["id"].as_str().unwrap().to_string()
}

/// A builder-reviewer team that can start (revision 1): every agent works in `<home>/work`, Claude
/// agents on the cheapest model, each with a tiny Job.
async fn ready_team(router: &Router, home: &std::path::Path) -> String {
    ready_team_with(router, home, "claude", OK_JOB).await
}

/// `ready_team`, with the builder of kind `builder_kind` given `builder_job`.
async fn ready_team_with(
    router: &Router,
    home: &std::path::Path,
    builder_kind: &str,
    builder_job: &str,
) -> String {
    let id = team(router).await;
    let work = home.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut draft = draft_json(router, &id).await;
    draft["folder"] = json!(work.display().to_string());
    draft["models"] = json!({ "claude": common::live::CLAUDE_MODEL });
    for a in draft["agents"].as_array_mut().unwrap() {
        let builder = a["id"] == "builder";
        a["kind"] = json!(if builder { builder_kind } else { "claude" });
        a["job"] = json!(if builder { builder_job } else { OK_JOB });
        a["folder"] = json!(work.display().to_string());
    }
    let (status, body) = call(
        router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": uuid::Uuid::new_v4().to_string(), "revision": 0, "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

fn events(st: &AppState, seat: &str) -> Vec<EventRecord> {
    st.store
        .lock()
        .unwrap()
        .events_for_seat(seat, 0, 5000)
        .unwrap()
}

fn job_messages(st: &AppState, seat: &str) -> Vec<(String, String)> {
    messages_by(st, seat, TEAM_START_ACTOR)
}

/// Messages handed over from an upstream agent (the reviewer's Job arrives this way).
fn handoff_messages(st: &AppState, seat: &str) -> Vec<(String, String)> {
    messages_by(st, seat, HANDOFF_ACTOR)
}

fn messages_by(st: &AppState, seat: &str, actor: &str) -> Vec<(String, String)> {
    events(st, seat)
        .into_iter()
        .filter(|e| e.actor == Actor::System(actor.into()))
        .filter_map(|e| match e.body {
            EventBody::MessageQueued {
                message_id, text, ..
            } => Some((message_id, common::work_message(&text))),
            _ => None,
        })
        .collect()
}

fn consumed(st: &AppState, seat: &str, message: &str) -> bool {
    events(st, seat).iter().any(|e| matches!(&e.body, EventBody::MessageConsumed { message_id, .. } if message_id == message))
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_start_delivers_each_job_as_a_first_message_sent_at_start() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team(&router, home.path()).await;
    let start = json!({ "request_id": "s1", "revision": 1 });
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        start.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rig"], id.as_str());

    let builder = format!("team.builder@{id}");
    let reviewer = format!("team.reviewer@{id}");
    // The builder hands work to the reviewer, so the reviewer's Job comes with that handoff.
    common::live::wait_for(
        "the builder's job and the reviewer's handoff consumed",
        || {
            job_messages(&st, &builder)
                .first()
                .is_some_and(|(m, _)| consumed(&st, &builder, m))
                && handoff_messages(&st, &reviewer)
                    .first()
                    .is_some_and(|(m, _)| consumed(&st, &reviewer, m))
        },
    )
    .await;
    assert!(
        job_messages(&st, &reviewer).is_empty(),
        "no Job at start for an agent that is handed work"
    );
    let handed = &handoff_messages(&st, &reviewer)[0].1;
    assert!(
        handed.starts_with(&format!("Handed over by Builder\n\n{OK_JOB}")),
        "{handed}"
    );
    let (_, text) = &job_messages(&st, &builder)[0];
    assert!(text.starts_with(OK_JOB), "{text}");
    assert!(
        events(&st, &builder)
            .iter()
            .any(|e| matches!(e.body, EventBody::AgentText { .. })),
        "the builder answered its Job"
    );

    assert_eq!(
        call(&router, "POST", &format!("/api/teams/{id}/start"), start).await,
        (status, body),
        "same request_id replays"
    );
    let (status, again) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s2" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(again["error"]["code"], "team_running");
    assert_eq!(
        job_messages(&st, &builder).len(),
        1,
        "a refused start sends nothing"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_restarting_a_stopped_team_delivers_jobs_again() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team(&router, home.path()).await;
    let builder = format!("team.builder@{id}");
    let (status, _) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    common::live::wait_for("first job consumed", || {
        job_messages(&st, &builder)
            .first()
            .is_some_and(|(m, _)| consumed(&st, &builder, m))
    })
    .await;
    common::live::wait_for("the boot finished", || !st.is_booting(&id)).await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": "x1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["failed"].is_array(),
        "stop reports the agents it could not stop: {body}"
    );
    common::live::wait_for("stopped", || {
        crate::common::lifecycle(&st, &builder) == Some(SeatLifecycle::Stopped)
    })
    .await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("second job consumed", || {
        let jobs = job_messages(&st, &builder);
        jobs.len() == 2 && consumed(&st, &builder, &jobs[1].0)
    })
    .await;
    assert!(
        !events(&st, &builder)
            .iter()
            .any(|e| matches!(e.body, EventBody::MessageFailed { .. })),
        "the earlier 'stopped' must not be read as a failed start"
    );
}

#[tokio::test]
async fn an_agent_that_cannot_start_gets_its_job_recorded_as_not_sent() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    let id = ready_team(&router, home.path()).await;
    let (status, _) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let builder = format!("team.builder@{id}");
    common::live::wait_for("job recorded as not sent", || {
        events(&st, &builder).iter().any(|e| {
            matches!(&e.body, EventBody::MessageFailed { reason, .. } if reason.starts_with("not sent: the agent did not start (Sculptured's agent runner is not connected)"))
        })
    })
    .await;
    assert_eq!(job_messages(&st, &builder).len(), 1);
}

#[tokio::test]
async fn start_refuses_invalid_teams_stale_revisions_and_stop_needs_a_start() {
    let home = tempfile::tempdir().unwrap();
    let (_st, router, _url) = serve(home.path()).await;
    let id = team(&router).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "team_invalid");
    let messages: Vec<&str> = body["error"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["message"].as_str().unwrap())
        .collect();
    assert!(
        messages.contains(&"Builder needs a folder to work in"),
        "{messages:?}"
    );

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s2", "revision": 7 }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("draft_conflict"))
    );
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": "x" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("team_not_started"))
    );
}

// ---- progress-ledger carry-forwards for Task 9 ----

/// The web's `TEAM_START_ACTOR` check reads `actor.kind === 'system' && actor.id === 'team-start'`.
#[test]
fn the_team_start_actor_serializes_as_the_web_expects() {
    assert_eq!(
        serde_json::to_value(Actor::System(TEAM_START_ACTOR.into())).unwrap(),
        json!({ "kind": "system", "id": "team-start" })
    );
}

/// An empty or blank Job is a real state ("no first message"), not a missing one: that
/// agent starts and gets no message at all, neither an empty one nor a "not sent" record.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_agent_with_an_empty_job_gets_no_first_message() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team_with(&router, home.path(), "claude", "  \n ").await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1", "revision": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let builder = format!("team.builder@{id}");
    let reviewer = format!("team.reviewer@{id}");
    // The builder has nothing to do until someone writes to it (that first message is its
    // work): the reviewer waits for it.
    common::live::wait_for("the reviewer waits for the builder", || {
        let view = st.store.lock().unwrap().seat(&reviewer).unwrap().unwrap();
        crate::common::lifecycle(&st, &reviewer) == Some(SeatLifecycle::Running)
            && view.waiting_for == Some(vec![builder.clone()])
            && view.handoff_stuck.is_none()
    })
    .await;
    assert_eq!(
        crate::common::lifecycle(&st, &builder),
        Some(SeatLifecycle::Running)
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    for s in [&builder, &reviewer] {
        let messages: Vec<_> = events(&st, s)
            .into_iter()
            .filter(|e| {
                matches!(
                    e.body,
                    EventBody::MessageQueued { .. } | EventBody::MessageFailed { .. }
                )
            })
            .collect();
        assert!(messages.is_empty(), "{s}: {messages:?}");
    }
}

/// The web reads a TenureEnded that follows `stopping` as an ordinary end and one that
/// follows `stopped` as "stopped unexpectedly", so an operator stop must journal
/// stopping → tenure-ended → stopped.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_operator_stop_ends_the_session_before_recording_stopped() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team(&router, home.path()).await;
    let builder = format!("team.builder@{id}");
    let (status, _) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    common::live::wait_for("job consumed", || {
        job_messages(&st, &builder)
            .first()
            .is_some_and(|(m, _)| consumed(&st, &builder, m))
    })
    .await;
    common::live::wait_for("the boot finished", || !st.is_booting(&id)).await;
    let before_stop = st.store.lock().unwrap().last_seq().unwrap();

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": "x1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("stopped", || {
        lifecycle(&st, &builder) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let order: Vec<String> = events(&st, &builder)
        .into_iter()
        .filter(|e| e.seq > before_stop)
        .filter_map(|e| match e.body {
            EventBody::SeatLifecycleChanged { lifecycle, .. } => {
                Some(format!("{lifecycle:?}").to_lowercase())
            }
            EventBody::TenureEnded { .. } => Some("tenure-ended".into()),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["stopping", "tenure-ended", "stopped"]);
}

// ---- Task 8 review follow-ups: save versus start ----

async fn draft_json(router: &Router, id: &str) -> Value {
    let (status, rec) = call(router, "GET", &format!("/api/teams/{id}"), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    rec["draft"].clone()
}

/// A structural edit: the builder's folder (part of what needs Stop team first).
fn with_builder_folder(mut draft: Value, folder: &std::path::Path) -> Value {
    let matching_items = draft["agents"].as_array_mut().unwrap();
    for a in matching_items {
        if a["id"] == "builder" {
            a["folder"] = json!(folder.display().to_string());
        }
    }
    draft
}

/// A stale revision is a conflict with what is stored, whatever the edit: it must be
/// reported as `draft_conflict` (load the latest), not as `team_running`.
#[tokio::test]
async fn a_stale_structural_save_on_a_live_team_is_a_draft_conflict() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    let id = team(&router).await;
    let draft = with_builder_folder(draft_json(&router, &id).await, home.path());
    let _starting = st.try_begin_boot(&id).expect("claim is free");

    let (status, body) = call(
        &router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": "p1", "revision": 7, "draft": draft }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("draft_conflict")),
        "{body}"
    );
    let (status, body) = call(
        &router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": "p2", "revision": 0, "draft": draft }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("team_running")),
        "{body}"
    );
}

/// An autosave racing a Start click: either the save is refused as `team_running`, or
/// it lands first and the start runs the saved version. A structural save must never
/// land on a team that started from the version before it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_structural_save_racing_a_start_never_lands_on_the_started_team() {
    let home = tempfile::tempdir().unwrap();
    let (_st, router, _url) = serve(home.path()).await;
    for i in 0..60 {
        let id = ready_team(&router, home.path()).await;
        let draft = with_builder_folder(draft_json(&router, &id).await, home.path());
        let gate = Arc::new(tokio::sync::Barrier::new(2));
        let (r1, g1, i1) = (router.clone(), gate.clone(), id.clone());
        let start = tokio::spawn(async move {
            g1.wait().await;
            call(
                &r1,
                "POST",
                &format!("/api/teams/{i1}/start"),
                json!({ "request_id": format!("s-{i1}") }),
            )
            .await
        });
        let (r2, g2, i2) = (router.clone(), gate, id.clone());
        let save = tokio::spawn(async move {
            g2.wait().await;
            call(
                &r2,
                "PUT",
                &format!("/api/teams/{i2}"),
                json!({ "request_id": format!("p-{i2}"), "revision": 1, "draft": draft }),
            )
            .await
        });
        let ((start_status, start_body), (save_status, save_body)) =
            (start.await.unwrap(), save.await.unwrap());
        assert_eq!(start_status, StatusCode::OK, "{start_body}");
        if save_status == StatusCode::OK {
            let (_, now) = call(&router, "GET", &format!("/api/teams/{id}"), Value::Null).await;
            assert_eq!(
                now["started_revision"], now["draft"]["revision"],
                "iteration {i}: the save landed on the started team: {now}"
            );
        } else {
            assert_eq!(
                (save_status, save_body["error"]["code"].clone()),
                (StatusCode::CONFLICT, json!("team_running")),
                "{save_body}"
            );
        }
    }
}

/// Start claims the boot before it reads the draft, so a save that the claim does not
/// refuse is already stored when start reads it. The order shows in what a start answers
/// while another start holds the claim: `team_running` straight away, not the outcome of
/// reading and checking the draft (`team_invalid` for this team, which has no folders).
#[tokio::test]
async fn start_claims_the_team_before_reading_its_draft() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    let id = team(&router).await;
    let starting = st.try_begin_boot(&id).expect("claim is free");
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("team_running")),
        "{body}"
    );
    drop(starting);
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s2" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::UNPROCESSABLE_ENTITY, json!("team_invalid")),
        "{body}"
    );
    assert!(!st.is_booting(&id), "a refused start releases its claim");
}

// ---- Task 9 review, fix round 1 ----

fn failed_reason(st: &AppState, seat: &str, message: &str) -> Option<String> {
    events(st, seat).into_iter().find_map(|e| match e.body {
        EventBody::MessageFailed { message_id, reason } if message_id == message => Some(reason),
        _ => None,
    })
}

/// Stopping a team ends the wait of every Job not yet sent: the Job is recorded as not
/// sent because the team was stopped, and a quick restart sends it once, not twice.
/// The team's own Stop refuses while the team is starting (`team_starting`), so the stop
/// mid-boot goes through the rig route, which still allows it. The builder is a Codex agent: its
/// launch waits for the real app-server's handshake, so the stop lands while it is still starting.
#[tokio::test]
#[ignore = "live: real codex and claude CLIs; run with --ignored"]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn live_a_stop_before_an_agent_starts_ends_its_job_wait_and_a_restart_sends_the_job_once() {
    let Some(_) = common::live::require() else {
        return;
    };
    let Some(_) = common::live::require_codex() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team_with(&router, home.path(), "codex", OK_JOB).await;
    let (builder, reviewer) = (format!("team.builder@{id}"), format!("team.reviewer@{id}"));
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": "x0" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("team_starting")),
        "{body}"
    );
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/rigs/{id}/stop"),
        json!({ "request_id": "x1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("the first boot to end with nothing running", || {
        !st.is_booting(&id)
            && [&builder, &reviewer]
                .iter()
                .all(|s| !crate::common::lifecycle(&st, s).is_some_and(controld::store::is_live))
    })
    .await;
    assert_eq!(
        crate::common::lifecycle(&st, &reviewer),
        Some(SeatLifecycle::Planned),
        "the reviewer never launched"
    );

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The builder (slow to launch) was still starting at the stop, so its Job waited.
    common::live::wait_for("the builder's job consumed after the restart", || {
        job_messages(&st, &builder)
            .iter()
            .any(|(m, _)| consumed(&st, &builder, m))
    })
    .await;
    common::live::wait_for("the reviewer's handoff consumed after the restart", || {
        handoff_messages(&st, &reviewer)
            .iter()
            .any(|(m, _)| consumed(&st, &reviewer, m))
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let jobs = job_messages(&st, &builder);
    let sent: Vec<_> = jobs
        .iter()
        .filter(|(m, _)| failed_reason(&st, &builder, m).is_none())
        .collect();
    assert_eq!(
        sent.len(),
        1,
        "the builder's Job was sent {} times: {jobs:?}",
        sent.len()
    );
    let reasons: Vec<_> = jobs
        .iter()
        .filter_map(|(m, _)| failed_reason(&st, &builder, m))
        .collect();
    assert_eq!(
        reasons,
        ["not sent: the team was stopped before this agent started"]
    );
    assert_eq!(
        handoff_messages(&st, &reviewer).len(),
        1,
        "one handoff, from the start that ran"
    );
}

/// A start that claims the team after a save's checks passed must read the saved draft,
/// not the one before it: the checks and the write share one store lock. Between them
/// (the seam), a "start" claims the team and reads the draft on another thread; that
/// read must wait for the write.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_start_claiming_between_a_saves_check_and_write_reads_the_saved_draft() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    // The seam is process-wide, so this team gets an id no other test uses.
    let (status, rec) = call(
        &router,
        "POST",
        "/api/teams",
        json!({ "request_id": "create", "template": "builder-reviewer", "name": "Seam team" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let id = rec["draft"]["id"].as_str().unwrap().to_string();
    assert_eq!(id, "seam-team");
    let draft = with_builder_folder(rec["draft"].clone(), home.path());

    let (read_tx, read_rx) = std::sync::mpsc::channel::<u64>();
    let read_rx = Arc::new(std::sync::Mutex::new(read_rx));
    let claims = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (hook_st, hook_id, hook_claims, hook_rx) =
        (st.clone(), id.clone(), claims.clone(), read_rx.clone());
    *controld::seams::BETWEEN_SAVE_CHECK_AND_WRITE
        .lock()
        .unwrap() = Some(Arc::new(move |team: &str| {
        if team != hook_id {
            return;
        }
        hook_claims
            .lock()
            .unwrap()
            .push(hook_st.try_begin_boot(team).expect("claim is free"));
        let (reader, tx, team) = (hook_st.clone(), read_tx.clone(), team.to_string());
        std::thread::spawn(move || {
            let revision = reader
                .store
                .lock()
                .unwrap()
                .draft(&team)
                .unwrap()
                .unwrap()
                .revision;
            tx.send(revision).unwrap();
        });
        // A reader that gets in before the write would report here.
        if let Ok(early) = hook_rx
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_millis(200))
        {
            panic!("a start read revision {early} between the save's check and its write");
        }
    }));

    let (status, body) = call(
        &router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": "p1", "revision": 0, "draft": draft }),
    )
    .await;
    *controld::seams::BETWEEN_SAVE_CHECK_AND_WRITE
        .lock()
        .unwrap() = None;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        claims.lock().unwrap().len(),
        1,
        "the seam ran once, between the check and the write"
    );
    let read = read_rx
        .lock()
        .unwrap()
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the start read the draft");
    assert_eq!(read, 1, "the start read the saved draft");
}

/// A newer start of the same team (a second RigBooted) also ends the wait: the Job it
/// queued is the one to send, so the older Job is recorded as not sent. The builder is a Codex agent,
/// still starting (the real app-server's handshake) when the newer start lands.
#[tokio::test]
#[ignore = "live: real codex and claude CLIs; run with --ignored"]
async fn live_a_newer_start_ends_the_job_wait_of_the_older_one() {
    let Some(_) = common::live::require() else {
        return;
    };
    let Some(_) = common::live::require_codex() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team_with(&router, home.path(), "codex", OK_JOB).await;
    let builder = format!("team.builder@{id}");
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    st.local_event(
        Actor::System("test".into()),
        model::Subject::Rig(id.clone()),
        model::Provenance::Operator,
        EventBody::RigBooted {
            spec_hash: "newer".into(),
            seat_count: 2,
        },
    )
    .unwrap();
    common::live::wait_for("the older Job recorded as not sent", || {
        job_messages(&st, &builder).iter().any(|(m, _)| {
            failed_reason(&st, &builder, m).as_deref()
                == Some("not sent: the team was started again before this agent started")
        })
    })
    .await;
    common::live::wait_for("the boot finished", || !st.is_booting(&id)).await;
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": "x1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

// ---- Backend hardening: stop while a start is in flight ----

fn rig_desired_changes(st: &AppState, id: &str) -> usize {
    st.store
        .lock()
        .unwrap()
        .events_for_subject(&model::Subject::Rig(id.to_string()), 0, 5000)
        .unwrap()
        .iter()
        .filter(|e| matches!(e.body, EventBody::RigDesiredChanged { .. }))
        .count()
}

/// A stop while the team is still starting would interleave with that boot (and with a
/// restart's Job delivery), so it is refused with 409 `team_starting` and records nothing.
/// The held claim stands for a boot in flight, so the test does not depend on timing.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stopping_a_team_that_is_still_starting_is_refused_and_records_nothing() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team(&router, home.path()).await;
    let builder = format!("team.builder@{id}");
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("the boot finished", || !st.is_booting(&id)).await;
    common::live::wait_for("builder running", || {
        crate::common::lifecycle(&st, &builder) == Some(SeatLifecycle::Running)
    })
    .await;

    let starting = st.try_begin_boot(&id).expect("claim is free");
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": "x1" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("team_starting")),
        "{body}"
    );
    assert_eq!(
        rig_desired_changes(&st, &id),
        0,
        "a refused stop records nothing"
    );
    assert_eq!(
        st.store
            .lock()
            .unwrap()
            .rig_snapshot(&id)
            .unwrap()
            .unwrap()
            .desired,
        model::Desired::Running
    );
    assert_eq!(
        crate::common::lifecycle(&st, &builder),
        Some(SeatLifecycle::Running)
    );
    drop(starting);

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({ "request_id": "x2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!st.is_booting(&id), "a stop releases what it holds");
    common::live::wait_for("stopped", || {
        crate::common::lifecycle(&st, &builder) == Some(SeatLifecycle::Stopped)
    })
    .await;
}

// ---- Backend hardening: import parses without the store lock ----

/// Parsing and compiling an imported spec can take a while for a large file, and every
/// other request needs the store, so import parses first and locks only to insert.
#[tokio::test]
async fn import_parses_and_compiles_without_holding_the_store_lock() {
    let home = tempfile::tempdir().unwrap();
    let (st, router, _url) = serve(home.path()).await;
    let lock_free = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (hook_st, seen) = (st.clone(), lock_free.clone());
    // The seam is process-wide, so the hook only records this test's spec.
    *controld::seams::AFTER_IMPORT_PARSE.lock().unwrap() = Some(Arc::new(move |name: &str| {
        if name == "lock-probe" {
            seen.lock().unwrap().push(hook_st.store.try_lock().is_ok());
        }
    }));
    let yaml = format!(
        "version: \"0.2\"\nname: lock-probe\npods:\n  - id: p\n    label: P\n    members:\n      - {{ id: a, agent_ref: \"local:a\", profile: default, runtime: claude-code, cwd: \"{}\" }}\n",
        home.path().display()
    );
    let (status, body) = call(
        &router,
        "POST",
        "/api/teams/import",
        json!({ "request_id": "i1", "yaml": yaml }),
    )
    .await;
    *controld::seams::AFTER_IMPORT_PARSE.lock().unwrap() = None;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["team"]["draft"]["id"], "lock-probe");
    assert_eq!(
        *lock_free.lock().unwrap(),
        [true],
        "the store was free while the spec was parsed"
    );
}

// ---- Backend hardening item 8: a restart of Sculptured's host ----

/// Sculptured's agent runner with the real CLIs (`common::live::drivers`).
fn spawn_hostd(url: String) -> tokio::task::JoinHandle<()> {
    let sup = Supervisor::new(
        "local".into(),
        common::live::drivers(),
        Journal::open_in_memory(10_000).unwrap(),
    );
    tokio::spawn(async move {
        let _ = hostd::link::run_link(&url, TOKEN, &sup).await;
    })
}

/// hostd has no graceful shutdown and the agents' CLIs are its children, so a restart (a
/// reboot, or stopping and starting `sculptured serve`) ends every agent. The spec
/// relaunches nothing without an operator decision (restore-on-boot is M2), so each
/// agent needs attention, with a plain reason that says what happened and what to do.
/// Start is that decision: it relaunches the team and sends each Job again.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn live_after_a_host_restart_agents_say_why_and_start_runs_the_team_again() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    let hostd = spawn_hostd(url.clone());
    crate::common::wait_for("host connected", || {
        st.hosts.has_driver("local", "claude-stream")
    })
    .await;
    let id = ready_team(&router, home.path()).await;
    let seats = [format!("team.builder@{id}"), format!("team.reviewer@{id}")];
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The builder's first message is its Job; the reviewer's is the builder's handoff.
    let first = |s: &String| {
        if s.starts_with("team.reviewer@") {
            handoff_messages(&st, s)
        } else {
            job_messages(&st, s)
        }
    };
    common::live::wait_for("both first messages consumed", || {
        seats
            .iter()
            .all(|s| first(s).first().is_some_and(|(m, _)| consumed(&st, s, m)))
    })
    .await;
    common::live::wait_for("the boot finished", || !st.is_booting(&id)).await;

    // hostd goes away with its agents, and a fresh one (nothing running) comes back.
    hostd.abort();
    common::live::wait_for("agents stale", || {
        seats
            .iter()
            .all(|s| crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Stale))
    })
    .await;
    let _hostd2 = spawn_hostd(url);
    common::live::wait_for("agents need attention", || {
        seats
            .iter()
            .all(|s| crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Attention))
    })
    .await;
    for s in &seats {
        let reason = st
            .store
            .lock()
            .unwrap()
            .seat(s)
            .unwrap()
            .unwrap()
            .lifecycle_reason;
        assert_eq!(
            reason.as_deref(),
            Some(
                "This agent's process ended when Sculptured restarted. Start the team again to relaunch it."
            ),
            "{s}"
        );
    }
    let (_, rec) = call(&router, "GET", &format!("/api/teams/{id}"), Value::Null).await;
    assert_eq!(
        rec["live"], false,
        "nothing runs, so the team can be started: {rec}"
    );

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s2" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for(
        "both agents run again with their first message sent again",
        || {
            seats.iter().all(|s| {
                let firsts = first(s);
                crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Running)
                    && firsts.len() == 2
                    && consumed(&st, s, &firsts[1].0)
            })
        },
    )
    .await;
    // Settle, then check again: each was sent exactly once more, not twice.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    for s in &seats {
        assert_eq!(first(s).len(), 2, "{s}: one first message per start");
    }
    assert!(
        job_messages(&st, &seats[1]).is_empty(),
        "the reviewer never gets its Job at start"
    );
}

#[tokio::test]
async fn a_spec_file_cannot_take_a_drafted_teams_id_and_the_team_still_starts() {
    let home = tempfile::tempdir().unwrap();
    let (_st, router, _url) = serve(home.path()).await;
    let id = ready_team(&router, home.path()).await;
    let yaml = format!(
        "version: \"0.2\"\nname: {id}\npods:\n  - id: dev\n    label: Dev\n    members:\n      - {{ id: a, agent_ref: \"local:a\", profile: default, runtime: claude-code, cwd: \"{}\" }}\n",
        home.path().display()
    );
    let spec = home.path().join("taker.yaml");
    std::fs::write(&spec, yaml).unwrap();
    let (status, body) = call(
        &router,
        "POST",
        "/api/rigs",
        json!({ "request_id": "file-1", "spec_path": spec.display().to_string() }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "rig_name_taken");
    let (status, _) = call(&router, "GET", &format!("/api/rigs/{id}"), Value::Null).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the refused file left no rig behind"
    );

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1", "revision": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["rig"], id.as_str());
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stop_reconciles_a_live_projection_when_the_runner_has_no_process() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    connect_host(&st, url).await;
    let id = ready_team(&router, home.path()).await;
    let builder = format!("team.builder@{id}");
    call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({"request_id":"start"}),
    )
    .await;
    common::live::wait_for("boot finished", || {
        !st.is_booting(&id)
            && crate::common::lifecycle(&st, &builder) == Some(SeatLifecycle::Running)
    })
    .await;
    call(
        &router,
        "POST",
        &format!("/api/teams/{id}/stop"),
        json!({"request_id":"stop"}),
    )
    .await;
    common::live::wait_for("stopped", || {
        crate::common::lifecycle(&st, &builder) == Some(SeatLifecycle::Stopped)
    })
    .await;
    for state in [
        SeatLifecycle::Running,
        SeatLifecycle::Launching,
        SeatLifecycle::Stopping,
        SeatLifecycle::Stale,
        SeatLifecycle::Attention,
    ] {
        st.local_event(
            Actor::System("test".into()),
            model::Subject::Seat(builder.clone()),
            model::Provenance::Heuristic,
            EventBody::SeatLifecycleChanged {
                lifecycle: state,
                reason: Some("outdated projection".into()),
            },
        )
        .unwrap();
        let request = json!({"request_id":format!("stop-{state:?}")});
        let (status, body) = call(&router, "POST", &format!("/api/teams/{id}/stop"), request).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["failed"], json!([]));
        assert_eq!(
            crate::common::lifecycle(&st, &builder),
            Some(SeatLifecycle::Stopped)
        );
        let (_, record) = call(&router, "GET", &format!("/api/teams/{id}"), Value::Null).await;
        assert_eq!(record["live"], false);
    }
}

/// Decision 1 (restore): after a host restart a started team asks once, and Resume continues each
/// agent's own session (the same native session id) instead of starting over.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_after_a_host_restart_the_team_resumes_each_agents_own_session() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    let hostd = spawn_hostd(url.clone());
    crate::common::wait_for("host connected", || {
        st.hosts.has_driver("local", "claude-stream")
    })
    .await;
    let id = ready_team(&router, home.path()).await;
    let seats = [format!("team.builder@{id}"), format!("team.reviewer@{id}")];
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let view = |s: &String| st.store.lock().unwrap().seat(s).unwrap().unwrap();
    common::live::wait_for("both run with a session", || {
        seats.iter().all(|s| {
            crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Running)
                && view(s).native_session_id.is_some()
        })
    })
    .await;
    common::live::wait_for("the boot finished", || !st.is_booting(&id)).await;
    let sessions: Vec<_> = seats
        .iter()
        .map(|s| view(s).native_session_id.expect("session id"))
        .collect();

    hostd.abort();
    common::live::wait_for("agents stale", || {
        seats
            .iter()
            .all(|s| crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Stale))
    })
    .await;
    let _hostd2 = spawn_hostd(url);
    common::live::wait_for("offers", || {
        seats
            .iter()
            .all(|s| view(s).restore_offer.is_some_and(|o| o.can_resume))
    })
    .await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/restore"),
        json!({ "request_id": "r1", "decision": "resume" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["restored"].as_array().map(Vec::len), Some(2), "{body}");
    let paired_seats = seats.iter().zip(&sessions);
    for (s, before) in paired_seats {
        let v = view(s);
        assert_eq!(v.lifecycle, SeatLifecycle::Running, "{s}");
        assert_eq!(
            v.native_session_id.as_ref(),
            Some(before),
            "{s} resumed its own session"
        );
        assert!(v.restore_offer.is_none(), "{s}");
    }
}

/// Each agent's "When it restarts" setting shapes the team's one Resume answer: an agent set to start
/// fresh starts a new session, and one set to restart from a checkpoint (Sculptured has none) keeps
/// waiting for its own decision.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_the_teams_resume_answer_follows_each_agents_restart_setting() {
    let Some(_) = common::live::require() else {
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let (st, router, url) = serve(home.path()).await;
    let hostd = spawn_hostd(url.clone());
    crate::common::wait_for("host connected", || {
        st.hosts.has_driver("local", "claude-stream")
    })
    .await;
    let id = ready_team(&router, home.path()).await;
    let (_, rec) = call(&router, "GET", &format!("/api/teams/{id}"), Value::Null).await;
    let mut draft = rec["draft"].clone();
    let matching_items = draft["agents"].as_array_mut().unwrap();
    for a in matching_items {
        a["advanced"]["resume"] = json!(if a["id"] == "builder" {
            "checkpoint-only"
        } else {
            "relaunch-fresh"
        });
    }
    let (status, body) = call(
        &router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": "p1", "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (builder, reviewer) = (format!("team.builder@{id}"), format!("team.reviewer@{id}"));
    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": "s1" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("both run", || {
        [&builder, &reviewer]
            .iter()
            .all(|s| crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Running))
    })
    .await;
    common::live::wait_for("the boot finished", || !st.is_booting(&id)).await;
    let view = |s: &String| st.store.lock().unwrap().seat(s).unwrap().unwrap();

    hostd.abort();
    common::live::wait_for("agents stale", || {
        [&builder, &reviewer]
            .iter()
            .all(|s| crate::common::lifecycle(&st, s) == Some(SeatLifecycle::Stale))
    })
    .await;
    let _hostd2 = spawn_hostd(url);
    common::live::wait_for("offers", || {
        [&builder, &reviewer]
            .iter()
            .all(|s| view(s).restore_offer.is_some_and(|o| o.can_resume))
    })
    .await;

    let (status, body) = call(
        &router,
        "POST",
        &format!("/api/teams/{id}/restore"),
        json!({ "request_id": "r1", "decision": "resume" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["restored"], json!([reviewer]), "{body}");
    assert_eq!(body["still_waiting"], json!([builder]), "{body}");
    let causes: Vec<_> = events(&st, &reviewer)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::TenureStarted { cause, .. } => Some(cause),
            _ => None,
        })
        .collect();
    assert_eq!(
        causes.last(),
        Some(&model::TenureCause::Fresh),
        "{causes:?}"
    );
    assert!(
        view(&builder).restore_offer.is_some(),
        "the builder still waits for its own decision"
    );
}
