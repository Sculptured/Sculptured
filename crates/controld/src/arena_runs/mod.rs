//! Run in Arena: a team's board runs in Arena as an external run. Sculptured starts the run
//! (`runner: "external"`), registers each board agent, and runs the board's own team (a copy of the
//! design, each agent given its Arena MCP endpoint, the agents nobody hands work to given Arena's task)
//! the way it runs any team; Arena records and scores. `drive` passes usage to Arena and finishes the
//! run. The run then shows in Control like any Arena run (`/arena/runs/<run>/<branch>`).
//!
//! - `POST /api/teams/{id}/arena-runs` `{request_id, task_id, settle_seconds?}` starts one;
//! - `GET /api/teams/{id}/arena-runs` lists the ones started from that board since Sculptured started;
//! - `POST /api/arena-runs/{branch}/finish` asks one to finish now.
mod client;
mod drive;
pub mod plan;

use crate::arena::Arena;
use crate::error::{ApiError, respond};
use crate::extract::{Json, Path};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use axum::routing::post;
pub use drive::{Drive, DriveState, Drives};
use model::TeamDraft;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// How long every agent must stay finished (idle, nothing waiting) before the run is finished.
pub const DEFAULT_SETTLE_SECONDS: u64 = 30;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/arena-runs", post(start).get(list))
        .route("/api/arena-runs/{branch}/finish", post(finish))
}

#[derive(Deserialize, Serialize)]
pub(crate) struct StartReq {
    #[serde(default)]
    request_id: String,
    task_id: String,
    #[serde(default)]
    settle_seconds: Option<u64>,
}

fn arena_of(st: &AppState) -> Result<Arc<Arena>, ApiError> {
    st.arena.clone().ok_or_else(crate::hosted::needs_computer)
}

async fn start(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<StartReq>,
) -> Response {
    let fp = crate::api::fingerprint(&format!("arena-run:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::api::idempotent(&st, &request_id, fp, move || async move {
        start_run(&st2, &id, req).await
    })
    .await
}

/// The board's agents registered on a new external run, and their MCP endpoints.
async fn open_run(
    arena: &Arena,
    board: &TeamDraft,
    task: &str,
) -> Result<(client::Started, Vec<(String, String)>), ApiError> {
    let started = client::start(arena, task, &board.id, &board.name).await?;
    let mut urls = Vec::new();
    for a in &board.agents {
        match client::register(arena, &started.branch, &plan::registration(board, a)).await {
            Ok(url) => urls.push((a.id.clone(), url)),
            Err(e) => {
                let _ = client::finish(arena, &started.branch).await;
                return Err(e);
            }
        }
    }
    Ok((started, urls))
}

/// Stores the run's team and starts it as Start does; returns where its events begin.
async fn start_team(st: &AppState, draft: &TeamDraft, request_id: &str) -> Result<i64, ApiError> {
    let after = {
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        store.insert_draft(draft)?;
        store.last_seq()?
    };
    let rid = format!("{request_id}:team");
    crate::teams::start_for_event(st, &draft.id, rid)
        .await
        .map_err(|why| ApiError::conflict("arena_team_not_started", why))?;
    Ok(after + 1)
}

/// The run team's id and name: after the board, the task and the branch.
fn run_names(st: &AppState, board: &TeamDraft, task: &str, branch: &str) -> (String, String) {
    let short: String = branch
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(6)
        .collect();
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let id = crate::teams::new_team_id(&store, &format!("{} arena {short}", board.name));
    (
        id,
        format!("{} · Shakedown task {task} ({short})", board.name),
    )
}

/// The board to run (it must have agents) and Arena's task.
async fn board_and_task(
    st: &AppState,
    arena: &Arena,
    board_id: &str,
    task: &str,
) -> Result<(TeamDraft, plan::Task), ApiError> {
    let board = crate::teams::load(st, board_id)?;
    if board.agents.is_empty() {
        return Err(ApiError::conflict(
            "team_empty",
            "This team has no agents to run in Shakedown.",
        ));
    }
    Ok((
        board,
        plan::Task::from_json(&client::task(arena, task).await?),
    ))
}

/// Starts driving the run whose team began at event `from`; the answer names the run, its team and agents.
fn begin_drive(
    st: &AppState,
    arena: Arc<Arena>,
    d: Arc<Drive>,
    from: i64,
    settle: Duration,
) -> Value {
    arena.drives.insert(d.clone());
    let agents: Vec<Value> = d
        .agents
        .iter()
        .map(|(seat, agent)| json!({ "agent": agent, "seat": seat }))
        .collect();
    let answer =
        json!({ "run_id": d.run, "branch_id": d.branch, "team": d.team, "agents": agents });
    tokio::spawn(drive::run(st.clone(), arena, d, from, settle));
    answer
}

async fn start_run(st: &AppState, board_id: &str, req: StartReq) -> Result<Value, ApiError> {
    let arena = arena_of(st)?;
    let (board, task) = board_and_task(st, &arena, board_id, &req.task_id).await?;
    let (started, urls) = open_run(&arena, &board, &task.id).await?;
    let (id, name) = run_names(st, &board, &task.id, &started.branch);
    let draft = plan::run_draft(&board, &id, &name, &urls, &task);
    let from = match start_team(st, &draft, &req.request_id).await {
        Ok(from) => from,
        Err(e) => {
            let _ = client::finish(&arena, &started.branch).await;
            return Err(e);
        }
    };
    let agents = draft
        .agents
        .iter()
        .filter_map(|a| Some((draft.seat_id(&a.id)?, a.id.clone())))
        .collect();
    let d = Drive::new(started, id, board.id.clone(), task.id.clone(), agents);
    let settle = Duration::from_secs(req.settle_seconds.unwrap_or(DEFAULT_SETTLE_SECONDS));
    Ok(begin_drive(st, arena, d, from, settle))
}

fn drive_json(d: &Drive) -> Value {
    json!({ "run_id": d.run, "branch_id": d.branch, "team": d.team, "task_id": d.task, "state": d.state() })
}

async fn list(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    respond(arena_of(&st).map(|a| json!({ "runs": a.drives.of_board(&id).iter().map(|d| drive_json(d)).collect::<Vec<_>>() })))
}

async fn finish(State(st): State<AppState>, Path(branch): Path<String>) -> Response {
    respond(arena_of(&st).and_then(|a| {
        let d = a
            .drives
            .get(&branch)
            .ok_or_else(|| ApiError::not_found("Sculptured is not driving that Shakedown run"))?;
        d.ask_finish();
        Ok(drive_json(&d))
    }))
}
