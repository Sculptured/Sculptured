//! Agents' skills and integrations: sent with the launch, a Codex agent told its skills and reading
//! one with `read_skill`, Kindex named in the briefing. Fake CLIs only.
mod common;
use axum::http::StatusCode;
use common::m2::*;
use common::{fake_bin, wait_for};
use drivers::claude::ClaudeStreamDriver;
use drivers::codex::CodexAppServerDriver;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn skills_and_kindex_reach_each_agent_the_way_its_cli_can_use_them() {
    let s = serve().await;
    let claude = ClaudeStreamDriver { program: fake_bin("fake-claude") };
    let codex = CodexAppServerDriver { program: fake_bin("fake-codex"), native_questions: false };
    let data = tempfile::tempdir().unwrap();
    let tools = hostd::supervisor::SeatToolsHost { socket: data.path().join("seat-tools.sock"), shim: "/unused/sculptured".into() };
    let _runner = connect(&s, vec![Arc::new(claude), Arc::new(codex)], Some(tools)).await;
    wait_for("both drivers", || s.st.hosts.has_driver("local", "claude-stream") && s.st.hosts.has_driver("local", "codex-app-server")).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let folder = dir.path().display().to_string();
    let skill = json!({ "name": "money-rules", "description": "Use when handling amounts of money.", "body": "Amounts are Decimal, never float." });
    let (_, rec) = call(&s.router, "POST", "/api/teams", json!({ "request_id": rid(), "template": "blank", "name": "Skilled" })).await;
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([
        { "id": "a", "group_id": null, "name": "Writer", "kind": "claude", "job": "write", "folder": folder, "x": 40, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "skills": [skill], "integrations": ["kindex", "notes"] } },
        { "id": "b", "group_id": null, "name": "Checker", "kind": "codex", "job": "check", "folder": folder, "x": 300, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "skills": [skill] } },
    ]);
    draft["components"] = json!([{ "id": "notes", "name": "Notes server", "description": "Team notes.", "kind": "mcp", "program": "notes-mcp-not-installed", "install": "pip install notes-mcp", "briefing": "You also have the notes tools." }]);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    let first = |who: &str| events(&s.st, who).into_iter().find_map(|e| match e.body { model::EventBody::MessageQueued { text, .. } => Some(text), _ => None });
    wait_for("both jobs are sent", || first(&a).is_some() && first(&b).is_some()).await;
    let (to_a, to_b) = (first(&a).unwrap(), first(&b).unwrap());
    assert!(to_b.contains("Your skills (load one with read_skill") && to_b.contains("- money-rules: Use when handling amounts of money."), "{to_b}");
    assert!(!to_a.contains("Your skills"), "Claude loads its skills natively: {to_a}");
    assert!(to_a.contains("You also have Kindex") && to_a.contains("You also have the notes tools.") && !to_b.contains("Kindex"), "each agent is told about its own components: {to_a}");

    let (rig, spec) = controld::launch::compiled_seat(&s.st, &a).unwrap();
    let params = controld::launch::launch_params(&s.st, "local", &rig.name, &spec, s.home.path()).unwrap();
    assert_eq!((params.skills.len(), params.integrations.clone()), (1, vec!["kindex".to_string()]), "built-in ones by name too, for older runners");
    assert_eq!(params.components.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["kindex", "notes"]);
    let dropped = |who: &str| events(&s.st, who).into_iter().filter_map(|e| match e.body { model::EventBody::FeaturesDropped { features, note } => Some((features, note)), _ => None }).collect::<Vec<_>>();
    wait_for("the missing program is told on the agent", || dropped(&a).iter().any(|(f, _)| f == &["notes".to_string()])).await;
    let (_, note) = dropped(&a).into_iter().find(|(f, _)| f == &["notes".to_string()]).unwrap();
    assert!(note.contains("notes-mcp-not-installed") && note.contains("pip install notes-mcp"), "{note}");
    let plugin = data.path().join("skills").join(&a).join("skills/money-rules/SKILL.md");
    assert!(std::fs::read_to_string(&plugin).is_ok_and(|md| md.contains("Amounts are Decimal")), "the runner wrote Claude's plugin: {}", plugin.display());

    let read = controld::skills::read_skill(&s.st, &b, &json!({ "name": "money-rules" })).unwrap();
    assert!(read["text"].as_str().unwrap().contains("Amounts are Decimal, never float."), "{read}");
    let missing = controld::skills::read_skill(&s.st, &b, &json!({ "name": "nope" })).unwrap_err();
    assert!(missing.contains("Your skills: money-rules"), "{missing}");
}
