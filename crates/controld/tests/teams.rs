mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use controld::store::{RigSource, Store};
use controld::{app, AppState};
use host_proto::HostEvent;
use http_body_util::BodyExt;
use model::{Actor, Desired, EventBody, Provenance, SeatLifecycle, Subject, TeamDraft};
use serde_json::{json, Value};
use tower::ServiceExt;

fn setup() -> (AppState, Router, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let st = AppState::new(Store::open_in_memory().unwrap(), "tok".into(), vec!["test".into()]).with_home(home.path());
    let router = app(st.clone(), None);
    (st, router, home)
}

async fn call(router: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri).header("host", "test");
    let body = match body {
        Some(b) => {
            req = req.header("content-type", "application/json");
            Body::from(b.to_string())
        }
        None => Body::empty(),
    };
    let resp = router.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn create(router: &Router, request_id: &str, template: &str, practice: bool) -> Value {
    let (status, body) = call(router, "POST", "/api/teams", Some(json!({ "request_id": request_id, "template": template, "practice": practice }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn messages(record: &Value) -> Vec<String> {
    record["issues"].as_array().unwrap().iter().map(|i| i["message"].as_str().unwrap().to_string()).collect()
}

fn draft_of(record: &Value) -> TeamDraft {
    serde_json::from_value(record["draft"].clone()).unwrap()
}

#[tokio::test]
async fn create_from_a_template_with_practice_agents_and_list_it() {
    let (_st, router, _home) = setup();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    assert_eq!(rec["draft"]["id"], "builder-and-reviewer");
    assert_eq!(rec["draft"]["name"], "Builder and Reviewer");
    assert_eq!(rec["draft"]["agents"][0]["kind"], "practice");
    assert_eq!(rec["live"], false);
    assert_eq!(rec["started_revision"], Value::Null);
    assert!(messages(&rec).is_empty(), "{:?}", messages(&rec));

    assert_eq!(create(&router, "t1", "builder-reviewer", true).await, rec, "same request_id replays");
    let second = create(&router, "t2", "builder-reviewer", true).await;
    assert_eq!(second["draft"]["id"], "builder-and-reviewer-2");

    let (status, list) = call(&router, "GET", "/api/teams", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[1]["agent_count"], 2);
    assert_eq!(list[1]["started"], Value::Null);

    let (status, templates) = call(&router, "GET", "/api/team-templates", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(templates[1]["id"], "builder-reviewer");
    assert_eq!(templates[1]["preview"]["agents"][1]["kind"], "codex");
}

#[tokio::test]
async fn without_practice_a_template_reports_folders_in_plain_words_and_codex_needs_no_warning() {
    let (_st, router, _home) = setup();
    let rec = create(&router, "t1", "builder-reviewer", false).await;
    let m = messages(&rec);
    assert!(m.contains(&"Builder needs a folder to work in".to_string()), "{m:?}");
    assert!(!m.iter().any(|x| x.contains("can't run in this version")), "{m:?}");
}

#[tokio::test]
async fn unknown_template_and_missing_request_id_are_400() {
    let (_st, router, _home) = setup();
    let (status, body) = call(&router, "POST", "/api/teams", Some(json!({ "request_id": "x", "template": "nope" }))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("unknown_template")));
    let (status, body) = call(&router, "POST", "/api/teams", Some(json!({ "template": "blank" }))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("request_id_required")));
}

#[tokio::test]
async fn put_saves_with_a_revision_and_a_stale_revision_is_409() {
    let (_st, router, _home) = setup();
    let rec = create(&router, "t1", "solo", true).await;
    let mut d = draft_of(&rec);
    assert_eq!(d.revision, 0);
    d.name = "Renamed".into();
    let (status, saved) = call(&router, "PUT", "/api/teams/solo-agent", Some(json!({ "request_id": "p1", "revision": 0, "draft": d }))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["draft"]["revision"], 1);
    d.name = "Lost update".into();
    let (status, body) = call(&router, "PUT", "/api/teams/solo-agent", Some(json!({ "request_id": "p2", "revision": 0, "draft": d }))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "draft_conflict");
    let (_, now) = call(&router, "GET", "/api/teams/solo-agent", None).await;
    assert_eq!((now["draft"]["name"].clone(), now["draft"]["revision"].clone()), (json!("Renamed"), json!(1)));

    let (status, body) = call(&router, "PUT", "/api/teams/other", Some(json!({ "request_id": "p3", "revision": 1, "draft": d }))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::BAD_REQUEST, json!("team_id_mismatch")));
    let (status, _) = call(&router, "GET", "/api/teams/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn put_returns_live_issues_for_the_saved_draft() {
    let (_st, router, home) = setup();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let mut d = draft_of(&rec);
    let gone = home.path().join("gone");
    d.agents[0].folder = Some(gone.display().to_string());
    std::fs::write(home.path().join("a-file"), "x").unwrap();
    d.agents[1].folder = Some(home.path().join("a-file").display().to_string());
    let (status, saved) = call(&router, "PUT", "/api/teams/builder-and-reviewer", Some(json!({ "request_id": "p1", "revision": 0, "draft": d }))).await;
    assert_eq!(status, StatusCode::OK);
    let m = messages(&saved);
    assert!(m.contains(&format!("Builder's folder no longer exists: {}", gone.display())), "{m:?}");
    assert!(m.contains(&format!("Reviewer's folder is a file, not a folder: {}", home.path().join("a-file").display())), "{m:?}");
    assert_eq!(saved["issues"][0]["element"], json!({ "kind": "agent", "id": "builder" }));
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn renaming_a_running_team_is_allowed_but_adding_an_agent_is_409() {
    let (st, router, _home) = setup();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let mut d = draft_of(&rec);
    let rig = rigspec::from_draft(&d).unwrap().rig;
    st.store.lock().unwrap().upsert_rig(&rig, &RigSource::Team { id: d.id.clone(), revision: 0 }, Desired::Running).unwrap();
    st.store
        .lock()
        .unwrap()
        .ingest(
            "local",
            HostEvent {
                host_seq: 1,
                at_ms: 1,
                actor: Actor::System("hostd".into()),
                subject: Subject::Seat(format!("team.builder@{}", d.id)),
                provenance: Provenance::ProtocolObserved,
                body: EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Running, reason: None },
            },
        )
        .unwrap();
    let (_, now) = call(&router, "GET", "/api/teams/builder-and-reviewer", None).await;
    assert_eq!(now["live"], true);
    assert_eq!(now["started_revision"], 0);

    d.name = "Renamed while running".into();
    d.agents[0].name = "Lead builder".into();
    d.agents[0].job = "New job text".into();
    d.agents[0].x += 16;
    let (status, saved) = call(&router, "PUT", "/api/teams/builder-and-reviewer", Some(json!({ "request_id": "p1", "revision": 0, "draft": d }))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");

    let mut grown = draft_of(&saved);
    let mut extra = grown.agents[0].clone();
    extra.id = "extra".into();
    extra.name = "Extra".into();
    grown.agents.push(extra);
    let (status, body) = call(&router, "PUT", "/api/teams/builder-and-reviewer", Some(json!({ "request_id": "p2", "revision": 1, "draft": grown }))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "team_running");
}

#[tokio::test]
async fn validate_checks_an_unsaved_draft_without_a_request_id() {
    let (_st, router, _home) = setup();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let mut d = draft_of(&rec);
    d.connections[0].to = "builder".into();
    let (status, issues) = call(&router, "POST", "/api/teams/builder-and-reviewer/validate", Some(json!({ "draft": d }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(issues[0]["message"], "Builder's connection loops back to itself");
    let (status, stored) = call(&router, "POST", "/api/teams/builder-and-reviewer/validate", Some(json!({}))).await;
    assert_eq!((status, stored), (StatusCode::OK, json!([])));
}

#[tokio::test]
async fn spec_returns_yaml_with_the_layout_comment() {
    let (_st, router, _home) = setup();
    create(&router, "t1", "builder-reviewer", true).await;
    let (status, spec) = call(&router, "GET", "/api/teams/builder-and-reviewer/spec", None).await;
    assert_eq!(status, StatusCode::OK);
    let yaml = spec["yaml"].as_str().unwrap();
    assert!(yaml.contains("name: builder-and-reviewer") && yaml.contains("runtime: stub"), "{yaml}");
    assert!(yaml.contains("# sculptured: "), "{yaml}");
    assert_eq!(spec["valid"], true);
}

#[tokio::test]
async fn import_creates_a_team_and_lists_dropped_parts() {
    let (_st, router, _home) = setup();
    let yaml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../rigspec/tests/fixtures/implementation-pair.yaml")).unwrap();
    let (status, body) = call(&router, "POST", "/api/teams/import", Some(json!({ "request_id": "i1", "yaml": yaml }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["team"]["draft"]["id"], "implementation-pair");
    assert_eq!(body["team"]["draft"]["agents"][0]["id"], "impl");
    assert!(!body["dropped"].as_array().unwrap().is_empty());
    let (status, body) = call(&router, "POST", "/api/teams/import", Some(json!({ "request_id": "i2", "yaml": yaml.replace("runtime: codex", "runtime: gpt") }))).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "spec_invalid");
    assert_eq!(body["error"]["diagnostics"][0]["path"], "pods[0].members[1].runtime");
}

#[tokio::test]
async fn team_ids_avoid_existing_rig_names_and_reserved_paths() {
    let (st, router, _home) = setup();
    let rig = rigspec::compile("version: \"0.2\"\nname: new-team\npods:\n  - id: p\n    label: P\n    members:\n      - { id: a, agent_ref: \"local:a\", profile: default, runtime: stub, cwd: \".\" }\n").unwrap();
    st.store.lock().unwrap().upsert_rig(&rig, &RigSource::File { path: "/x.yaml".into() }, Desired::Stopped).unwrap();
    assert_eq!(create(&router, "b1", "blank", true).await["draft"]["id"], "new-team-2");
    let (status, body) = call(&router, "POST", "/api/teams", Some(json!({ "request_id": "b2", "template": "blank", "name": "Import" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["draft"]["id"], "import-2");
}

#[tokio::test]
async fn every_team_route_is_behind_the_login_seam() {
    let (mut st, _router, _home) = setup();
    st.auth = controld::auth::AuthPolicy::DenyAll;
    let router = app(st, None);
    for (method, uri, body) in [
        ("GET", "/api/teams", None),
        ("POST", "/api/teams", Some(json!({ "request_id": "a1", "template": "blank" }))),
        ("GET", "/api/team-templates", None),
        ("GET", "/api/teams/x", None),
        ("PUT", "/api/teams/x", Some(json!({ "request_id": "a2", "revision": 0, "draft": {} }))),
        ("POST", "/api/teams/x/validate", Some(json!({}))),
        ("GET", "/api/teams/x/spec", None),
        ("POST", "/api/teams/import", Some(json!({ "request_id": "a3", "yaml": "" }))),
    ] {
        let (status, body) = call(&router, method, uri, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(body["error"]["code"], "login_required", "{method} {uri}");
    }
}

// ---- Backend hardening: agent folders sit inside home ----

fn issues_with(issues: &Value, code: &str) -> Vec<(String, String)> {
    issues
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["code"] == code)
        .map(|i| (i["element"]["id"].as_str().unwrap().to_string(), i["message"].as_str().unwrap().to_string()))
        .collect()
}

async fn validate_with(router: &Router, d: &TeamDraft) -> Value {
    let (status, issues) = call(router, "POST", &format!("/api/teams/{}/validate", d.id), Some(json!({ "draft": d }))).await;
    assert_eq!(status, StatusCode::OK, "{issues}");
    issues
}

fn is_root() -> bool {
    std::process::Command::new("id").arg("-u").output().map(|o| o.stdout.starts_with(b"0\n")).unwrap_or(false)
}

/// An agent folder outside home is refused with the same words whether or not it
/// exists, so the issues never reveal what exists outside home.
#[tokio::test]
async fn agent_folders_outside_home_are_refused_without_saying_whether_they_exist() {
    let (_st, router, home) = setup();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("a-file"), "x").unwrap();
    std::os::unix::fs::symlink(outside.path(), home.path().join("escape")).unwrap();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let base = draft_of(&rec);
    let places = [
        outside.path().to_path_buf(),
        outside.path().join("missing"),
        outside.path().join("missing/deeper"),
        outside.path().join("a-file"),
        home.path().join("escape"),
        home.path().join("escape/missing"),
    ];
    let mut seen = Vec::new();
    for p in &places {
        let mut d = base.clone();
        d.agents[0].folder = Some(p.display().to_string());
        let issues = validate_with(&router, &d).await;
        let found = issues_with(&issues, "folder-outside-home");
        assert_eq!(found, [("builder".to_string(), "Builder's folder is outside your home folder".to_string())], "{}: {issues}", p.display());
        let text = issues.to_string();
        assert!(!text.contains(&outside.path().display().to_string()), "no outside path is echoed: {text}");
        seen.push(issues);
    }
    assert!(seen.windows(2).all(|w| w[0] == w[1]), "the answer is the same for every outside place: {seen:?}");
}

/// Inside home, missing folders, files and unreadable folders keep their own codes.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn agent_folders_inside_home_report_missing_files_and_unreadable() {
    let (_st, router, home) = setup();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let mut d = draft_of(&rec);
    std::fs::write(home.path().join("a-file"), "x").unwrap();
    d.agents[0].folder = Some(home.path().join("gone").display().to_string());
    d.agents[1].folder = Some(home.path().join("a-file").display().to_string());
    let issues = validate_with(&router, &d).await;
    assert_eq!(issues_with(&issues, "folder-not-found").len(), 1, "{issues}");
    assert_eq!(issues_with(&issues, "folder-not-a-folder").len(), 1, "{issues}");
    let dotted = format!("{}/gone/../work", home.path().display());
    d.agents[1].folder = Some(dotted.clone());
    let issues = validate_with(&router, &d).await;
    assert_eq!(
        issues_with(&issues, "folder-bad-path"),
        [("reviewer".to_string(), format!("Reviewer's folder path can't be used: {dotted}"))],
        "{issues}"
    );

    if is_root() {
        eprintln!("skipped the unreadable case: permissions are not enforced for root");
        return;
    }
    let locked = home.path().join("locked");
    std::fs::create_dir_all(locked.join("inner")).unwrap();
    std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o000)).unwrap();
    d.agents[0].folder = Some(locked.join("inner").display().to_string());
    d.agents[1].folder = Some(home.path().display().to_string());
    let issues = validate_with(&router, &d).await;
    std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert_eq!(
        issues_with(&issues, "folder-unreadable"),
        [("builder".to_string(), format!("Sculptured can't read Builder's folder: {}", locked.join("inner").display()))],
        "{issues}"
    );
    assert_eq!(issues.as_array().unwrap().len(), 1, "home itself is a valid folder: {issues}");
}

/// Start checks folders again: a folder that moved outside home after it was saved
/// (here a link re-pointed outside) stops the start.
#[tokio::test]
async fn start_checks_folders_again() {
    let (_st, router, home) = setup();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join("work")).unwrap();
    let link = home.path().join("link");
    std::os::unix::fs::symlink(home.path().join("work"), &link).unwrap();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let mut d = draft_of(&rec);
    d.agents[0].folder = Some(link.display().to_string());
    let (status, saved) = call(&router, "PUT", &format!("/api/teams/{}", d.id), Some(json!({ "request_id": "p1", "revision": 0, "draft": d }))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(messages(&saved).is_empty(), "{saved}");

    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    let (status, body) = call(&router, "POST", &format!("/api/teams/{}/start", d.id), Some(json!({ "request_id": "s1" }))).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("team_invalid")), "{body}");
    assert_eq!(issues_with(&body["error"]["issues"], "folder-outside-home").len(), 1, "{body}");
}

/// With no known home folder, a Practice agent without a folder has nowhere to run,
/// and no agent's folder can be checked against home.
#[tokio::test]
async fn an_unknown_home_is_an_issue_for_agents_that_need_it() {
    let (mut st, _router, home) = setup();
    st.home = None;
    let router = app(st, None);
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    assert_eq!(
        issues_with(&rec["issues"], "home-unknown"),
        [
            ("builder".to_string(), "Your home folder is unknown, so Builder needs a folder".to_string()),
            ("reviewer".to_string(), "Your home folder is unknown, so Reviewer needs a folder".to_string()),
        ],
        "{rec}"
    );
    let mut d = draft_of(&rec);
    d.agents[0].folder = Some(home.path().display().to_string());
    let issues = validate_with(&router, &d).await;
    assert_eq!(
        issues_with(&issues, "home-unknown"),
        [
            ("builder".to_string(), "Your home folder is unknown, so Builder's folder can't be checked".to_string()),
            ("reviewer".to_string(), "Your home folder is unknown, so Reviewer needs a folder".to_string()),
        ],
        "{issues}"
    );
}

// ---- Backend hardening: missing tests ----

async fn put(router: &Router, request_id: &str, revision: u64, d: &TeamDraft) -> (StatusCode, Value) {
    call(router, "PUT", &format!("/api/teams/{}", d.id), Some(json!({ "request_id": request_id, "revision": revision, "draft": d }))).await
}

/// A PUT retried with the same request_id replays the stored answer and saves once; the
/// same id with a different body is refused.
#[tokio::test]
async fn put_replays_the_same_request_id_and_saves_once() {
    let (_st, router, _home) = setup();
    let mut d = draft_of(&create(&router, "t1", "solo", true).await);
    d.name = "Renamed".into();
    let first = put(&router, "p1", 0, &d).await;
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(put(&router, "p1", 0, &d).await, first, "same request_id replays");
    let (_, now) = call(&router, "GET", "/api/teams/solo-agent", None).await;
    assert_eq!(now["draft"]["revision"], 1, "the retry did not save again");
    d.name = "Something else".into();
    let (status, body) = put(&router, "p1", 0, &d).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::CONFLICT, json!("request_id_reused")), "{body}");
}

fn fixture_yaml() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../rigspec/tests/fixtures/implementation-pair.yaml")).unwrap()
}

async fn import(router: &Router, request_id: &str, yaml: &str) -> (StatusCode, Value) {
    call(router, "POST", "/api/teams/import", Some(json!({ "request_id": request_id, "yaml": yaml }))).await
}

/// An import retried with the same request_id replays and creates one team.
#[tokio::test]
async fn import_replays_the_same_request_id_and_creates_one_team() {
    let (_st, router, _home) = setup();
    let yaml = fixture_yaml();
    let first = import(&router, "i1", &yaml).await;
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(import(&router, "i1", &yaml).await, first, "same request_id replays");
    let (_, list) = call(&router, "GET", "/api/teams", None).await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    let (status, body) = import(&router, "i1", &yaml.replace("implementation-pair", "other-pair")).await;
    assert_eq!((status, body["error"]["code"].clone()), (StatusCode::CONFLICT, json!("request_id_reused")), "{body}");
}

/// The same spec imported twice gets two teams with distinct ids, and a spec named
/// after a fixed API path (`import`) never takes that id.
#[tokio::test]
async fn imports_never_reuse_an_id_or_take_a_reserved_one() {
    let (_st, router, _home) = setup();
    let yaml = fixture_yaml();
    let (_, a) = import(&router, "i1", &yaml).await;
    let (_, b) = import(&router, "i2", &yaml).await;
    assert_eq!(a["team"]["draft"]["id"], "implementation-pair");
    assert_eq!(b["team"]["draft"]["id"], "implementation-pair-2");
    let named_import = yaml.replace("name: implementation-pair", "name: import");
    assert!(named_import.contains("name: import\n"), "{named_import}");
    let (status, c) = import(&router, "i3", &named_import).await;
    assert_eq!(status, StatusCode::OK, "{c}");
    assert_eq!(c["team"]["draft"]["id"], "import-2");
    let (status, _) = call(&router, "GET", "/api/teams/import-2", None).await;
    assert_eq!(status, StatusCode::OK);
}

/// The team list is newest first by last change, whatever the ids say.
#[tokio::test]
async fn teams_are_listed_newest_first() {
    let (_st, router, _home) = setup();
    let ids = |list: &Value| list.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let older = draft_of(&create(&router, "t1", "blank", true).await);
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    create(&router, "t2", "solo", true).await;
    let (_, list) = call(&router, "GET", "/api/teams", None).await;
    assert_eq!(ids(&list), ["solo-agent", "new-team"], "the newer team comes first although its id sorts later");
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _) = put(&router, "p1", 0, &older).await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(&router, "GET", "/api/teams", None).await;
    assert_eq!(ids(&list), ["new-team", "solo-agent"], "a save makes a team the newest");
}

/// A spec file booted under a team's id is not that team running: the team stays
/// not live and not started, and its structure can still change.
#[tokio::test]
async fn a_file_sourced_rig_with_the_team_id_is_not_the_team_live() {
    let (st, router, _home) = setup();
    let rec = create(&router, "t1", "solo", true).await;
    let mut d = draft_of(&rec);
    let rig = rigspec::from_draft(&d).unwrap().rig;
    st.store.lock().unwrap().upsert_rig(&rig, &RigSource::File { path: "/x.yaml".into() }, Desired::Running).unwrap();
    let seat = rig.seats[0].id.clone();
    st.store
        .lock()
        .unwrap()
        .ingest(
            "local",
            HostEvent {
                host_seq: 1,
                at_ms: 1,
                actor: Actor::System("hostd".into()),
                subject: Subject::Seat(seat),
                provenance: Provenance::ProtocolObserved,
                body: EventBody::SeatLifecycleChanged { lifecycle: SeatLifecycle::Running, reason: None },
            },
        )
        .unwrap();
    let (_, now) = call(&router, "GET", "/api/teams/solo-agent", None).await;
    assert_eq!((now["live"].clone(), now["started_revision"].clone()), (json!(false), Value::Null), "{now}");
    let mut extra = d.agents[0].clone();
    extra.id = "extra".into();
    extra.name = "Extra".into();
    d.agents.push(extra);
    let (status, body) = put(&router, "p1", 0, &d).await;
    assert_eq!(status, StatusCode::OK, "a structural change is allowed: {body}");
}

/// Agent folders that are links returning through `..` from outside home answer alike.
#[tokio::test]
async fn agent_folder_links_that_return_through_dot_dot_answer_alike() {
    let (_st, router, home) = setup();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir(outside.path().join("realdir")).unwrap();
    std::fs::write(outside.path().join("afile"), "x").unwrap();
    std::fs::create_dir(home.path().join("proj")).unwrap();
    // From `outside/<via>`, `..` twice reaches the shared temp folder, then home's own name.
    let home_name = home.path().file_name().unwrap().to_str().unwrap().to_string();
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let mut seen = Vec::new();
    for via in ["nosuchA", "realdir", "afile"] {
        let link = home.path().join(format!("dot-{via}"));
        std::os::unix::fs::symlink(format!("{}/{via}/../../{home_name}/proj", outside.path().display()), &link).unwrap();
        let mut d = draft_of(&rec);
        d.agents[0].folder = Some(link.display().to_string());
        let issues = validate_with(&router, &d).await;
        assert_eq!(
            issues_with(&issues, "folder-outside-home"),
            [("builder".to_string(), "Builder's folder is outside your home folder".to_string())],
            "{via}: {issues}"
        );
        seen.push(issues);
    }
    assert!(seen.windows(2).all(|w| w[0] == w[1]), "{seen:?}");
}

/// An agent folder spelled through the raw HOME (reached through a link) is inside home.
#[tokio::test]
async fn agent_folders_under_a_home_reached_through_a_link_are_inside() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("home/u/proj")).unwrap();
    std::os::unix::fs::symlink("home", root.path().join("homelink")).unwrap();
    let raw = root.path().join("homelink/u");
    let st = AppState::new(Store::open_in_memory().unwrap(), "tok".into(), vec!["test".into()]).with_home(&raw);
    let router = app(st, None);
    let rec = create(&router, "t1", "builder-reviewer", true).await;
    let mut d = draft_of(&rec);
    d.agents[0].folder = Some(raw.join("proj").display().to_string());
    d.agents[1].folder = Some(raw.display().to_string());
    let issues = validate_with(&router, &d).await;
    assert_eq!(issues, json!([]), "{issues}");
}

