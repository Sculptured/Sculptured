//! Agents' skills and integrations: sent with the launch, a Codex agent told its skills and reading
//! one with `read_skill`, Kindex named in the briefing. Launch settings and `read_skill` are read
//! from a team started with no computer connected; what agents are sent and what the runner does
//! with it are checked live, on the real Claude Code (haiku) and Codex (cheapest model) CLIs.
mod common;
use axum::http::StatusCode;

use common::live::{CLAUDE_MODEL, CODEX_MODEL};
use common::m2::{Served, rid};
use serde_json::{Value, json};

const OK: &str = "Reply with the single word OK and nothing else.";

/// Saves team `name` with `agents` and `components`, working in `live::work_folder`, on the
/// cheapest models; returns its id. Not started.
async fn team(s: &Served, name: &str, agents: Value, components: Value) -> String {
    let (_, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": name }),
    )
    .await;
    let mut draft = rec["draft"].clone();
    draft["folder"] = json!(common::live::work_folder(s));
    draft["models"] = json!({ "claude": CLAUDE_MODEL, "codex": CODEX_MODEL });
    draft["agents"] = agents;
    draft["components"] = components;
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

/// Writer (Claude: a skill, Kindex and a notes server) and Checker (Codex: the same skill).
async fn skilled(s: &Served) -> String {
    let folder = common::live::work_folder(s);
    let skill = json!({ "name": "money-rules", "description": "Use when handling amounts of money.", "body": "Amounts are Decimal, never float." });
    let agents = json!([
        { "id": "a", "group_id": null, "name": "Writer", "kind": "claude", "job": OK, "folder": folder, "x": 40, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "skills": [skill], "integrations": ["kindex", "notes"] } },
        { "id": "b", "group_id": null, "name": "Checker", "kind": "codex", "job": OK, "folder": folder, "x": 300, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "skills": [skill] } },
    ]);
    let components = json!([{ "id": "notes", "name": "Notes server", "description": "Team notes.", "kind": "mcp", "program": "notes-mcp-not-installed", "install": "pip install notes-mcp", "briefing": "You also have the notes tools." }]);
    team(s, "Skilled", agents, components).await
}

/// Planner (Claude) and Solver (Codex) with a skills pack, two remote servers and a Claude plugin.
async fn datacenter(s: &Served) -> String {
    let folder = common::live::work_folder(s);
    let all = json!(["mesh-skills", "openfoam", "weather", "foam-plugin"]);
    let agents = json!([
        { "id": "a", "group_id": null, "name": "Planner", "kind": "claude", "job": OK, "folder": folder, "x": 40, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "integrations": all } },
        { "id": "b", "group_id": null, "name": "Solver", "kind": "codex", "job": OK, "folder": folder, "x": 300, "y": 40,
          "advanced": { "permission": "unrestricted", "resume": "resume-if-possible", "integrations": ["mesh-skills", "openfoam", "foam-plugin"] } },
    ]);
    let components = json!([
        { "id": "mesh-skills", "name": "Mesh skills", "description": "Meshing advice.", "kind": "skills",
          "skills": [{ "name": "mesh-check", "description": "Use before a CFD run.", "body": "Check the mesh quality first." }] },
        { "id": "openfoam", "name": "OpenFOAM", "description": "CFD runs.", "kind": "mcp-remote", "url": "https://foam.example.com/mcp",
          "headers": { "Authorization": "Bearer ${FOAM_TOKEN_FOR_TEST}" }, "install": "Ask the HPC team for a token." },
        { "id": "weather", "name": "Weather", "description": "Site weather.", "kind": "mcp-remote", "url": "https://weather.example.com/mcp",
          "headers": { "X-Key": "${WEATHER_KEY_NOT_SET_FOR_TEST}" }, "install": "Get a key from the weather service." },
        { "id": "foam-plugin", "name": "OpenFOAM plugin", "description": "Case templates.", "kind": "claude-plugin", "plugin": "foam-not-installed@nowhere",
          "claude_only": true, "install": "claude plugin install foam-not-installed@nowhere" },
    ]);
    team(s, "Datacenter", agents, components).await
}

/// Starts team `id` with no computer connected: its compiled agents are recorded, none launches.
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

fn params(s: &Served, who: &str) -> host_proto::LaunchParams {
    let (rig, spec) = controld::launch::compiled_seat(&s.st, who).unwrap();
    controld::launch::launch_params(&s.st, "local", &rig.name, &spec, s.home.path()).unwrap()
}

fn component_ids(p: &host_proto::LaunchParams) -> Vec<String> {
    p.components.iter().map(|c| c.id.clone()).collect()
}

fn first(s: &Served, who: &str) -> Option<String> {
    crate::common::m2::events(&s.st, who)
        .into_iter()
        .find_map(|e| match e.body {
            model::EventBody::MessageQueued { text, .. } => Some(text),
            _ => None,
        })
}

