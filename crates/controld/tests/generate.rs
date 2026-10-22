//! Generating teams with the user's own CLI: questions, alternatives as laid-out team files, retry.
//! The CLIs are fixtures; no model is called.
mod common;
use axum::http::StatusCode;

use common::m2::Served;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A fake `claude` that answers questions with `questions.json` and teams with `teams.json` from
/// `dir` (each a full `--output-format json` result), logging each prompt.
fn fake_claude(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("claude");
    std::fs::write(&p, format!("#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nD={}\ncat >> $D/prompts\necho '---' >> $D/prompts\n{body}\n", dir.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..100 {
        match std::process::Command::new(&p).arg("--version").output() {
            Err(e) if e.raw_os_error() == Some(26) => {
                std::thread::sleep(std::time::Duration::from_millis(20))
            }
            _ => break,
        }
    }
    p
}

const BY_SCHEMA: &str =
    r#"case "$*" in *alternatives*) cat $D/teams.json;; *) cat $D/questions.json;; esac"#;

async fn served_with(claude: PathBuf) -> Served {
    let mut s = crate::common::m2::serve().await;
    s.st = s.st.clone().with_cli_programs(Some(claude), None);
    s.router = controld::app(s.st.clone(), None);
    s
}

fn result(v: Value) -> String {
    json!({ "type": "result", "structured_output": v }).to_string()
}

fn sketch(name: &str, agents: &[&str], arrows: &[(&str, &str)]) -> Value {
    json!({
        "name": name,
        "groups": [],
        "agents": agents.iter().map(|a| json!({ "name": a, "kind": "claude", "group": null, "job": format!("Do {a}'s part"), "check": null })).collect::<Vec<_>>(),
        "arrows": arrows.iter().map(|(f, t)| json!({ "from": f, "to": t, "kind": "hands-work-to" })).collect::<Vec<_>>(),
    })
}

fn opts(extra: Value) -> Value {
    let mut o = json!({ "cli": "claude", "model": null, "size": null, "kinds": null, "shape": null, "checks": false, "folder": null, "host": null, "team": null });
    let matching_extra = extra.as_object().unwrap();
    for (k, v) in matching_extra {
        o[k] = v.clone();
    }
    o
}

#[tokio::test]
async fn questions_come_back_as_two_to_four_with_suggestions() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("questions.json"),
        result(json!({ "questions": [
            { "question": "Which framework?", "suggestions": ["Astro", "Next.js"] },
            { "question": "Who reviews?", "suggestions": ["A reviewer agent", "Nobody"] },
            { "question": "Tests?", "suggestions": ["Yes", "No"] },
        ]})),
    )
    .unwrap();
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/questions",
        json!({ "options": opts(json!({})), "prompt": "Rebuild our docs site" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["questions"].as_array().unwrap().len(), 3);
    assert!(
        std::fs::read_to_string(d.path().join("prompts"))
            .unwrap()
            .contains("Rebuild our docs site")
    );
}

#[tokio::test]
async fn three_alternatives_become_team_files_laid_out_with_issues() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("teams.json"), result(json!({ "alternatives": [
        { "label": "lean", "rationale": "Two agents.", "team": sketch("Docs", &["Writer", "Editor"], &[("Writer", "Editor")]) },
        { "label": "balanced", "rationale": "A chain.", "team": sketch("Docs", &["Planner", "Writer", "Editor"], &[("Planner", "Writer"), ("Writer", "Editor"), ("Editor", "Ghost")]) },
        { "label": "thorough", "rationale": "Four.", "team": sketch("Docs", &["Planner", "Writer", "Editor", "Checker"], &[("Planner", "Writer")]) },
    ]}))).unwrap();
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({})), "prompt": "docs", "answers": [], "count": 3 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let alts = v["alternatives"].as_array().unwrap();
    assert_eq!(alts.len(), 3);
    assert!(
        alts.iter()
            .all(|a| a["draft"]["agents"].is_array() && a["issues"].is_array())
    );
    let balanced = &alts[1];
    let x = |name: &str| {
        balanced["draft"]["agents"]
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
    assert!(
        balanced["dropped"].to_string().contains("Ghost"),
        "{}",
        balanced["dropped"]
    );
    assert_eq!(
        balanced["draft"]["connections"].as_array().unwrap().len(),
        2
    );
}

