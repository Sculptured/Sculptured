//! Generating teams with the user's own CLI: questions, alternatives as laid-out team files, retry.
//! How a sketch becomes a team draft is tested on literal sketches; asking the CLI is tested live,
//! on the real Claude Code CLI with its cheapest model.
mod common;
use axum::http::StatusCode;

use common::live::CLAUDE_MODEL;
use common::m2::Served;
use controld::generate::sketch::{self, Kinds, Sketch};
use serde_json::{Value, json};
use std::path::PathBuf;

/// A served controld whose generate calls run `claude`.
async fn served_with(claude: PathBuf) -> Served {
    let mut s = crate::common::m2::serve().await;
    s.st = s.st.clone().with_cli_programs(Some(claude), None);
    s.router = controld::app(s.st.clone(), None);
    s
}

fn sketch(name: &str, agents: &[&str], arrows: &[(&str, &str)]) -> Value {
    json!({
        "name": name,
        "groups": [],
        "agents": agents.iter().map(|a| json!({ "name": a, "kind": "claude", "group": null, "job": format!("Do {a}'s part"), "check": null })).collect::<Vec<_>>(),
        "arrows": arrows.iter().map(|(f, t)| json!({ "from": f, "to": t, "kind": "hands-work-to" })).collect::<Vec<_>>(),
    })
}

/// The sketch as a team draft (JSON) and what was dropped, as Sculptured converts the CLI's answer.
fn convert(v: Value, kinds: Kinds, folder: Option<&str>) -> (Value, Vec<String>) {
    let s: Sketch = serde_json::from_value(v).unwrap();
    let c = sketch::to_draft(&s, kinds, folder);
    (serde_json::to_value(&c.team).unwrap(), c.dropped)
}