fn dropped(s: &Served, who: &str) -> Vec<(Vec<String>, String)> {
    crate::common::m2::events(&s.st, who)
        .into_iter()
        .filter_map(|e| match e.body {
            model::EventBody::FeaturesDropped { features, note } => Some((features, note)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn skills_and_integrations_travel_with_each_agents_launch() {
    let s = crate::common::m2::serve().await;
    let id = skilled(&s).await;
    start_without_computer(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    let pa = params(&s, &a);
    assert_eq!(
        (pa.skills.len(), pa.integrations.clone()),
        (1, vec!["kindex".to_string()]),
        "built-in ones by name too, for older runners"
    );
    assert_eq!(component_ids(&pa), ["kindex", "notes"]);
    assert!(
        pa.briefing
            .as_deref()
            .is_some_and(|b| b.contains("Your role: Writer")),
        "a protected Claude agent's briefing travels with its launch, for after a compaction"
    );
    assert!(
        params(&s, &b).briefing.is_none(),
        "Codex has no compaction hook"
    );
    let read = controld::skills::read_skill(&s.st, &b, &json!({ "name": "money-rules" })).unwrap();
    assert!(
        read["text"]
            .as_str()
            .unwrap()
            .contains("Amounts are Decimal, never float."),
        "{read}"
    );
    let missing = controld::skills::read_skill(&s.st, &b, &json!({ "name": "nope" })).unwrap_err();
    assert!(missing.contains("Your skills: money-rules"), "{missing}");
}

#[tokio::test]
#[ignore = "live: real claude (haiku) and codex CLIs; run with --ignored"]
async fn live_skills_and_kindex_reach_each_agent_the_way_its_cli_can_use_them() {
    let (Some(_), Some(_)) = (common::live::require(), common::live::require_codex()) else {
        return;
    };
    let (s, _runner, data) = common::live::with_tools().await;
    let id = skilled(&s).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    common::live::wait_for("both jobs are sent", || {
        first(&s, &a).is_some() && first(&s, &b).is_some()
    })
    .await;
    let (to_a, to_b) = (first(&s, &a).unwrap(), first(&s, &b).unwrap());
    assert!(
        to_b.contains("Your skills (load one with read_skill")
            && to_b.contains("- money-rules: Use when handling amounts of money."),
        "{to_b}"
    );
    assert!(
        !to_a.contains("Your skills"),
        "Claude loads its skills natively: {to_a}"
    );
    assert!(
        to_a.contains("You also have Kindex")
            && to_a.contains("You also have the notes tools.")
            && !to_b.contains("Kindex"),
        "each agent is told about its own components: {to_a}"
    );
    common::live::wait_for("the missing program is told on the agent", || {
        dropped(&s, &a)
            .iter()
            .any(|(f, _)| f == &["notes".to_string()])
    })
    .await;
    let (_, note) = dropped(&s, &a)
        .into_iter()
        .find(|(f, _)| f == &["notes".to_string()])
        .unwrap();
    assert!(
        note.contains("notes-mcp-not-installed") && note.contains("pip install notes-mcp"),
        "{note}"
    );
    let plugin = data
        .path()
        .join("skills")
        .join(&a)
        .join("skills/money-rules/SKILL.md");
    assert!(
        std::fs::read_to_string(&plugin).is_ok_and(|md| md.contains("Amounts are Decimal")),
        "the runner wrote Claude's plugin: {}",
        plugin.display()
    );
    crate::common::m2::stop_team(&s, &id).await;
}

#[tokio::test]
async fn skills_components_join_an_agents_skills_and_plugins_stay_with_claude() {
    let s = crate::common::m2::serve().await;
    let id = datacenter(&s).await;
    start_without_computer(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    let read = controld::skills::read_skill(&s.st, &b, &json!({ "name": "mesh-check" })).unwrap();
    assert!(
        read["text"]
            .as_str()
            .unwrap()
            .contains("Check the mesh quality first."),
        "{read}"
    );
    let (pa, pb) = (params(&s, &a), params(&s, &b));
    assert_eq!(
        pa.skills
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["mesh-check"],
        "the pack joins the agent's skills"
    );
    assert_eq!(
        component_ids(&pa),
        ["openfoam", "weather", "foam-plugin"],
        "the pack itself is not sent"
    );
    assert_eq!(
        component_ids(&pb),
        ["openfoam"],
        "a Claude plugin is not given to Codex"
    );
    assert!(
        pa.required_features()
            .contains(&host_proto::features::COMPONENT_KINDS),
        "an older runner could not read these"
    );
}

#[tokio::test]
#[ignore = "live: real claude (haiku) and codex CLIs; run with --ignored"]
async fn live_remote_servers_and_plugins_reach_the_runner_and_codex_is_told_its_skills() {
    let (Some(_), Some(_)) = (common::live::require(), common::live::require_codex()) else {
        return;
    };
    let (s, _runner, data) = common::live::with_tools().await;
    std::fs::write(
        data.path().join(hostd::supervisor::COMPONENT_SECRETS_FILE),
        json!({ "FOAM_TOKEN_FOR_TEST": "s3cret" }).to_string(),
    )
    .unwrap();
    let id = datacenter(&s).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    common::live::wait_for("both jobs are sent", || {
        first(&s, &a).is_some() && first(&s, &b).is_some()
    })
    .await;
    assert!(
        first(&s, &b)
            .unwrap()
            .contains("- mesh-check: Use before a CFD run."),
        "a Codex agent is told the pack's skills"
    );
    common::live::wait_for("the runner tells what it could not give", || {
        dropped(&s, &a).len() >= 2
    })
    .await;
    let notes = dropped(&s, &a);
    let note = |id: &str| {
        notes
            .iter()
            .find(|(f, _)| f == &[id.to_string()])
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    };
    assert!(
        note("weather").contains("sculptured set-secret WEATHER_KEY_NOT_SET_FOR_TEST"),
        "{notes:?}"
    );
    assert!(
        note("foam-plugin").contains("claude plugin install foam-not-installed@nowhere"),
        "{notes:?}"
    );
    assert!(
        note("openfoam").is_empty(),
        "its secret is set on the computer: {notes:?}"
    );
    let file = std::fs::read_to_string(
        data.path()
            .join("components/remote")
            .join(format!("{a}.json")),
    )
    .unwrap();
    assert!(
        file.contains("Bearer s3cret") && !file.contains("weather"),
        "Claude's private config holds the filled-in server only: {file}"
    );
    assert!(
        !notes.iter().any(|(_, n)| n.contains("s3cret")),
        "the secret is never in a note"
    );
    crate::common::m2::stop_team(&s, &id).await;
}
