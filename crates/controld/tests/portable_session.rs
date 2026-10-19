//! Session files (`sculptured.session/v1`): a run downloads with the team as it started and every event.
mod common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::m2::*;
use controld::portable::session::{Origin, SessionHeader, SessionLine, Source};
use http_body_util::BodyExt;
use model::{Actor, EventBody, Provenance, Subject};
use serde_json::json;
use tower::ServiceExt;

/// The raw text and headers of a GET.
async fn text(router: &axum::Router, uri: &str) -> (StatusCode, String, axum::http::HeaderMap) {
    let req = Request::builder().method("GET").uri(uri).header("host", "test").body(Body::empty()).unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let (status, headers) = (resp.status(), resp.headers().clone());
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap(), headers)
}

/// A running Practice team on a stub computer; returns (team, its run's start seq, the agent's seat, the computer).
async fn ran(s: &Served) -> (String, i64, String, hostd::Supervisor) {
    let host = connect_stub(s).await;
    let id = practice_chain(s, "Recorded", &[("a", "Alpha", "plan")], &[]).await;
    start_team(s, &id).await;
    let (_, runs) = get(&s.router, &format!("/api/teams/{id}/deployments")).await;
    let seq = runs[0]["start_seq"].as_i64().unwrap();
    (id.clone(), seq, seat(&id, "a"), host)
}

fn run_rows(s: &Served, id: &str, from: i64) -> i64 {
    let store = s.st.store.lock().unwrap();
    store.connection().query_row(
        "SELECT COUNT(*) FROM events WHERE seq>=?1 AND (subject=?2 OR (substr(subject,1,5)='seat:' AND substr(subject,-length(?3))=?3))",
        rusqlite::params![from, format!("rig:{id}"), format!("@{id}")],
        |r| r.get(0),
    ).unwrap()
}

fn parse(file: &str) -> (SessionHeader, Vec<SessionLine>) {
    let mut lines = file.lines();
    let header: SessionHeader = serde_json::from_str(lines.next().unwrap()).unwrap();
    (header, lines.map(|l| serde_json::from_str(l).unwrap()).collect())
}

#[tokio::test]
async fn a_run_downloads_with_the_team_as_it_started_and_every_event() {
    let s = serve().await;
    let (id, seq, agent, _host) = ran(&s).await;
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft["agents"][0]["name"] = json!("Renamed later");
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    stop_team(&s, &id).await;
    common::wait_for("stopped", || common::lifecycle(&s.st, &agent) == Some(model::SeatLifecycle::Stopped)).await;

    let (status, file, headers) = text(&s.router, &format!("/api/teams/{id}/deployments/{seq}/session")).await;
    assert_eq!(status, StatusCode::OK, "{file}");
    assert_eq!(headers["content-type"], "application/x-ndjson");
    assert!(headers["content-disposition"].to_str().unwrap().contains(".sculptured-session.jsonl"));
    let (header, lines) = parse(&file);
    assert_eq!((header.format.as_str(), header.source), ("sculptured.session/v1", Source::Sculptured));
    assert_eq!(header.team.as_ref().unwrap().agents[0].name.as_deref(), Some("Alpha"), "the team as the run started, as MADL");
    assert!(header.agents.iter().any(|a| a.id == agent), "{:?}", header.agents);
    assert!(header.ended_ms.is_some(), "stopped runs have an end");
    assert!(lines.iter().all(|l| l.origin == Origin::Observed));
    assert!(lines.windows(2).all(|w| w[0].event.seq < w[1].event.seq), "in order");
    assert_eq!(lines.len() as i64, run_rows(&s, &id, seq), "every event of the run");
}

