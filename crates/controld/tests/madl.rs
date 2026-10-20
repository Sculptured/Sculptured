//! MADL files: a team exports as a valid MADL document, entirely in MADL's core, and comes back
//! identical from JSON or YAML; a MADL file written elsewhere imports as a team, listing what
//! Sculptured can't keep.
mod common;
use axum::http::StatusCode;
use common::m2::*;
use controld::portable::madl::{read, to_madl};
use controld::portable::team::{PortableSettings, PortableTeam};
use serde_json::json;

fn file_of(team: model::TeamDraft, settings: PortableSettings) -> PortableTeam {
    PortableTeam { team, settings }
}

fn gstack() -> PortableTeam {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/teams/gstack-roles.madl.json")).unwrap();
    read(&text).unwrap().file
}

/// Export to MADL, read it back as JSON and as YAML, and check nothing changed and nothing rode
/// in an extension.
fn round_trip(file: &PortableTeam) {
    let doc = to_madl(file);
    let report = madl::validate(&doc);
    assert!(report.is_ok(), "the export is valid MADL: {:?}", report.diagnostics);
    let text = serde_json::to_string_pretty(&doc).unwrap();
    assert!(!text.contains("com.usesculptured") && !text.contains("\"extensions\""), "everything is in MADL's core: {text}");
    for text in [text, serde_yaml_ng::to_string(&doc).unwrap()] {
        let back = read(&text).unwrap_or_else(|e| panic!("{e:?}\n{text}"));
        assert!(back.dropped.is_empty(), "nothing dropped: {:?}", back.dropped);
        assert_eq!(serde_json::to_value(&back.file).unwrap(), serde_json::to_value(file).unwrap(), "{text}");
    }
}

#[test]
fn an_example_team_round_trips_through_madl() {
    round_trip(&gstack());
}

#[test]
fn budgets_and_agent_limits_round_trip_through_madl() {
    let mut file = gstack();
    file.team.budget = serde_json::from_value(json!({
        "plan": [{ "provider": "claude", "window": "five_hour", "soft": 25.0, "hard": 30.0, "pause": { "continue_with": "resume", "max_windows": 2 } }, { "provider": "codex", "window": "seven_day", "hard": 10.0 }],
        "tokens": { "hard": 50000000 }, "minutes": { "soft": 45, "hard": 60 }, "max_working": 6,
    })).unwrap();
    let a = &mut file.team.agents[0].advanced;
    a.token_limit = Some(serde_json::from_value(json!({ "tokens": 2000000, "soft": 1000000, "counts": "output", "include_subagents": false })).unwrap());
    a.turn_limit = Some(model::SoftHard { soft: None, hard: 40 });
    a.minute_limit = Some(model::SoftHard { soft: Some(20), hard: 30 });
    file.team.claude_setup = model::ClaudeSetup { global: true, project: false };
    round_trip(&file);
    let doc = serde_json::to_value(to_madl(&file)).unwrap();
    assert_eq!(doc["system"]["budget"]["wall_clock"], json!({ "soft": "45m", "hard": "60m" }));
    assert_eq!(doc["system"]["budget"]["plan"][0]["pause"], json!({ "continue_with": "resume", "max_windows": 2 }));
    assert_eq!(doc["defaults"]["preferences"]["config"], json!({ "user": true, "project": false }));
    assert_eq!(doc["agents"][0]["settings"]["restrictions"]["budget"]["counts"], json!("output"));
}

/// The textbook template's workflow, with check and person stages, a panel, escalation and positions.
fn workflow_team() -> model::TeamDraft {
    let mut team = rigspec::textbook::template("book", "Book", true);
    let w = team.workflow.as_mut().unwrap();
    let first = w.stages[0].id.clone();
    w.stages[0].routes.insert("check".into(), Some("verify".into()));
    w.stages[0].position = Some(model::BoardPosition { x: 300, y: 40 });
    w.stages.push(model::WorkflowStage { id: "verify".into(), name: "Verify".into(), kind: model::StageKind::Check, command: Some("make verify".into()), routes: [("failed".to_string(), Some(first)), ("passed".to_string(), Some("sign-off".to_string()))].into(), ..Default::default() });
    w.stages.push(model::WorkflowStage { id: "sign-off".into(), name: "Sign off".into(), kind: model::StageKind::Human, prompt: Some("Publish this book?".into()), routes: [("yes".to_string(), None)].into(), position: Some(model::BoardPosition { x: 900, y: 40 }), ..Default::default() });
    w.guard = Some(model::Guard { command: "make health".into(), every_minutes: 15, timeout_s: Some(300) });
    w.copies = model::CopiesMode::PerItem;
    w.input = "Physics for first years".into();
    w.input_position = Some(model::BoardPosition { x: -300, y: 80 });
    team
}

