//! Tests: Run in Arena. A board team runs in Arena as an external run: Sculptured starts the run,
//! registers each board agent, runs the board's own team with each agent's Arena MCP endpoint, passes
//! usage on, and finishes the run once every agent has finished. Arena here is a fake that records
//! what it is told. Runs are live: the board's agents run on the real Claude Code CLI on the cheapest model.
mod common;
use axum::Router;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use controld::arena::Arena;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// The task's instructions: they leave the work to the agent's job (cheap for a live agent).
const TASK_INSTRUCTIONS: &str = "Do exactly what your job says.";

/// The board agents' jobs: cheap for live agents.
const COORD_JOB: &str = "Coordinate. Reply with one short sentence.";
const QUALITY_JOB: &str = "Check quality. Reply with one short sentence.";

/// What the fake arenad was told, in order: `(route, body)`.
type Calls = Arc<Mutex<Vec<(String, Value)>>>;

fn record(calls: &Calls, route: String, body: Value) {
    calls.lock().unwrap().push((route, body));
}

/// A fake arenad for external runs on a free loopback port: its base URL and what it was told.
async fn fake_arenad() -> (String, Calls) {
    let calls: Calls = Arc::default();
    let tasks = json!([{ "id": "T1", "reason": "Batch failed", "ticket": "Batch CVX-2611 failed its EM limit.", "instructions": TASK_INSTRUCTIONS }]);
    let app = Router::new()
        .route("/api/tasks", get(move || async move { axum::Json(tasks) }))
        .route("/api/runs", post(|State(c): State<Calls>, axum::Json(b): axum::Json<Value>| async move {
            record(&c, "runs".into(), b);
            axum::Json(json!({ "run_id": "run-1", "branch_id": "br-1", "agents": [] }))
        }))
        .route("/api/branches/{b}/agents", post(|State(c): State<Calls>, Path(b): Path<String>, axum::Json(body): axum::Json<Value>| async move {
            let id = body["id"].as_str().unwrap_or_default().to_string();
            record(&c, format!("register {b}"), body);
            axum::Json(json!({ "agent": id, "mcp_url": format!("http://127.0.0.1:4510/mcp/{b}/{id}"), "gateway_url": "", "tools": [], "spec": {} }))
        }))
        .route("/api/branches/{b}/agents/{a}/{what}", post(|State(c): State<Calls>, Path((b, a, what)): Path<(String, String, String)>, axum::Json(body): axum::Json<Value>| async move {
            record(&c, format!("{what} {b} {a}"), body);
            axum::Json(json!({}))
        }))
        .route("/api/branches/{b}/finish", post(|State(c): State<Calls>, Path(b): Path<String>| async move {
            record(&c, format!("finish {b}"), Value::Null);
            axum::Json(json!({ "status": "passed", "error": null }))
        }))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), calls)
}

fn told(calls: &Calls, prefix: &str) -> Vec<Value> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(r, _)| r.starts_with(prefix))
        .map(|(_, b)| b.clone())
        .collect()
}