#[tokio::test]
async fn a_long_run_downloads_every_event() {
    let s = serve().await;
    let (id, seq, agent, _host) = ran(&s).await;
    for i in 0..2100 {
        s.st.local_event(Actor::System("test".into()), Subject::Seat(agent.clone()), Provenance::Verified, EventBody::CheckStarted { command: format!("check {i}") }).unwrap();
    }
    let (_, file, _) = text(&s.router, &format!("/api/teams/{id}/deployments/{seq}/session")).await;
    let (_, lines) = parse(&file);
    assert!(lines.len() >= 2100, "{} lines: more than one export chunk", lines.len());
    assert_eq!(lines.len() as i64, run_rows(&s, &id, seq));
}

#[tokio::test]
async fn a_run_from_before_snapshots_has_a_null_team() {
    let s = serve().await;
    let (id, seq, _, _host) = ran(&s).await;
    s.st.store.lock().unwrap().connection().execute("DELETE FROM deployment_drafts", []).unwrap();
    let (status, file, _) = text(&s.router, &format!("/api/teams/{id}/deployments/{seq}/session")).await;
    assert_eq!(status, StatusCode::OK, "{file}");
    let (header, lines) = parse(&file);
    assert!(header.team.is_none());
    assert!(!lines.is_empty());
}

async fn post_text(router: &axum::Router, uri: &str, text: &str) -> (StatusCode, serde_json::Value) {
    let req = Request::builder().method("POST").uri(uri).header("host", "test").header("content-type", "application/x-ndjson").body(Body::from(text.to_string())).unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
}

fn live_events(s: &Served) -> i64 {
    s.st.store.lock().unwrap().connection().query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap()
}

async fn team_count(s: &Served) -> usize {
    get(&s.router, "/api/teams").await.1.as_array().map(Vec::len).unwrap_or(0)
}

#[tokio::test]
async fn an_exported_run_imports_as_a_past_run_and_exports_identically() {
    let s = serve().await;
    let (id, seq, agent, _host) = ran(&s).await;
    stop_team(&s, &id).await;
    common::wait_for("stopped", || common::lifecycle(&s.st, &agent) == Some(model::SeatLifecycle::Stopped)).await;
    let (_, original, _) = text(&s.router, &format!("/api/teams/{id}/deployments/{seq}/session")).await;
    let before = live_events(&s);

    let (status, imported) = post_text(&s.router, "/api/runs/import", &original).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let new_id = imported["team"]["draft"]["id"].as_str().unwrap().to_string();
    assert_ne!(new_id, id);
    let (_, runs) = get(&s.router, &format!("/api/teams/{new_id}/deployments")).await;
    let runs = runs.as_array().unwrap();
    assert_eq!(runs.len(), 1, "{runs:?}");
    let neg = runs[0]["start_seq"].as_i64().unwrap();
    assert!(neg < 0 && runs[0]["source"] == "sculptured", "{runs:?}");

    let (_, detail) = get(&s.router, &format!("/api/teams/{new_id}/deployments/{neg}")).await;
    assert!(detail["agents"].as_array().unwrap().iter().any(|a| a["id"] == agent.as_str()), "{detail}");
    let (_, page) = get(&s.router, &format!("/api/teams/{new_id}/deployments/{neg}/events?seat={}", agent.replace('@', "%40"))).await;
    assert!(!page["events"].as_array().unwrap().is_empty(), "{page}");

    let (status, again, _) = text(&s.router, &format!("/api/teams/{new_id}/deployments/{neg}/session")).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    let mut header: serde_json::Value = serde_json::from_str(again.lines().next().unwrap()).unwrap();
    assert_eq!(header["team_id"], new_id.as_str());
    header["team_id"] = json!(id);
    let rebuilt = std::iter::once(header.to_string()).chain(again.lines().skip(1).map(str::to_string)).collect::<Vec<_>>().join("\n") + "\n";
    let original_header: serde_json::Value = serde_json::from_str(original.lines().next().unwrap()).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(rebuilt.lines().next().unwrap()).unwrap(), original_header);
    assert_eq!(rebuilt.lines().skip(1).collect::<Vec<_>>(), original.lines().skip(1).collect::<Vec<_>>(), "events identical");

    assert_eq!(live_events(&s), before, "nothing entered the live event log");
    assert!(s.st.store.lock().unwrap().rig_snapshot(&new_id).unwrap().is_none(), "no agents for the imported team");
}

