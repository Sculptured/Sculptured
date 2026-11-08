//! Knowledge: the project map made at a start and carried in the agents' first messages, lessons agents
//! record, a search of the team's history, and the Knowledge view's data. What agents are sent is
//! checked live, on the real Claude Code CLI with its cheapest model; the map and kept lessons are
//! checked on a team started with no computer connected.
mod common;
use axum::http::StatusCode;

use common::m2::{Served, rid};
use serde_json::{Value, json};

/// Saves `agents` as team "Know" (Claude agents on haiku); returns its id. Not started.
async fn know_team(s: &Served, agents: Value) -> String {
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Know" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = agents;
    common::live::make_live(s, &mut draft);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

/// A folder with a two-file `messaging` module, and a Builder working there whose shared values
/// are one present and one missing; team "Know" with the lesson "Reuse the message bus…".
async fn mapped_team(s: &Served, job: &str) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    std::fs::create_dir_all(dir.path().join("messaging")).unwrap();
    std::fs::write(
        dir.path().join("messaging/README.md"),
        "# Messaging primitives\n\n## Slack bridge\nsends to Slack\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("messaging/bus.rs"), "fn send() {}").unwrap();
    let agents = json!([{ "id": "a", "group_id": null, "name": "Builder", "kind": "claude", "job": job, "folder": dir.path().display().to_string(), "x": 40, "y": 40,
        "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "data_access": { "shared": [
            { "name": "send", "value": "fn send()", "file": "messaging/bus.rs" }, { "name": "port", "value": "8080", "file": "messaging/bus.rs" } ] } } }]);
    let id = know_team(s, agents).await;
    s.st.store
        .lock()
        .unwrap()
        .add_decision(
            &id,
            "Builder",
            "Reuse the message bus; do not build a second one.",
            "lesson",
        )
        .unwrap();
    (id, dir)
}