#[test]
fn a_workflow_team_round_trips_as_a_madl_workflow() {
    let mut team = workflow_team();
    team.agents[0].advanced.token_limit = Some(model::TokenLimit { soft: None, tokens: 50_000, counts: model::TokenCount::Output, include_subagents: false });
    team.agents[1].advanced.model = Some("claude-opus-5-5".into());
    team.input = Some(model::TeamInput { text: "Notes for the editor".into(), x: -200, y: 10, to: vec![team.agents[0].id.clone()] });
    let settings = PortableSettings { posture: Some(model::PermissionPosture::Restricted), own_copies: true, question_timeout_min: Some(45) };
    let file = file_of(team, settings);
    round_trip(&file);
    let doc = to_madl(&file);
    assert_eq!(doc.workflows.len(), 1, "the workflow is MADL's own");
    assert!(doc.steps.is_empty(), "stages are not drawn as loose steps");
    assert_eq!(doc.workflows[0].input.as_deref().and_then(|i| doc.inputs.iter().find(|x| x.id == i)).map(|i| i.to.clone()), Some(vec![file.team.workflow.as_ref().unwrap().dispatcher.clone()]));
    assert_eq!(doc.policies.as_ref().map(|p| p.guards.len()), Some(1));
}

#[test]
fn every_part_of_a_team_round_trips_in_madl_core() {
    let mut team = workflow_team();
    team.models = model::ModelDefaults { claude: Some("sonnet".into()), codex: Some("gpt-6-sol".into()) };
    team.groups[0].models = model::ModelDefaults { claude: Some("opus".into()), codex: None };
    team.folder = Some("/home/someone/book".into());
    team.agents[0].folder = Some("/home/someone/book/engine".into());
    team.agents[0].advanced.model = Some("sonnet".into());
    team.agents[1].advanced.permission = model::PermissionPosture::Restricted;
    team.agents[2].group_id = Some("not-drawn".into());
    team.protection = model::Protection { on: false, read_kb: 200, shell_chars: 4000, context_tokens: 80_000 };
    team.claude_setup = model::ClaudeSetup { global: true, project: true };
    team.safety = model::safety::presets();
    let mut notes = model::component::builtins()[0].clone();
    (notes.id, notes.name, notes.program) = ("notes".into(), "Notes server".into(), "notes-mcp".into());
    let remote = serde_json::from_value(json!({ "id": "docs", "name": "Docs", "description": "The docs server.", "kind": "mcp-remote", "url": "https://docs.example.com/mcp", "headers": { "Authorization": "Bearer ${DOCS_TOKEN}" } })).unwrap();
    let pack = serde_json::from_value(json!({ "id": "money", "name": "Money", "description": "Rules for money.", "kind": "skills", "skills": [ { "name": "money-rules", "description": "Use when handling money.", "body": "Decimal only." } ] })).unwrap();
    team.components = vec![notes, remote, pack];
    team.agents[0].advanced.integrations = vec!["notes".into(), "docs".into(), "signet".into(), "money".into()];
    team.agents[0].advanced.data_access = Some(model::DataAccess { writes: vec!["src/".into()], shared: vec![model::SharedValue { name: "port".into(), value: "PORT = 8080".into(), file: "src/config.py".into() }], ..Default::default() });
    team.budget.max_working = Some(3);
    team.limits = model::TeamLimits { max_agents: Some(8), max_child_levels: Some(2) };
    team.decision_log = true;
    team.schedules = vec![model::Schedule { id: "nightly".into(), agent: team.agents[0].id.clone(), every_minutes: 1440, message: "Review today.".into() }];
    let w = team.workflow.as_mut().unwrap();
    w.stages[0].escalate = Some(model::Escalation { after_rounds: 2, agent: team.agents[1].id.clone() });
    let settings = PortableSettings { posture: Some(model::PermissionPosture::Unrestricted), own_copies: true, question_timeout_min: Some(30) };
    round_trip(&file_of(team, settings));
}