#[tokio::test]
async fn practice_option_makes_every_agent_practice_and_the_folder_is_set() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("teams.json"), result(json!({ "alternatives": [
        { "label": "team", "rationale": "One.", "team": sketch("Docs", &["Writer", "Editor"], &[("Writer", "Editor")]) },
    ]}))).unwrap();
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
    let folder = s.home.path().join("docs");
    std::fs::create_dir_all(&folder).unwrap();
    let (status, v) = crate::common::m2::call(&s.router, "POST", "/api/generate/teams", json!({ "options": opts(json!({ "kinds": "practice", "folder": folder.display().to_string() })), "prompt": "docs", "answers": [], "count": 1 })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let team = &v["alternatives"][0]["draft"];
    assert!(
        team["agents"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["kind"] == "practice")
    );
    assert_eq!(team["folder"], json!(folder.display().to_string()));
}

#[tokio::test]
async fn a_bad_answer_is_retried_once_then_reported_with_the_raw_text() {
    let d = tempfile::tempdir().unwrap();
    let s = served_with(fake_claude(
        d.path(),
        r#"echo x >> $D/calls; echo '{"type":"result","result":"nonsense"}'"#,
    ))
    .await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({})), "prompt": "docs", "answers": [], "count": 1 }),
    )
    .await;
    assert_eq!(
        (status, v["error"]["code"].as_str()),
        (StatusCode::BAD_GATEWAY, Some("generate_unusable")),
        "{v}"
    );
    assert!(
        v.to_string().contains("nonsense"),
        "the raw answer is shown: {v}"
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("calls"))
            .unwrap()
            .lines()
            .count(),
        2,
        "one retry"
    );

    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("teams.json"),
        result(json!({ "alternatives": [
            { "label": "team", "rationale": "One.", "team": sketch("Docs", &["Writer"], &[]) },
        ]})),
    )
    .unwrap();
    let s = served_with(fake_claude(d.path(), r#"echo x >> $D/calls; if [ "$(wc -l < $D/calls)" -eq 1 ]; then echo garbage; else cat $D/teams.json; fi"#)).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({})), "prompt": "docs", "answers": [], "count": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the retry's answer is used: {v}");
    assert!(
        std::fs::read_to_string(d.path().join("prompts"))
            .unwrap()
            .contains("could not be used"),
        "the retry says why"
    );
}

#[tokio::test]
async fn duplicate_agent_names_get_unique_ids() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("teams.json"), result(json!({ "alternatives": [
        { "label": "team", "rationale": "Two writers.", "team": sketch("Docs", &["Writer", "Writer"], &[]) },
    ]}))).unwrap();
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({})), "prompt": "docs", "answers": [], "count": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let agents = v["alternatives"][0]["draft"]["agents"].as_array().unwrap();
    assert_ne!(agents[0]["id"], agents[1]["id"]);
}

fn one_team(dir: &Path, team: Value) {
    std::fs::write(
        dir.join("teams.json"),
        result(json!({ "alternatives": [{ "label": "team", "rationale": "One.", "team": team }] })),
    )
    .unwrap();
}

#[tokio::test]
async fn two_arrows_between_the_same_agents_keep_distinct_ids() {
    let d = tempfile::tempdir().unwrap();
    let mut t = sketch("Docs", &["A", "B", "C", "A-B"], &[("A", "B")]);
    t["arrows"] = json!([
        { "from": "A", "to": "B", "kind": "hands-work-to" },
        { "from": "A", "to": "B", "kind": "watches" },
        { "from": "A-B", "to": "C", "kind": "hands-work-to" },
        { "from": "A", "to": "C", "kind": "hands-work-to" },
    ]);
    one_team(d.path(), t);
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
    let (_, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({})), "prompt": "docs", "answers": [], "count": 1 }),
    )
    .await;
    let ids: Vec<String> = v["alternatives"][0]["draft"]["connections"]
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