#[tokio::test]
async fn project_folder_persists_and_inherited_paths_obey_home_boundaries() {
    let (_st, router, home) = setup();
    let rec = create(&router, "folder-team", "builder-reviewer", false).await;
    let mut d = draft_of(&rec);
    d.folder = Some(home.path().display().to_string());
    let uri = format!("/api/teams/{}", d.id);
    let (status, saved) = call(&router, "PUT", &uri, Some(json!({"request_id":"folder-save", "revision":d.revision,"draft":d}))).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(!saved["issues"].as_array().unwrap().iter().any(|i| i["severity"] == "error"), "{saved}");
    let (_, read) = call(&router, "GET", &uri, None).await;
    assert_eq!(read["draft"]["folder"], home.path().display().to_string());
    let mut d = draft_of(&read);
    d.folder = Some("/outside-home".into());
    let (_, invalid) = call(&router, "POST", &format!("{uri}/validate"), Some(json!({"draft":d}))).await;
    assert!(invalid.as_array().unwrap().iter().any(|i| i["code"] == "folder-outside-home"), "{invalid}");
}

#[tokio::test]
async fn reusable_groups_are_versioned_library_records_not_projects() {
    let (_st, router, _home) = setup();
    let mut draft=rigspec::templates::template("builder-reviewer","fragment","Pair",true).unwrap();
    draft.groups.push(model::DraftGroup{id:"group".into(),label:"Pair".into(),x:0,y:0,w:600,h:300,models:model::ModelDefaults::default()});
    for a in &mut draft.agents {a.group_id=Some("group".into());}
    let block=model::GroupBlock{id:"pair".into(),name:"Pair".into(),revision:0,draft,boundary_notes:vec![],source:None};
    let req=json!({"request_id":"group-save","block":block});
    let (status,saved)=call(&router,"PUT","/api/group-blocks/pair",Some(req.clone())).await;
    assert_eq!(status,StatusCode::OK,"{saved}"); assert_eq!(saved["revision"],1);
    assert_eq!(call(&router,"PUT","/api/group-blocks/pair",Some(req)).await.1,saved);
    let (_,teams)=call(&router,"GET","/api/teams",None).await;assert_eq!(teams,json!([]));
    assert_eq!(call(&router,"GET","/api/group-blocks/pair",None).await.1,saved);
    assert_eq!(call(&router,"GET","/api/group-blocks",None).await.1,json!([saved.clone()]));
    let (status,body)=call(&router,"PUT","/api/group-blocks/pair",Some(json!({"request_id":"stale","block":block}))).await;
    assert_eq!(status,StatusCode::CONFLICT);assert_eq!(body["error"]["code"],"group_block_conflict");
    let mut bad=saved;bad["draft"]["connections"][0]["to"]=json!("outside");
    let (status,_)=call(&router,"PUT","/api/group-blocks/pair",Some(json!({"request_id":"external","block":bad}))).await;
    assert_eq!(status,StatusCode::BAD_REQUEST);
}

