//! Where a run's tokens went: MAIS over the session logs a real Claude Code agent keeps (live).
mod common;
use axum::http::StatusCode;
use common::m2::{Served, rid};
use serde_json::{Value, json};
use std::path::PathBuf;

async fn team(s: &Served, job: &str) -> String {
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Report" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([{ "id": "a", "group_id": null, "name": "Alpha", "kind": "claude", "job": job, "folder": null, "x": 40, "y": 40 }]);
    draft["connections"] = json!([]);
    common::live::make_live(s, &mut draft);
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

/// The report once it holds `calls` model calls.
async fn report_with(s: &Served, id: &str, calls: u64) -> Value {
    let deadline = tokio::time::Instant::now() + common::live::WAIT;
    loop {
        let (status, body) =
            crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
        if status == StatusCode::OK && body["report"]["run"]["calls"].as_u64() >= Some(calls) {
            return body;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no report with {calls} calls: {status} {body}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// The agents' own Claude Code setup in a folder of this test: `CLAUDE_CONFIG_DIR` is set to it, so the
/// real CLI writes its session logs there and the report reads them from there. Only the sign-in
/// (`.credentials.json`) is copied in from the person's setup; their session logs are never read.
/// Without a credentials file (an API key in the environment instead) nothing is copied.
fn own_claude_setup(dir: &std::path::Path) {
    let theirs = std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(
        || PathBuf::from(std::env::var_os("HOME").expect("HOME is set")).join(".claude"),
        PathBuf::from,
    );
    let ours = dir.join(".claude");
    std::fs::create_dir_all(&ours).unwrap();
    let credentials = theirs.join(".credentials.json");
    if credentials.is_file() {
        std::fs::copy(&credentials, ours.join(".credentials.json")).unwrap();
    }
    // SAFETY: this binary has one test, on a single-threaded runtime, and it calls this first.
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", &ours) };
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_runs_report_names_its_agents_and_counts_every_call() {
    let Some(_) = common::live::require() else {
        return;
    };
    let setup = tempfile::tempdir().unwrap();
    own_claude_setup(setup.path());
    let s = crate::common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let id = team(
        &s,
        "Use your Bash tool to run exactly: echo hi\nThen reply with the single word OK.",
    )
    .await;
    let (status, body) =
        crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no run yet: {body}");
    crate::common::m2::start_team(&s, &id).await;
    common::live::wait_turns(&s.st, &crate::common::m2::seat(&id, "a"), 1).await;
    let body = report_with(&s, &id, 1).await;
    let r = &body["report"];
    assert_eq!(r["format"], "mais.report/v1");
    assert_eq!(
        r["agents"][0]["name"], "Alpha",
        "the session is linked to its team member"
    );
    assert!(
        r["run"]["usage"]["output"].as_u64() > Some(0),
        "the model's own token counts are read from its session log: {r}"
    );
    assert_eq!(
        body["start_seq"].as_i64(),
        s.st.store.lock().unwrap().latest_run_start(&id).unwrap()
    );
    let kept: i64 =
        s.st.store
            .lock()
            .unwrap()
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM run_reports WHERE rig=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(kept, 1, "the report is kept for the run");
    assert_eq!(body["running"], true);
    // A budget's hard stop ends every agent without a Stop of the team (its desired state stays running).
    s.st.store
        .lock()
        .unwrap()
        .connection()
        .execute("UPDATE seats SET lifecycle='stopped' WHERE rig=?1", [&id])
        .unwrap();
    let (_, done) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
    assert_eq!(
        done["running"], false,
        "no agent is left, though nobody pressed Stop: {done}"
    );
    let (_, again) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/report")).await;
    assert_eq!(
        again["at_ms"], done["at_ms"],
        "a finished run's report is kept, not made again"
    );
    crate::common::m2::stop_team(&s, &id).await;
}