/// The board's first driven run's state once it is no longer driving or finishing (live agents: at
/// most 240 s).
async fn settled_state(s: &common::m2::Served, board: &str) -> Value {
    for _ in 0..4800 {
        let (_, list) = common::m2::get(&s.router, &format!("/api/teams/{board}/arena-runs")).await;
        let state = list["runs"][0]["state"].clone();
        if state["state"] != "driving" && state["state"] != "finishing" {
            return state;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the run did not finish");
}

/// A served controld that knows the fake arenad; with a live host when `live`.
async fn served(live: bool) -> (common::m2::Served, Calls) {
    let (base, calls) = fake_arenad().await;
    let mut s = common::m2::serve().await;
    s.st = s.st.clone().with_arena(Arena::new(&base));
    s.router = controld::app(s.st.clone(), None);
    if live {
        common::live::connect(&s).await;
    }
    (s, calls)
}

/// The run's team: each agent has its own Arena endpoint; only the agent nobody hands work to gets the task.
fn check_run_team(s: &common::m2::Served, team: &str) {
    let d = s.st.store.lock().unwrap().draft(team).unwrap().unwrap();
    let urls: Vec<_> = d
        .components
        .iter()
        .map(|c| (c.id.as_str(), c.url.as_deref()))
        .collect();
    let base = "http://127.0.0.1:4510/mcp/br-1";
    let want = [
        ("arena-coord", format!("{base}/coord")),
        ("arena-quality", format!("{base}/quality")),
    ];
    assert_eq!(
        urls,
        want.iter()
            .map(|(id, u)| (*id, Some(u.as_str())))
            .collect::<Vec<_>>()
    );
    check_jobs(&d);
}

/// Each agent is given its endpoint; the agent nobody hands work to gets the task after its job.
fn check_jobs(d: &model::TeamDraft) {
    let coord = d.agent("coord").unwrap();
    assert!(
        coord
            .advanced
            .integrations
            .contains(&"arena-coord".to_string())
    );
    let job = &coord.job;
    assert!(
        job.starts_with(COORD_JOB)
            && job.contains("failed its EM limit")
            && job.contains(TASK_INSTRUCTIONS),
        "{job}"
    );
    assert_eq!(d.agent("quality").unwrap().job, QUALITY_JOB);
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn live_a_board_runs_in_arena_as_an_external_run_and_is_finished_when_its_agents_are() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, calls) = served(true).await;
    let board = common::m2::chain(
        &s,
        "Larchmont",
        &[
            ("coord", "Coordinator", COORD_JOB),
            ("quality", "Quality", QUALITY_JOB),
        ],
        &[("coord", "quality")],
    )
    .await;
    let (status, v) = common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{board}/arena-runs"),
        json!({ "request_id": common::m2::rid(), "task_id": "T1", "settle_seconds": 1 }),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{v}");
    assert_eq!(
        (v["run_id"].as_str(), v["branch_id"].as_str()),
        (Some("run-1"), Some("br-1"))
    );
    let started = told(&calls, "runs");
    assert_eq!(
        started,
        [
            json!({ "task_id": "T1", "runner": "external", "team": { "id": board, "name": "Larchmont", "agents": [] } })
        ]
    );
    let registered = told(&calls, "register br-1");
    assert_eq!(
        registered
            .iter()
            .map(|r| (
                r["id"].as_str().unwrap(),
                r["kind"].as_str().unwrap(),
                r["prompt"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("coord", "external", COORD_JOB),
            ("quality", "external", QUALITY_JOB)
        ]
    );
    let team = v["team"].as_str().unwrap().to_string();
    check_run_team(&s, &team);

    let state = settled_state(&s, &board).await;
    assert_eq!(
        state,
        json!({ "state": "finished", "why": "every agent finished its turns and nothing waited on it", "status": "passed", "error": null })
    );
    assert_eq!(told(&calls, "finish br-1").len(), 1);
    let ends = told(&calls, "end br-1");
    assert_eq!(ends.len(), 2);
    assert!(
        ends.iter()
            .all(|e| e["ok"] == true && e["prompt_tokens"].as_u64().is_some()),
        "{ends:?}"
    );
    assert!(
        !told(&calls, "usage br-1").is_empty(),
        "each usage report goes to Arena"
    );
    let seat = common::m2::seat(&team, "coord");
    common::live::wait_for("the run's team is stopped", || {
        common::lifecycle(&s.st, &seat).is_some_and(|l| !controld::store::is_live(l))
    })
    .await;
}

#[tokio::test]
async fn an_unknown_task_is_refused() {
    let (s, calls) = served(false).await;
    let board = common::m2::chain(&s, "Solo", &[("coord", "Coordinator", COORD_JOB)], &[]).await;
    let (status, v) = common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{board}/arena-runs"),
        json!({ "request_id": common::m2::rid(), "task_id": "nope" }),
    )
    .await;
    assert_eq!(
        (status.as_u16(), v["error"]["code"].as_str()),
        (400, Some("arena_task_unknown"))
    );
    assert!(told(&calls, "runs").is_empty());
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_finish_asks_the_run_to_finish_now() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, calls) = served(true).await;
    let hang = "Use your Bash tool to run exactly: sleep 600";
    let board = common::m2::chain(&s, "Solo", &[("coord", "Coordinator", hang)], &[]).await;
    let (status, v) = common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{board}/arena-runs"),
        json!({ "request_id": common::m2::rid(), "task_id": "T1" }),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{v}");
    let seat = common::m2::seat(v["team"].as_str().unwrap(), "coord");
    common::live::wait_for("the coordinator working", || {
        common::m2::events(&s.st, &seat)
            .iter()
            .any(|e| matches!(e.body, model::EventBody::TurnStarted { .. }))
    })
    .await;
    let (status, _) =
        common::m2::call(&s.router, "POST", "/api/arena-runs/br-1/finish", json!({})).await;
    assert_eq!(status.as_u16(), 200);
    assert_eq!(
        settled_state(&s, &board).await["why"],
        "you asked to finish the run"
    );
    assert_eq!(told(&calls, "finish br-1").len(), 1);
}