fn sent(s: &Served, who: &str) -> Vec<String> {
    crate::common::m2::events(&s.st, who)
        .into_iter()
        .filter_map(|e| match e.body {
            model::EventBody::MessageQueued { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

async fn send(s: &Served, who: &str, text: &str) {
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{who}/send"),
        json!({ "request_id": rid(), "text": text, "intent": "inform" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_agents_get_the_map_lessons_and_a_history_search() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let (id, _dir) = mapped_team(
        &s,
        "Reply with exactly the word PINEAPPLE and nothing else.",
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("the job is sent", || !sent(&s, &a).is_empty()).await;
    let text = sent(&s, &a).remove(0);
    assert!(
        text.contains("Project map, generated at this start")
            && text.contains("- messaging (2 files")
            && text.contains("Messaging primitives"),
        "{text}"
    );
    assert!(
        text.contains("Reuse the message bus; do not build a second one. (Builder)")
            && text.contains("search_history"),
        "{text}"
    );
    assert!(
        text.contains("- send = fn send() (messaging/bus.rs line 1)")
            && text.contains("NOT FOUND: port = 8080 should be in messaging/bus.rs"),
        "shared values are checked in their files: {text}"
    );
    // The first turn is in the history the team can search, once that turn is past.
    common::live::wait_turns(&s.st, &a, 1).await;
    send(&s, &a, "Reply with the single word OK and nothing else.").await;
    common::live::wait_for("the next message is queued", || sent(&s, &a).len() == 2).await;
    let found =
        controld::knowledge::search_history(&s.st, &a, &json!({ "query": "pineapple" })).unwrap();
    assert!(
        found["text"]
            .as_str()
            .unwrap()
            .contains("\"agent\":\"Builder\""),
        "{found}"
    );
    crate::common::m2::stop_team(&s, &id).await;
}

/// Starts team `id` with no computer connected: the start maps its folders, no agent launches.
async fn start_without_computer(s: &Served, id: &str) {
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::wait_for("boot finished", || !s.st.is_booting(id)).await;
}

async fn knowledge_view(s: &Served, id: &str) -> Value {
    let (status, view) = crate::common::m2::call(
        &s.router,
        "GET",
        &format!("/api/teams/{id}/knowledge"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    view
}

#[tokio::test]
async fn a_start_maps_the_folder_and_a_confirmed_lesson_is_kept_with_the_project() {
    let s = crate::common::m2::serve().await;
    let (id, dir) = mapped_team(&s, "hello").await;
    start_without_computer(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    crate::common::wait_for("the full map is in the folder", || {
        dir.path().join(".sculptured/map.md").exists()
    })
    .await;
    let mut view = knowledge_view(&s, &id).await;
    for _ in 0..250 {
        if view["maps"][0]["map"]["files"].is_number() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        view = knowledge_view(&s, &id).await;
    }
    assert_eq!(view["maps"][0]["map"]["files"], 2, "{view}");
    controld::knowledge::record_lesson(&s.st, &a, &json!({ "lesson": "Check the map first." }))
        .unwrap();
    let lesson = knowledge_view(&s, &id).await["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["text"] == "Check the map first.")
        .expect("the new lesson")
        .clone();
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!(
            "/api/teams/{id}/decisions/{}/confirm",
            lesson["id"].as_str().unwrap()
        ),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        knowledge_view(&s, &id).await["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["text"] == "Check the map first." && e["confirmed_ms"].is_number())
    );
    // A confirmed lesson is kept with the project, for later teams in this folder.
    let kept = dir.path().join(".sculptured/lessons.md");
    crate::common::wait_for("the lesson is kept with the project", || {
        std::fs::read_to_string(&kept).is_ok_and(|t| t.contains("- Check the map first."))
    })
    .await;
    assert!(
        localops::map::build(dir.path())
            .unwrap()
            .kept
            .contains(&"Check the map first.".to_string())
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_lesson_recorded_mid_run_reaches_running_teammates_with_their_next_message() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let folder = dir.path().display().to_string();
    let ok = "Reply with the single word OK and nothing else.";
    let id = know_team(
        &s,
        json!([
            { "id": "a", "group_id": null, "name": "Scout", "kind": "claude", "job": ok, "folder": folder, "x": 40, "y": 40 },
            { "id": "b", "group_id": null, "name": "Builder", "kind": "claude", "job": ok, "folder": folder, "x": 300, "y": 40 },
        ]),
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    common::live::wait_turns(&s.st, &a, 1).await;
    common::live::wait_turns(&s.st, &b, 1).await;
    controld::knowledge::record_lesson(
        &s.st,
        &a,
        &json!({ "lesson": "Money helpers live in money.py." }),
    )
    .unwrap();
    controld::knowledge::record_lesson(
        &s.st,
        &a,
        &json!({ "lesson": "Never use float for money.", "kind": "constraint" }),
    )
    .unwrap();
    assert!(
        controld::knowledge::record_lesson(&s.st, &a, &json!({ "lesson": "x", "kind": "rule" }))
            .is_err()
    );
    let next = "STEP-TWO: reply with the single word OK and nothing else.";
    for who in [&a, &b] {
        send(&s, who, next).await;
        common::live::wait_for("the second message is queued", || sent(&s, who).len() == 2).await;
    }
    let to_b = sent(&s, &b).pop().unwrap();
    assert!(
        to_b.contains("New lessons from your team since your last message")
            && to_b.contains("Money helpers live in money.py. (Scout)")
            && to_b.contains("[constraint] Never use float for money. (Scout)")
            && to_b.ends_with(next),
        "{to_b}"
    );
    assert!(
        !to_b.contains("Project map, generated"),
        "the briefing itself is not repeated: {to_b}"
    );
    assert_eq!(
        sent(&s, &a).pop().unwrap(),
        next,
        "an agent is not told its own lesson back"
    );
    // A search leaves out the asking agent's current turn: its message and its own calls.
    let found =
        controld::knowledge::search_history(&s.st, &b, &json!({ "query": "step-two" })).unwrap();
    let text = found["text"].as_str().unwrap();
    assert!(
        text.contains("\"agent\":\"Scout\"") && !text.contains("\"agent\":\"Builder\""),
        "{text}"
    );
    crate::common::m2::stop_team(&s, &id).await;
}