#[tokio::test]
async fn agents_sharing_a_name_are_renamed_and_names_match_regardless_of_case() {
    let d = tempfile::tempdir().unwrap();
    let mut t = sketch("Docs", &["Writer", "Writer", "Editor"], &[]);
    t["arrows"] = json!([{ "from": "writer", "to": " Editor ", "kind": "hands-work-to" }]);
    one_team(d.path(), t);
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
    let (_, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({})), "prompt": "docs", "answers": [], "count": 1 }),
    )
    .await;
    let alt = &v["alternatives"][0];
    let names: Vec<&str> = alt["draft"]["agents"]
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
        alt["dropped"].to_string().contains("Writer 2"),
        "and that is said: {}",
        alt["dropped"]
    );
    assert_eq!(
        alt["draft"]["connections"].as_array().unwrap().len(),
        1,
        "\"writer\" and \" Editor \" still match"
    );
}

#[tokio::test]
async fn the_schemas_leave_counts_to_the_server() {
    let d = tempfile::tempdir().unwrap();
    // Asked for 3, the CLI gives 2: they are shown (no failed retry over a count).
    std::fs::write(d.path().join("teams.json"), result(json!({ "alternatives": [
        { "label": "lean", "rationale": "A.", "team": sketch("Docs", &["Writer"], &[]) },
        { "label": "thorough", "rationale": "B.", "team": sketch("Docs", &["Writer", "Editor"], &[]) },
    ]}))).unwrap();
    let s = served_with(fake_claude(
        d.path(),
        &format!(r#"printf '%s\n' "$@" > $D/args; {BY_SCHEMA}"#),
    ))
    .await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/generate/teams",
        json!({ "options": opts(json!({})), "prompt": "docs", "answers": [], "count": 3 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["alternatives"].as_array().unwrap().len(), 2);
    let args = std::fs::read_to_string(d.path().join("args")).unwrap();
    assert!(
        !args.contains("minItems") && !args.contains("maxItems"),
        "structured-output modes may refuse item limits: {args}"
    );
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
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/runs/import")
        .header("host", "test")
        .header("content-type", "application/x-ndjson")
        .body(axum::body::Body::from(
            c["session"].as_str().unwrap().to_string(),
        ))
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

#[tokio::test]
async fn a_team_rebuilt_from_a_session_is_improved_from_a_summary_without_tool_output() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("teams.json"), result(json!({ "alternatives": [
        { "label": "team", "rationale": "A lead and a researcher.", "team": sketch("Login", &["Lead", "Researcher"], &[("Lead", "Researcher")]) },
    ]}))).unwrap();
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
    let (team, seq) = imported(&s).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/improve-from-run"),
        json!({ "options": opts(json!({})), "start_seq": seq }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let names: Vec<&str> = v["draft"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Lead", "Researcher"]);
    assert_eq!(v["rationale"], "A lead and a researcher.");
    let prompt = std::fs::read_to_string(d.path().join("prompts")).unwrap();
    assert!(
        prompt.contains("Explore auth") && prompt.contains("Find how auth works"),
        "the summary names the agents and their tasks"
    );
    assert!(
        !prompt.contains("users table"),
        "tool output never goes into the prompt"
    );
}

#[tokio::test]
async fn only_an_imported_run_can_improve_a_team() {
    let d = tempfile::tempdir().unwrap();
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
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

#[tokio::test]
async fn a_large_session_still_fits_the_improve_prompt() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("teams.json"), result(json!({ "alternatives": [
        { "label": "team", "rationale": "Smaller.", "team": sketch("Big", &["Lead", "Helper"], &[("Lead", "Helper")]) },
    ]}))).unwrap();
    let s = served_with(fake_claude(d.path(), BY_SCHEMA)).await;
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
    assert_eq!(status, StatusCode::OK, "{v}");
    let prompt = std::fs::read_to_string(d.path().join("prompts")).unwrap();
    assert!(prompt.len() <= 32 * 1024, "{}", prompt.len());
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
