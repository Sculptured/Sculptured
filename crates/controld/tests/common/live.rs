//! Live tests: agents run on the real Claude Code CLI with its cheapest model, so what is tested is
//! a real model deployment, variance included. Each live test is named `live_…` and marked
//! `#[ignore = "live: …"]` (it costs credits and needs `claude` installed and signed in); run them with
//!
//!     cargo test -p controld --test <file> -- --ignored live_
//!
//! A live test asserts on what Sculptured records and enforces (sessions launched, turns recorded,
//! tool calls captured, questions raised, permissions held, statuses, limits), never on the model's
//! exact words or paths. When the CLI is missing, `require()` prints why and the test returns.
#![allow(dead_code)]
use super::m2::{Served, call, rid, start_team};
use axum::http::StatusCode;
use drivers::Driver;
use drivers::claude::ClaudeStreamDriver;
use drivers::codex::CodexAppServerDriver;
use hostd::supervisor::{SeatToolsHost, Supervisor};
use model::{EventBody, EventRecord};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The model every live Claude agent runs with: the cheapest one.
pub const CLAUDE_MODEL: &str = "haiku";

/// How long a live wait lasts: a real model turn takes seconds to a minute or two.
pub const WAIT: Duration = Duration::from_secs(240);

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// The `claude` CLI, or None (with the reason printed) when this machine cannot run live tests.
pub fn require() -> Option<PathBuf> {
    let found = on_path("claude");
    if found.is_none() {
        eprintln!(
            "skipped: the claude CLI is not on PATH; live tests need it installed and signed in"
        );
    }
    found
}

/// The `codex` CLI, or None (with the reason printed).
pub fn require_codex() -> Option<PathBuf> {
    let found = on_path("codex");
    if found.is_none() {
        eprintln!(
            "skipped: the codex CLI is not on PATH; live Codex tests need it installed and signed in"
        );
    }
    found
}

/// The drivers a live host offers: the real Claude Code and Codex CLIs.
pub fn drivers() -> Vec<Arc<dyn Driver>> {
    let claude = ClaudeStreamDriver {
        program: on_path("claude").unwrap_or_else(|| "claude".into()),
    };
    let codex = CodexAppServerDriver {
        program: on_path("codex").unwrap_or_else(|| "codex".into()),
        native_questions: false,
    };
    vec![Arc::new(claude), Arc::new(codex)]
}

/// A host on this computer with the real CLIs.
pub async fn connect(s: &Served) -> Supervisor {
    super::m2::connect(s, drivers(), None).await
}

/// `<home>/work`, created: a folder inside the served home folder for agents to work in.
pub fn work_folder(s: &Served) -> String {
    let dir = s.home.path().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    dir.to_string_lossy().into_owned()
}

/// Every agent of `draft` becomes a Claude agent on the cheapest model, working in `work_folder`.
pub fn make_live(s: &Served, draft: &mut Value) {
    let folder = work_folder(s);
    draft["folder"] = json!(folder);
    draft["models"] = json!({ "claude": CLAUDE_MODEL });
    for a in draft["agents"].as_array_mut().unwrap() {
        a["kind"] = json!("claude");
    }
}

async fn save(s: &Served, draft: &Value) {
    let id = draft["id"].as_str().unwrap();
    let (status, body) = call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// A team from built-in template `template` with every agent made live (`make_live`), started;
/// returns its id once every agent runs. Agents start on their template jobs.
pub async fn team(s: &Served, template: &str) -> String {
    let (status, rec) = call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": template }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    make_live(s, &mut draft);
    save(s, &draft).await;
    let id = draft["id"].as_str().unwrap().to_string();
    start_team(s, &id).await;
    id
}

/// Waits up to `WAIT` for `pred`.
pub async fn wait_for(what: &str, pred: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + WAIT;
    while !pred() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// The first event on `seat` matching `pred`, waiting up to `WAIT`.
pub async fn wait_event(
    st: &controld::AppState,
    seat: &str,
    what: &str,
    pred: impl Fn(&EventBody) -> bool,
) -> EventRecord {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        if let Some(e) = super::m2::events(st, seat)
            .into_iter()
            .find(|e| pred(&e.body))
        {
            return e;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Waits until `seat` has finished at least `n` turns (TurnEnded events), whatever their status.
pub async fn wait_turns(st: &controld::AppState, seat: &str, n: usize) {
    wait_for(&format!("{n} finished turn(s) on {seat}"), || {
        super::m2::events(st, seat)
            .iter()
            .filter(|e| matches!(e.body, EventBody::TurnEnded { .. }))
            .count()
            >= n
    })
    .await;
}

/// The `sculptured` binary (its `seat-tools` subcommand is the MCP server a CLI starts for its agent),
/// built if needed, from this test binary's target folder.
pub fn sculptured_bin() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let bin = exe.parent().unwrap().parent().unwrap().join("sculptured");
    if !bin.exists() {
        let ok = std::process::Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", "sculptured", "--bin", "sculptured"])
            .status()
            .unwrap()
            .success();
        assert!(ok, "cannot build the sculptured binary");
    }
    bin
}

/// A served controld with a live host (the real CLIs) whose agents reach Sculptured's tools through
/// the real shim and socket. Keep the returned host and folder for the test's length.
pub async fn with_tools() -> (Served, Supervisor, tempfile::TempDir) {
    let s = super::m2::serve().await;
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("seat-tools.sock");
    let host = SeatToolsHost {
        socket: socket.clone(),
        shim: sculptured_bin(),
    };
    let sup = super::m2::connect(&s, drivers(), Some(host)).await;
    tokio::spawn(hostd::seat_tools::serve_socket(sup.clone(), socket.clone()));
    super::wait_for("the socket", || socket.exists()).await;
    (s, sup, dir)
}