#[test]
fn a_restricted_team_and_one_on_the_global_setting_round_trip() {
    for posture in [None, Some(model::PermissionPosture::Restricted)] {
        let file = file_of(gstack().team, PortableSettings { posture, ..Default::default() });
        round_trip(&file);
    }
}

const FOREIGN: &str = r#"
madl: draft
system: { id: cad-review, name: CAD review }
defaults:
  preferences: { runtime: claude-code, model: anthropic/claude-sonnet-5-5 }
  restrictions: { budget: { tokens: 90000 } }
inputs: [ { id: brief, text: "Design a bracket." } ]
groups: [ { id: design, name: Design } ]
agents:
  - { id: planner, groups: [design], instructions: Plan it. }
  - { id: cad, groups: [design], runtime: camel }
  - { id: lead, runtime: human }
channels: [ { id: chat, medium: direct, protocol: a2a@1.2 } ]
connections:
  - { from: planner, to: cad, kind: handoff, via: chat }
  - { from: cad, to: lead, kind: approve }
communication: { mode: closed, deny: [ { from: cad, to: planner } ] }
"#;

#[test]
fn a_foreign_madl_file_imports_and_lists_what_was_not_kept() {
    let got = read(FOREIGN).unwrap();
    let team = &got.file.team;
    assert_eq!(team.agents.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["planner", "cad"], "the person is not a board agent");
    assert_eq!(team.agents[0].advanced.model.as_deref(), Some("claude-sonnet-5-5"), "the default model, without its provider");
    assert_eq!(team.agents[0].advanced.token_limit.map(|l| l.tokens), Some(90_000));
    assert_eq!(team.input.as_ref().map(|i| i.to.clone()), Some(vec!["planner".to_string()]), "the Input goes to the starting agent");
    let paths: Vec<&str> = got.dropped.iter().map(|d| d.path.as_str()).collect();
    for expected in ["/agents/1/runtime", "/agents/2", "/connections/0", "/connections/1", "/channels/0", "/communication"] {
        assert!(paths.contains(&expected), "{expected} is listed: {paths:?}");
    }
}

#[test]
fn foreign_tools_become_components_and_what_cannot_be_kept_is_listed() {
    let text = r#"{
      "madl": "draft", "system": { "id": "t", "name": "T" },
      "tools": [ { "id": "notes", "mcp": { "command": "notes-mcp" } }, { "id": "both", "runtimes": ["codex", "camel"], "plugin": "x@y" } ],
      "agents": [ { "id": "a", "runtime": "claude-code", "tools": ["notes", "both", { "id": "web", "builtin": "claude-code:WebSearch" }, "kindex"] } ]
    }"#;
    let got = read(text).unwrap();
    assert_eq!(got.file.team.components.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["notes"]);
    assert_eq!(got.file.team.agents[0].advanced.integrations, ["notes", "kindex"], "a document tool, and a built-in one by name");
    let paths: Vec<&str> = got.dropped.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, ["/tools/1", "/agents/0/tools/2"], "{:?}", got.dropped);
}

#[test]
fn an_invalid_madl_file_is_refused_with_its_errors() {
    let err = read("madl: draft\nsystem: { id: s, name: S }\nagents: [ { id: a, groups: [nope] } ]\n").unwrap_err();
    let text = format!("{err:?}");
    assert!(text.contains("/agents/0/groups/0") && text.contains("unknown-reference"), "{text}");
}