fn opts(extra: Value) -> Value {
    let mut o = json!({ "cli": "claude", "model": CLAUDE_MODEL, "size": null, "kinds": null, "shape": null, "checks": false, "folder": null, "host": null, "team": null });
    let matching_extra = extra.as_object().unwrap();
    for (k, v) in matching_extra {
        o[k] = v.clone();
    }
    o
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_questions_come_back_as_up_to_four_with_suggestions() {
    let Some(claude) = common::live::require() else {
        return;
    };
    let s = served_with(claude).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/questions",
        json!({ "options": opts(json!({})), "prompt": "Rebuild our docs site" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let questions = v["questions"].as_array().unwrap();
    assert!((1..=4).contains(&questions.len()), "{v}");
    assert!(
        questions.iter().all(|q| {
            !q["question"].as_str().unwrap().trim().is_empty()
                && q["suggestions"].as_array().unwrap().len() <= 4
        }),
        "{v}"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_alternatives_come_back_as_laid_out_team_drafts_with_issues() {
    let Some(claude) = common::live::require() else {
        return;
    };
    let s = served_with(claude).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({ "size": "2-3" })), "prompt": "Write and proofread a one-page README", "answers": [], "count": 3 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let alts = v["alternatives"].as_array().unwrap();
    assert!((1..=3).contains(&alts.len()), "{v}");
    for a in alts {
        let agents = a["draft"]["agents"].as_array().unwrap();
        assert!(!agents.is_empty() && a["issues"].is_array(), "{a}");
        let ids: std::collections::HashSet<_> = agents.iter().map(|x| &x["id"]).collect();
        assert_eq!(ids.len(), agents.len(), "every agent has its own id: {a}");
        assert!(
            agents
                .iter()
                .all(|x| x["x"].is_i64() && x["y"].is_i64() && x["job"].is_string()),
            "laid out, each with its job: {a}"
        );
    }
}

#[test]
fn a_chain_is_laid_out_along_its_arrows_and_unknown_agents_are_dropped() {
    let balanced = sketch(
        "Docs",
        &["Planner", "Writer", "Editor"],
        &[
            ("Planner", "Writer"),
            ("Writer", "Editor"),
            ("Editor", "Ghost"),
        ],
    );
    let (draft, dropped) = convert(balanced, Kinds::AsSketched, None);
    let x = |name: &str| {
        draft["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap()["x"]
            .as_i64()
            .unwrap()
    };
    assert!(
        x("Planner") < x("Writer") && x("Writer") < x("Editor"),
        "laid out along the chain"
    );
    assert!(dropped.iter().any(|d| d.contains("Ghost")), "{dropped:?}");
    assert_eq!(draft["connections"].as_array().unwrap().len(), 2);
}

#[test]
fn codex_option_makes_every_agent_codex_and_the_folder_is_set() {
    let folder = "/home/someone/docs";
    let (team, _) = convert(
        sketch("Docs", &["Writer", "Editor"], &[("Writer", "Editor")]),
        Kinds::from_option(Some("codex")),
        Some(folder),
    );
    assert!(
        team["agents"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["kind"] == "codex")
    );
    assert_eq!(team["folder"], json!(folder));
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_failing_cli_is_retried_then_reported_with_its_raw_text() {
    let Some(claude) = common::live::require() else {
        return;
    };
    let s = served_with(claude).await;
    // A model the CLI does not know: the real CLI fails both times, and Sculptured says so.
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({ "model": "no-such-model-xyz" })), "prompt": "docs", "answers": [], "count": 1 }),
    )
    .await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (StatusCode::BAD_GATEWAY, Some("generate_unusable")),
        "{v}"
    );
    assert!(
        v.to_string().contains("could not be used"),
        "the reason is shown: {v}"
    );
}

#[test]
fn duplicate_agent_names_get_unique_ids() {
    let (draft, _) = convert(
        sketch("Docs", &["Writer", "Writer"], &[]),
        Kinds::AsSketched,
        None,
    );
    let agents = draft["agents"].as_array().unwrap();
    assert_ne!(agents[0]["id"], agents[1]["id"]);
}

#[test]
fn two_arrows_between_the_same_agents_keep_distinct_ids() {
    let mut t = sketch("Docs", &["A", "B", "C", "A-B"], &[("A", "B")]);
    t["arrows"] = json!([
        { "from": "A", "to": "B", "kind": "hands-work-to" },
        { "from": "A", "to": "B", "kind": "watches" },
        { "from": "A-B", "to": "C", "kind": "hands-work-to" },
        { "from": "A", "to": "C", "kind": "hands-work-to" },
    ]);
    let (draft, _) = convert(t, Kinds::AsSketched, None);
    let ids: Vec<String> = draft["connections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 4, "{ids:?}");
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        4,
        "every arrow has its own id: {ids:?}"
    );
}

#[test]
fn agents_sharing_a_name_are_renamed_and_names_match_regardless_of_case() {
    let mut t = sketch("Docs", &["Writer", "Writer", "Editor"], &[]);
    t["arrows"] = json!([{ "from": "writer", "to": " Editor ", "kind": "hands-work-to" }]);
    let (draft, dropped) = convert(t, Kinds::AsSketched, None);
    let names: Vec<&str> = draft["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["Writer", "Writer 2", "Editor"],
        "the second is renamed so arrows and people can tell them apart"
    );
    assert!(
        dropped.iter().any(|d| d.contains("Writer 2")),
        "and that is said: {dropped:?}"
    );
    assert_eq!(
        draft["connections"].as_array().unwrap().len(),
        1,
        "\"writer\" and \" Editor \" still match"
    );
}

#[test]
fn the_schemas_leave_counts_to_the_server() {
    for schema in [
        sketch::teams_schema(3),
        sketch::teams_schema(1),
        sketch::questions_schema(),
    ] {
        let text = schema.to_string();
        assert!(
            !text.contains("minItems") && !text.contains("maxItems"),
            "structured-output modes may refuse item limits: {text}"
        );
    }
}

fn session_files() -> Value {
    let dir = format!(
        "{}/tests/fixtures/sessions/claude",
        env!("CARGO_MANIFEST_DIR")
    );
    let file = |p: &str| json!({ "name": p.rsplit('/').next().unwrap(), "text": std::fs::read_to_string(format!("{dir}/{p}")).unwrap() });
    json!({ "files": [file("main-with-agents.jsonl"), file("subagents/agent-a1.jsonl"), file("subagents/agent-a1.meta.json")] })
}

/// A team imported from the fixture session: (team id, its imported run's start seq).
async fn imported(s: &Served) -> (String, i64) {
    let (status, c) =
        crate::common::m2::call(&s.router, "POST", "/api/runs/convert", session_files()).await;
    assert_eq!(status, StatusCode::OK, "{c}");
    import_text(s, c["session"].as_str().unwrap()).await
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_team_rebuilt_from_a_session_is_improved_into_a_laid_out_team() {
    let Some(claude) = common::live::require() else {
        return;
    };
    let s = served_with(claude).await;
    let (team, seq) = imported(&s).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/improve-from-run"),
        json!({ "options": opts(json!({})), "start_seq": seq }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(
        !v["draft"]["agents"].as_array().unwrap().is_empty(),
        "a team with agents is offered: {v}"
    );
    assert!(
        v["rationale"]
            .as_str()
            .is_some_and(|r| !r.trim().is_empty())
            && v["issues"].is_array(),
        "{v}"
    );
}

#[tokio::test]
async fn only_an_imported_run_can_improve_a_team() {
    let s = crate::common::m2::serve().await;
    let (team, _) = imported(&s).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/improve-from-run"),
        json!({ "options": opts(json!({})), "start_seq": 5 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "not_imported");
}

/// A large session's improve prompt is built and passes the CLI's 32 KB limit. The CLI program
/// here does not exist, so the call stops right after that check, at starting the program.
#[tokio::test]
async fn a_large_session_still_fits_the_improve_prompt() {
    let nowhere = tempfile::tempdir().unwrap();
    let s = served_with(nowhere.path().join("no-claude-here")).await;
    // 40 subagents, each given a 5 KB task: about 200 KB of jobs.
    let agent_call = |i: usize| json!({"type":"assistant","sessionId":"s","timestamp":"2026-10-01T10:00:01Z","message":{"id":format!("m{i}"),"role":"assistant","model":"m","content":[{"type":"tool_use","id":format!("t{i}"),"name":"Agent","input":{"description":format!("Helper {}{}", (b'a' + (i / 26) as u8) as char, (b'a' + (i % 26) as u8) as char),"subagent_type":"general-purpose","prompt":"x".repeat(5000)}}]}});
    let mut text = json!({"type":"user","sessionId":"s","timestamp":"2026-10-01T10:00:00Z","message":{"role":"user","content":"Go"}}).to_string() + "\n";
    for i in 0..40 {
        text.push_str(&format!("{}\n", agent_call(i)));
    }
    let (status, c) = call_route(
        &s,
        json!({ "files": [{ "name": "big.jsonl", "text": text }] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{c}");
    let (team, seq) = import_text(&s, c["session"].as_str().unwrap()).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/improve-from-run"),
        json!({ "options": opts(json!({})), "start_seq": seq }),
    )
    .await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("cli_missing")),
        "the prompt was not refused as too long: {v}"
    );
}

async fn call_route(s: &Served, body: Value) -> (StatusCode, Value) {
    crate::common::m2::call(&s.router, "POST", "/api/runs/convert", body).await
}

async fn import_text(s: &Served, session: &str) -> (String, i64) {
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/runs/import")
        .header("host", "test")
        .header("content-type", "application/x-ndjson")
        .body(axum::body::Body::from(session.to_string()))
        .unwrap();
    let resp = tower::ServiceExt::oneshot(s.router.clone(), req)
        .await
        .unwrap();
    let bytes = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .unwrap()
        .to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    (
        v["team"]["draft"]["id"].as_str().unwrap().to_string(),
        v["run"]["start_seq"].as_i64().unwrap(),
    )
}