/// A Library copy of a Community block, as "Save to Library" makes it.
fn community_copy(source: serde_json::Value) -> serde_json::Value {
    let mut draft=rigspec::templates::template("builder-reviewer","fragment","Pair",true).unwrap();
    draft.groups.push(model::DraftGroup{id:"group".into(),label:"Pair".into(),x:0,y:0,w:600,h:300,models:model::ModelDefaults::default()});
    for a in &mut draft.agents {a.group_id=Some("group".into());}
    let mut block=json!({"id":"community-b1","name":"Pair","revision":0,"draft":draft,"boundary_notes":[]});
    block["source"]=source;
    block
}

#[tokio::test]
async fn a_library_copy_remembers_its_community_block_and_version() {
    let (_st, router, _home) = setup();
    let block=community_copy(json!({"community_block":"b1","version":2}));
    let (status,saved)=call(&router,"PUT","/api/group-blocks/community-b1",Some(json!({"request_id":"c1","block":block}))).await;
    assert_eq!(status,StatusCode::OK,"{saved}");
    assert_eq!(saved["source"],json!({"community_block":"b1","version":2}));
    assert_eq!(call(&router,"GET","/api/group-blocks/community-b1",None).await.1["source"],json!({"community_block":"b1","version":2}));
}

#[tokio::test]
async fn a_bad_community_source_is_refused() {
    let (_st, router, _home) = setup();
    for (n,source) in [json!({"community_block":"../x","version":1}),json!({"community_block":"b1","version":0})].into_iter().enumerate() {
        let (status,body)=call(&router,"PUT","/api/group-blocks/community-b1",Some(json!({"request_id":format!("bad{n}"),"block":community_copy(source)}))).await;
        assert_eq!((status,body["error"]["code"].as_str()),(StatusCode::BAD_REQUEST,Some("group_block_invalid")),"{body}");
    }
}

