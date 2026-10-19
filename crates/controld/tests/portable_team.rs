//! Team files are MADL documents: export without machine folders unless asked; import refuses a
//! wrong or broken file before creating anything, listing every issue; a drafted team keeps where it
//! came from.
mod common;
use axum::http::StatusCode;
use common::m2::*;
use serde_json::{Value, json};

async fn team_count(s: &Served) -> usize {
    get(&s.router, "/api/teams").await.1.as_array().map(Vec::len).unwrap_or(0)
}

/// A two-agent Practice team with folders, own copies and the restricted posture.
async fn designed(s: &Served) -> (String, String) {
    let id = practice_chain(s, "Exported", &[("a", "Alpha", "plan"), ("b", "Beta", "build")], &[("a", "b")]).await;
    let folder = s.home.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let folder = folder.display().to_string();
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft["folder"] = json!(folder);
    draft["agents"][1]["folder"] = json!(folder);
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}/settings"), json!({ "settings": { "posture": "restricted", "question_timeout_min": null, "own_copies": true } })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (id, folder)
}

fn without_machine_parts(mut d: Value) -> Value {
    for k in ["id", "revision", "folder"] { d.as_object_mut().unwrap().remove(k); }
    for a in d["agents"].as_array_mut().unwrap() { a.as_object_mut().unwrap().remove("folder"); }
    d
}

async fn import(s: &Served, doc: &Value) -> (StatusCode, Value) {
    call(&s.router, "POST", "/api/teams/import-madl", json!({ "request_id": rid(), "text": doc.to_string() })).await
}

#[tokio::test]
async fn a_team_exports_without_folders_and_imports_as_an_identical_new_team() {
    let s = serve().await;
    let (id, folder) = designed(&s).await;
    let (status, doc) = get(&s.router, &format!("/api/teams/{id}/madl")).await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["madl"], "draft");
    assert_eq!(doc["$schema"], json!(controld::portable::madl::MADL_SCHEMA_URL), "editors can validate and complete the file");
    assert!(!doc.to_string().contains(&folder), "no machine folder in the file: {doc}");
    assert_eq!(doc["defaults"]["preferences"]["workspace"]["copy"], json!("own"));
    assert_eq!(doc["defaults"]["restrictions"]["approvals"]["required_for"], json!(["runtime-permissions"]));

    let (status, imported) = import(&s, &doc).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let new_id = imported["team"]["draft"]["id"].as_str().unwrap().to_string();
    assert_ne!(new_id, id, "a new team");
    let (_, original) = get(&s.router, &format!("/api/teams/{id}")).await;
    assert_eq!(without_machine_parts(imported["team"]["draft"].clone()), without_machine_parts(original["draft"].clone()));
    let (_, settings) = get(&s.router, &format!("/api/teams/{new_id}/settings")).await;
    assert_eq!((settings["own_copies"].clone(), settings["posture"].clone()), (json!(true), json!("restricted")));

    let (_, with) = get(&s.router, &format!("/api/teams/{id}/madl?folders=true")).await;
    assert_eq!((with["defaults"]["preferences"]["workspace"]["folder"].clone(), with["agents"][1]["workspace"]["folder"].clone()), (json!(folder), json!(folder)));
}

#[tokio::test]
async fn a_wrong_or_broken_file_creates_nothing_and_says_why() {
    let s = serve().await;
    let (id, _) = designed(&s).await;
    let (_, doc) = get(&s.router, &format!("/api/teams/{id}/madl")).await;
    let before = team_count(&s).await;

    let mut later = doc.clone();
    later["madl"] = json!("2099-01-01");
    let (status, err) = import(&s, &later).await;
    assert_eq!((status, err["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("madl_invalid")), "{err}");
    assert!(err.to_string().contains("unknown-revision"), "names the problem: {err}");

    let (status, err) = call(&s.router, "POST", "/api/teams/import-madl", json!({ "request_id": rid(), "text": "{}" })).await;
    assert_eq!((status, err["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("madl_invalid")), "{err}");

    let mut broken = doc.clone();
    broken["connections"] = json!([{ "id": "c-x", "from": "a", "to": "ghost", "kind": "handoff" }]);
    let dup = broken["agents"][0].clone();
    broken["agents"].as_array_mut().unwrap().push(dup);
    let (status, err) = import(&s, &broken).await;
    assert_eq!((status, err["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("madl_invalid")), "{err}");
    let found = err["error"]["diagnostics"].as_array().unwrap();
    assert!(found.len() >= 2, "every problem is listed, not just the first: {err}");

    assert_eq!(team_count(&s).await, before, "nothing was created");
}

#[tokio::test]
async fn a_claude_team_without_folders_imports_and_its_missing_folders_are_warnings() {
    let s = serve().await;
    let (id, _) = designed(&s).await;
    let (_, mut doc) = get(&s.router, &format!("/api/teams/{id}/madl")).await;
    for a in doc["agents"].as_array_mut().unwrap() { a["runtime"] = json!("claude-code"); }
    let (status, imported) = import(&s, &doc).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let codes: Vec<&str> = imported["warnings"].as_array().unwrap().iter().filter_map(|i| i["code"].as_str()).collect();
    assert!(codes.contains(&"folder-missing"), "shown so the user can choose a folder in Design: {codes:?}");
}

#[tokio::test]
async fn a_file_can_be_checked_without_creating_a_team() {
    let s = serve().await;
    let (id, _) = designed(&s).await;
    let (_, doc) = get(&s.router, &format!("/api/teams/{id}/madl")).await;
    let before = team_count(&s).await;
    let (status, v) = call(&s.router, "POST", "/api/teams/check-madl", json!({ "text": doc.to_string() })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v["issues"].is_array() && v["dropped"].is_array());
    let yaml = serde_yaml_ng::to_string(&doc).unwrap();
    let (status, v) = call(&s.router, "POST", "/api/teams/check-madl", json!({ "text": yaml })).await;
    assert_eq!(status, StatusCode::OK, "YAML reads too: {v}");
    let (status, err) = call(&s.router, "POST", "/api/teams/check-madl", json!({ "text": "madl: draft\n" })).await;
    assert_eq!((status, err["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("madl_invalid")), "{err}");
    assert_eq!(team_count(&s).await, before, "nothing was created");
}

#[tokio::test]
async fn a_drafted_team_keeps_where_it_came_from() {
    let s = serve().await;
    let (id, _) = designed(&s).await;
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    let draft = rec["draft"].clone();
    let (status, v) = call(&s.router, "POST", "/api/generate/check", json!({ "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert!(v["issues"].is_array());
    let origin = json!({ "kind": "generated", "cli": "claude", "model": null, "at_ms": 1, "prompt": "docs team" });
    let (status, created) = call(&s.router, "POST", "/api/generate/create", json!({ "request_id": rid(), "draft": draft, "origin": origin })).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let new_id = created["team"]["draft"]["id"].as_str().unwrap().to_string();
    let (_, rec) = get(&s.router, &format!("/api/teams/{new_id}")).await;
    assert_eq!((rec["origin"]["cli"].clone(), rec["origin"]["prompt"].clone()), (json!("claude"), json!("docs team")), "{rec}");
    let mut broken = draft.clone();
    broken["connections"] = json!([{ "id": "c-x", "from": "a", "to": "ghost", "kind": "hands-work-to" }]);
    let before = team_count(&s).await;
    let (status, err) = call(&s.router, "POST", "/api/generate/create", json!({ "request_id": rid(), "draft": broken, "origin": origin })).await;
    assert_eq!((status, err["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("team_invalid")), "{err}");
    assert_eq!(team_count(&s).await, before, "nothing was created");
}