#[tokio::test]
async fn the_api_exports_madl_json_and_imports_it_as_a_new_team() {
    let s = serve().await;
    let id = practice_chain(&s, "Exported", &[("a", "Alpha", "plan"), ("b", "Beta", "build")], &[("a", "b")]).await;
    let (status, doc) = get(&s.router, &format!("/api/teams/{id}/madl")).await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["madl"], json!("draft"), "the body is the MADL document: {doc}");
    assert_eq!(doc["$schema"], json!(controld::portable::madl::MADL_SCHEMA_URL));
    let (status, checked) = call(&s.router, "POST", "/api/teams/check-madl", json!({ "text": doc.to_string() })).await;
    assert_eq!(status, StatusCode::OK, "{checked}");
    assert_eq!(checked["dropped"], json!([]));
    let (status, imported) = call(&s.router, "POST", "/api/teams/import-madl", json!({ "request_id": rid(), "text": doc.to_string() })).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    assert_ne!(imported["team"]["draft"]["id"], json!(id), "a new team");
    assert_eq!(imported["dropped"], json!([]));
    let names: Vec<&str> = imported["team"]["draft"]["agents"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Alpha", "Beta"]);
    let (status, refused) = call(&s.router, "POST", "/api/teams/import-madl", json!({ "request_id": rid(), "text": "madl: draft\nsystem: { id: s }\n" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    let (status, _) = get(&s.router, &format!("/api/teams/{id}/file")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "the old team file is gone");
}

#[test]
fn contracts_and_protections_round_trip_through_madl() {
    let mut file = gstack();
    file.team.protection = model::Protection { on: true, read_kb: 200, shell_chars: 4000, context_tokens: 80_000 };
    let a = &mut file.team.agents[0].advanced;
    a.data_access = Some(model::DataAccess { reads: vec!["manuals/".into()], writes: vec!["cad/wings/".into()], side_effects: vec!["none".into()], rationale: Some("Only the wing team changes wing geometry.".into()), shared: vec![model::SharedValue { name: "spar count".into(), value: "SPARS = 2".into(), file: "cad/wings/params.py".into() }] });
    a.authority = Some(model::Authority { domains: vec!["wing geometry".into()], scope: Some("system".into()), rationale: None });
    round_trip(&file);
    let doc = serde_json::to_value(to_madl(&file)).unwrap();
    assert_eq!(doc["defaults"]["preferences"]["context"], json!({ "read_max_kb": 200, "tool_output_chars": 4000, "refresh_at_tokens": 80000 }));
    assert_eq!(doc["agents"][0]["data_access"]["shared"][0]["value"], json!("SPARS = 2"));
}

#[test]
fn skills_travel_embedded_and_components_are_madl_tools() {
    let mut file = gstack();
    let a = &mut file.team.agents[0].advanced;
    a.skills = vec![
        model::Skill { name: "money-rules".into(), description: "Use when handling amounts of money.".into(), body: "Decimal only.\n".into(), ..Default::default() },
        model::Skill { name: "pdf".into(), description: "Use for PDF files.".into(), body: "Read PDFs.\n".into(), source: Some("https://github.com/anthropics/skills/blob/f00dfeed/pdf/SKILL.md".into()) },
    ];
    a.integrations = vec!["kindex".into(), "notes".into()];
    let mut notes = model::component::builtins()[0].clone();
    (notes.id, notes.name, notes.program) = ("notes".into(), "Notes server".into(), "notes-mcp".into());
    file.team.components = vec![notes];
    round_trip(&file);
    let doc = to_madl(&file);
    assert_eq!(doc.tools.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["notes", "kindex"], "the team's component, then the built-in one it uses");
    assert_eq!(doc.tools[1].mcp.as_ref().and_then(|m| m.command.as_deref()), Some("kin-mcp"), "so other tools can set it up too");
}

#[test]
fn a_minimal_briefing_round_trips_as_a_madl_preference() {
    let mut file = gstack();
    file.team.agents[0].advanced.briefing = Some(model::BriefingLevel::Minimal);
    round_trip(&file);
    let doc = serde_json::to_value(to_madl(&file)).unwrap();
    assert_eq!(doc["agents"][0]["settings"]["preferences"]["briefing"], json!("minimal"));
}

#[test]
fn a_workflow_whose_batch_a_command_makes_round_trips_without_a_dispatcher() {
    let mut team = workflow_team();
    let w = team.workflow.as_mut().unwrap();
    w.dispatcher = String::new();
    w.dispatch = Some(model::WorkflowDispatch { command: "./split-issues".into(), timeout_s: Some(120) });
    let file = file_of(team, PortableSettings::default());
    round_trip(&file);
    let doc = to_madl(&file);
    assert_eq!(doc.workflows[0].dispatcher, None);
    assert_eq!(doc.workflows[0].dispatch.as_ref().map(|d| (d.command.as_str(), d.timeout_s)), Some(("./split-issues", Some(120))));
}

#[test]
fn a_team_file_exported_before_madl_still_imports() {
    let file = gstack();
    let old = json!({ "format": "sculptured.team/v1", "team": file.team, "settings": { "own_copies": true, "question_timeout_min": 30 } });
    let back = read(&old.to_string()).unwrap();
    assert_eq!(back.file.team, file.team);
    assert_eq!((back.file.settings.own_copies, back.file.settings.question_timeout_min), (true, Some(30)));
}