#[tokio::test]
async fn input_position_can_be_saved_while_running_without_changing_workflow_configuration() {
    let (st,router,_home)=setup();
    let rec=create(&router,"input-team","textbook",true).await;
    let mut d=draft_of(&rec);let rig=rigspec::from_draft(&d).unwrap().rig;
    st.store.lock().unwrap().upsert_rig(&rig,&RigSource::Team{id:d.id.clone(),revision:0},Desired::Running).unwrap();
    st.store.lock().unwrap().ingest("local",HostEvent{host_seq:1,at_ms:1,actor:Actor::System("hostd".into()),subject:Subject::Seat(d.seat_id("dispatcher").unwrap()),provenance:Provenance::ProtocolObserved,body:EventBody::SeatLifecycleChanged{lifecycle:SeatLifecycle::Running,reason:None}}).unwrap();
    d.workflow.as_mut().unwrap().input_position=Some(model::BoardPosition{x:-120,y:280});
    let uri=format!("/api/teams/{}",d.id);
    let (status,saved)=call(&router,"PUT",&uri,Some(json!({"request_id":"move-input","revision":0,"draft":d}))).await;
    assert_eq!(status,StatusCode::OK,"{saved}");
    assert_eq!(call(&router,"GET",&uri,None).await.1["draft"]["workflow"]["input_position"],json!({"x":-120,"y":280}));
    let mut changed=draft_of(&saved);changed.workflow.as_mut().unwrap().input="New runtime input".into();
    let (status,_)=call(&router,"PUT",&uri,Some(json!({"request_id":"change-input","revision":1,"draft":changed}))).await;
    assert_eq!(status,StatusCode::CONFLICT);
}
