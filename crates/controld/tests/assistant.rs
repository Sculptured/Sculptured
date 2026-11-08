//! The design assistant: the person's own CLI changes the design as MADL; the answer comes back as a
//! proposal and the saved design is untouched until approved. Asking the CLI is tested live, on the
//! real Claude Code CLI with its cheapest model; refusals before any CLI call are plain tests.
mod common;
use axum::http::StatusCode;

use common::live::CLAUDE_MODEL;
use common::m2::{Served, rid};
use serde_json::{Value, json};
use std::collections::HashMap;

/// A served controld whose assistant runs the real `claude`.
async fn served(claude: std::path::PathBuf) -> Served {
    let mut s = crate::common::m2::serve().await;
    s.st = s.st.clone().with_cli_programs(Some(claude), None);
    s.router = controld::app(s.st.clone(), None);
    s
}

fn opts() -> Value {
    json!({ "cli": "claude", "model": CLAUDE_MODEL })
}

const ADD_TESTER: &str = "Add a tester agent after Beta that runs the tests and reports failures. Keep Alpha and Beta as they are.";

const WITH_TESTER: &str = "madl: draft
system: { id: whatever, name: Pair }
agents:
  - { id: a, name: Alpha, runtime: claude-code, instructions: Plan the work in numbered steps. }
  - { id: b, name: Beta, runtime: claude-code, instructions: build }
  - { id: tester, name: Tester, runtime: claude-code, instructions: Run the tests and report failures. }
connections:
  - { id: c-a-b, from: a, to: b, kind: handoff }
  - { from: b, to: tester, kind: handoff }
";

async fn pair(s: &Served) -> String {
    crate::common::m2::chain(
        s,
        "Pair",
        &[("a", "Alpha", "plan"), ("b", "Beta", "build")],
        &[("a", "b")],
    )
    .await
}

async fn ask(s: &Served, team: &str, body: Value) -> Value {
    let (status, got) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/assistant"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    got
}

/// Each agent's board position in `draft`, by id.
fn places(draft: &Value) -> HashMap<String, (Value, Value)> {
    draft["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let id = a["id"].as_str().unwrap().to_string();
            (id, (a["x"].clone(), a["y"].clone()))
        })
        .collect()
}

/// Every agent that is in both drafts sits in the same place in each.
fn same_places(before: &Value, after: &Value) {
    let was = places(before);
    for (id, place) in places(after) {
        if let Some(old) = was.get(&id) {
            assert_eq!(&place, old, "{id} keeps its place");
        }
    }
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_proposal_changes_nothing_saved_and_keeps_cards_where_they_were() {
    let Some(claude) = common::live::require() else {
        return;
    };
    let s = served(claude).await;
    let team = pair(&s).await;
    let (_, before) = crate::common::m2::get(&s.router, &format!("/api/teams/{team}")).await;
    let p = ask(
        &s,
        &team,
        json!({ "options": opts(), "messages": [{ "role": "person", "text": ADD_TESTER }] }),
    )
    .await;
    assert!(
        !p["draft"]["agents"].as_array().unwrap().is_empty(),
        "a valid design came back: {p}"
    );
    same_places(&before["draft"], &p["draft"]);
    assert_eq!(
        p["draft"]["id"],
        json!(team),
        "the saved team's id, whatever the CLI named it"
    );
    assert!(
        p["summary"].is_string() && p["madl"].as_str().is_some_and(|m| m.contains("agents")),
        "{p}"
    );
    assert!(p["dropped"].is_array() && p["issues"].is_array(), "{p}");
    let (_, after) = crate::common::m2::get(&s.router, &format!("/api/teams/{team}")).await;
    assert_eq!(
        after["draft"], before["draft"],
        "nothing is saved until the person approves"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_iterating_starts_from_the_proposal_and_keeps_its_cards_in_place() {
    let Some(claude) = common::live::require() else {
        return;
    };
    let s = served(claude).await;
    let team = pair(&s).await;
    let first = ask(
        &s,
        &team,
        json!({ "options": opts(), "messages": [{ "role": "person", "text": ADD_TESTER }] }),
    )
    .await;
    let messages = json!([
        { "role": "person", "text": ADD_TESTER },
        { "role": "assistant", "text": first["summary"] },
        { "role": "person", "text": "Rename the tester to QA. Change nothing else." },
    ]);
    let second = ask(
        &s,
        &team,
        json!({ "options": opts(), "messages": messages, "proposal": first["madl"] }),
    )
    .await;
    assert!(
        !second["draft"]["agents"].as_array().unwrap().is_empty(),
        "{second}"
    );
    same_places(&first["draft"], &second["draft"]);
    assert_eq!(second["draft"]["id"], json!(team));
}

#[tokio::test]
async fn an_invalid_proposal_is_refused_with_what_was_wrong() {
    let s = crate::common::m2::serve().await;
    let team = pair(&s).await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/assistant"),
        json!({ "options": opts(), "messages": [{ "role": "person", "text": "Call it QA" }],
                "proposal": WITH_TESTER.replace("to: tester", "to: nobody") }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "proposal_invalid", "{body}");
    assert!(
        body.to_string().contains("unknown-reference"),
        "the reason is said: {body}"
    );
}

#[tokio::test]
async fn asking_needs_a_request() {
    let s = crate::common::m2::serve().await;
    let team = crate::common::m2::chain(&s, "Pair", &[("a", "Alpha", "plan")], &[]).await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/assistant"),
        json!({ "options": opts(), "messages": [] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_proposal_keeps_the_saved_budget_and_agent_limits() {
    let Some(claude) = common::live::require() else {
        return;
    };
    let s = served(claude).await;
    let team = pair(&s).await;
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{team}")).await;
    let mut draft = rec["draft"].clone();
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 30.0 }], "max_working": 3 });
    draft["budget"] = budget.clone();
    draft["agents"][0]["advanced"]["turn_limit"] = json!({ "hard": 12 });
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{team}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let p = ask(
        &s,
        &team,
        json!({ "options": opts(), "messages": [{ "role": "person", "text": ADD_TESTER }] }),
    )
    .await;
    assert_eq!(
        p["draft"]["budget"], budget,
        "the run budget stays as saved, whatever the proposal says"
    );
    let alpha = p["draft"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "a")
        .unwrap_or_else(|| panic!("Alpha was kept: {p}"));
    assert_eq!(alpha["advanced"]["turn_limit"], json!({ "hard": 12 }));
}
