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
    assert!(params.briefing.as_deref().is_some_and(|b| b.contains("Your role: Writer")), "a protected Claude agent's briefing travels with its launch, for after a compaction");
    let (rig_b, spec_b) = controld::launch::compiled_seat(&s.st, &b).unwrap();
    assert!(controld::launch::launch_params(&s.st, "local", &rig_b.name, &spec_b, s.home.path()).unwrap().briefing.is_none(), "Codex has no compaction hook");
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

#[tokio::test]
async fn skills_components_join_an_agents_skills_and_remote_servers_and_plugins_reach_the_runner() {
    let s = serve().await;
    let claude = ClaudeStreamDriver { program: fake_bin("fake-claude") };
    let codex = CodexAppServerDriver { program: fake_bin("fake-codex"), native_questions: false };
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join(hostd::supervisor::COMPONENT_SECRETS_FILE), json!({ "FOAM_TOKEN_FOR_TEST": "s3cret" }).to_string()).unwrap();
    let tools = hostd::supervisor::SeatToolsHost { socket: data.path().join("seat-tools.sock"), shim: "/unused/sculptured".into() };
    let _runner = connect(&s, vec![Arc::new(claude), Arc::new(codex)], Some(tools)).await;
    wait_for("both drivers", || s.st.hosts.has_driver("local", "claude-stream") && s.st.hosts.has_driver("local", "codex-app-server")).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let folder = dir.path().display().to_string();
    let (_, rec) = call(&s.router, "POST", "/api/teams", json!({ "request_id": rid(), "template": "blank", "name": "Datacenter" })).await;
    let mut draft = rec["draft"].clone();
    let all = json!(["mesh-skills", "openfoam", "weather", "foam-plugin"]);
    draft["agents"] = json!([
        { "id": "a", "group_id": null, "name": "Planner", "kind": "claude", "job": "plan", "folder": folder, "x": 40, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "integrations": all } },
        { "id": "b", "group_id": null, "name": "Solver", "kind": "codex", "job": "solve", "folder": folder, "x": 300, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "integrations": ["mesh-skills", "openfoam", "foam-plugin"] } },
    ]);
    draft["components"] = json!([
        { "id": "mesh-skills", "name": "Mesh skills", "description": "Meshing advice.", "kind": "skills",
          "skills": [{ "name": "mesh-check", "description": "Use before a CFD run.", "body": "Check the mesh quality first." }] },
        { "id": "openfoam", "name": "OpenFOAM", "description": "CFD runs.", "kind": "mcp-remote", "url": "https://foam.example.com/mcp",
          "headers": { "Authorization": "Bearer ${FOAM_TOKEN_FOR_TEST}" }, "install": "Ask the HPC team for a token." },
        { "id": "weather", "name": "Weather", "description": "Site weather.", "kind": "mcp-remote", "url": "https://weather.example.com/mcp",
          "headers": { "X-Key": "${WEATHER_KEY_NOT_SET_FOR_TEST}" }, "install": "Get a key from the weather service." },
        { "id": "foam-plugin", "name": "OpenFOAM plugin", "description": "Case templates.", "kind": "claude-plugin", "plugin": "foam-not-installed@nowhere",
          "claude_only": true, "install": "claude plugin install foam-not-installed@nowhere" },
    ]);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    let first = |who: &str| events(&s.st, who).into_iter().find_map(|e| match e.body { model::EventBody::MessageQueued { text, .. } => Some(text), _ => None });
    wait_for("both jobs are sent", || first(&a).is_some() && first(&b).is_some()).await;
    assert!(first(&b).unwrap().contains("- mesh-check: Use before a CFD run."), "a Codex agent is told the pack's skills");
    let read = controld::skills::read_skill(&s.st, &b, &json!({ "name": "mesh-check" })).unwrap();
    assert!(read["text"].as_str().unwrap().contains("Check the mesh quality first."), "{read}");

    let params = |who: &str| {
        let (rig, spec) = controld::launch::compiled_seat(&s.st, who).unwrap();
        controld::launch::launch_params(&s.st, "local", &rig.name, &spec, s.home.path()).unwrap()
    };
    let ids = |p: &host_proto::LaunchParams| p.components.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    let (pa, pb) = (params(&a), params(&b));
    assert_eq!(pa.skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["mesh-check"], "the pack joins the agent's skills");
    assert_eq!(ids(&pa), ["openfoam", "weather", "foam-plugin"], "the pack itself is not sent");
    assert_eq!(ids(&pb), ["openfoam"], "a Claude plugin is not given to Codex");
    assert!(pa.required_features().contains(&host_proto::features::COMPONENT_KINDS), "an older runner could not read these");

    let dropped = |who: &str| events(&s.st, who).into_iter().filter_map(|e| match e.body { model::EventBody::FeaturesDropped { features, note } => Some((features, note)), _ => None }).collect::<Vec<_>>();
    wait_for("the runner tells what it could not give", || dropped(&a).len() >= 2).await;
    let notes = dropped(&a);
    let note = |id: &str| notes.iter().find(|(f, _)| f == &[id.to_string()]).map(|(_, n)| n.clone()).unwrap_or_default();
    assert!(note("weather").contains("sculptured set-secret WEATHER_KEY_NOT_SET_FOR_TEST"), "{notes:?}");
    assert!(note("foam-plugin").contains("claude plugin install foam-not-installed@nowhere"), "{notes:?}");
    assert!(note("openfoam").is_empty(), "its secret is set on the computer: {notes:?}");
    let file = std::fs::read_to_string(data.path().join("components/remote").join(format!("{a}.json"))).unwrap();
    assert!(file.contains("Bearer s3cret") && !file.contains("weather"), "Claude's private config holds the filled-in server only: {file}");
    assert!(!notes.iter().any(|(_, n)| n.contains("s3cret")), "the secret is never in a note");
}