#[tokio::test]
async fn a_broken_session_file_says_which_line() {
    let s = serve().await;
    let (id, seq, _, _host) = ran(&s).await;
    let (_, original, _) = text(&s.router, &format!("/api/teams/{id}/deployments/{seq}/session")).await;
    let before = team_count(&s).await;
    let mut lines: Vec<String> = original.lines().map(str::to_string).collect();
    assert!(lines.len() >= 3);
    lines[2] = r#"{"origin":"observed"}"#.into();
    let (status, err) = post_text(&s.router, "/api/runs/import", &lines.join("\n")).await;
    assert_eq!((status, err["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("session_file_invalid")), "{err}");
    assert!(err["error"]["diagnostics"].as_array().unwrap().iter().any(|d| d["path"] == "line 3"), "{err}");

    let mut header: serde_json::Value = serde_json::from_str(original.lines().next().unwrap()).unwrap();
    header["format"] = json!("sculptured.session/v2");
    let v2 = std::iter::once(header.to_string()).chain(original.lines().skip(1).map(str::to_string)).collect::<Vec<_>>().join("\n");
    let (status, err) = post_text(&s.router, "/api/runs/import", &v2).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
    assert!(err.to_string().contains("sculptured.session/v1"), "{err}");
    assert_eq!(team_count(&s).await, before, "nothing was created");
}

#[tokio::test]
async fn a_run_with_an_event_this_build_cannot_read_still_downloads_unchanged() {
    let s = serve().await;
    let (id, seq, agent, _host) = ran(&s).await;
    let future = format!(r#"{{"seq":0,"host":"test","host_seq":1,"at_ms":1,"actor":{{"kind":"system","id":"x"}},"subject":{{"kind":"seat","id":"{agent}"}},"provenance":"verified","body":{{"type":"from-a-newer-build","n":1}}}}"#);
    s.st.store.lock().unwrap().connection().execute(
        "INSERT INTO events (host, host_seq, at_ms, subject, json) VALUES ('test', 1, 1, ?1, ?2)",
        rusqlite::params![format!("seat:{agent}"), future],
    ).unwrap();
    let (status, file, _) = text(&s.router, &format!("/api/teams/{id}/deployments/{seq}/session")).await;
    assert_eq!(status, StatusCode::OK, "{file}");
    assert!(file.lines().any(|l| l == format!(r#"{{"origin":"observed","event":{future}}}"#)), "kept exactly as recorded");
}

#[tokio::test]
async fn a_run_whose_team_cannot_be_imported_says_why() {
    let s = serve().await;
    let (id, seq, _, _host) = ran(&s).await;
    let (_, original, _) = text(&s.router, &format!("/api/teams/{id}/deployments/{seq}/session")).await;
    let mut header: serde_json::Value = serde_json::from_str(original.lines().next().unwrap()).unwrap();
    header["team"]["connections"] = json!([{ "id": "c-x", "from": "a", "to": "ghost", "kind": "handoff" }]);
    let file = std::iter::once(header.to_string()).chain(original.lines().skip(1).map(str::to_string)).collect::<Vec<_>>().join("\n");
    let (status, imported) = post_text(&s.router, "/api/runs/import", &file).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let codes: Vec<&str> = imported["warnings"].as_array().unwrap().iter().filter_map(|w| w["code"].as_str()).collect();
    assert!(codes.contains(&"run-team-left-out") && imported.to_string().contains("ghost"), "the reason the team was left out: {imported}");
}

fn claude_fixture(path: &str) -> controld::portable::convert::Upload {
    let full = format!("{}/tests/fixtures/sessions/claude/{path}", env!("CARGO_MANIFEST_DIR"));
    controld::portable::convert::Upload { name: path.rsplit('/').next().unwrap().into(), text: std::fs::read_to_string(full).unwrap() }
}

#[tokio::test]
async fn a_converted_session_whose_folder_is_not_allowed_imports_with_a_warning() {
    let s = serve().await;
    let c = controld::portable::convert::convert(&[claude_fixture("main.jsonl")]).unwrap();
    let (status, body) = post_text(&s.router, "/api/runs/import", &c.session).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["team"]["draft"]["agents"].as_array().unwrap().len(), 1);
    let warnings = body["warnings"].to_string();
    assert!(warnings.contains("/work/app") || warnings.to_lowercase().contains("folder"), "{warnings}");
}

#[tokio::test]
async fn a_claude_session_converts_imports_and_downloads_unchanged() {
    let s = serve().await;
    let files: Vec<_> = ["main-with-agents.jsonl", "subagents/agent-a1.jsonl", "subagents/agent-a1.meta.json"]
        .iter()
        .map(|p| { let u = claude_fixture(p); json!({ "name": u.name, "text": u.text }) })
        .collect();
    let (status, c) = call(&s.router, "POST", "/api/runs/convert", json!({ "files": files })).await;
    assert_eq!(status, StatusCode::OK, "{c}");
    assert_eq!(c["preview"]["subagents"], 3);
    assert_eq!(c["preview"]["source"], "imported:claude-code");
    let session = c["session"].as_str().unwrap();
    let (status, imported) = post_text(&s.router, "/api/runs/import", session).await;
    assert_eq!(status, StatusCode::OK, "{imported}");
    let team = imported["team"]["draft"]["id"].as_str().unwrap();
    let seq = imported["run"]["start_seq"].as_i64().unwrap();
    let (_, detail) = get(&s.router, &format!("/api/teams/{team}/deployments/{seq}")).await;
    assert_eq!(detail["agents"].as_array().unwrap().len(), 4, "{detail}");
    let (_, back, _) = text(&s.router, &format!("/api/teams/{team}/deployments/{seq}/session")).await;
    assert_eq!(back.lines().skip(1).collect::<Vec<_>>(), session.lines().skip(1).collect::<Vec<_>>());
    assert_eq!(imported["team"]["draft"]["agents"].as_array().unwrap().len(), 3);
    assert_eq!(live_events(&s), live_events(&s), "nothing enters the live log");
}

#[tokio::test]
async fn an_unreadable_upload_says_so_and_creates_nothing() {
    let s = serve().await;
    let before = team_count(&s).await;
    let (status, body) = call(&s.router, "POST", "/api/runs/convert", json!({ "files": [{ "name": "x.jsonl", "text": "nope" }] })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(team_count(&s).await, before);
}

#[test]
fn fifty_thousand_lines_convert_quickly() {
    let mut text = String::new();
    for i in 0..25_000 {
        text.push_str(&format!("{{\"type\":\"user\",\"sessionId\":\"s\",\"timestamp\":\"2026-10-01T10:00:00Z\",\"message\":{{\"role\":\"user\",\"content\":\"p{i}\"}}}}\n"));
        text.push_str(&format!("{{\"type\":\"assistant\",\"sessionId\":\"s\",\"timestamp\":\"2026-10-01T10:00:01Z\",\"message\":{{\"id\":\"m{i}\",\"role\":\"assistant\",\"model\":\"m\",\"content\":[{{\"type\":\"text\",\"text\":\"ok\"}}]}}}}\n"));
    }
    let t = std::time::Instant::now();
    controld::portable::convert::convert(&[controld::portable::convert::Upload { name: "big.jsonl".into(), text }]).unwrap();
    assert!(t.elapsed() < std::time::Duration::from_secs(5), "{:?}", t.elapsed());
}
