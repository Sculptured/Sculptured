//! arenad's external-run API as Run in Arena calls it (arena `docs/reference.md`): start a run with
//! `runner: "external"`, register agents, report usage and ends, finish. Errors come back as
//! Sculptured's (`arena::arena_error`), with arenad's own words as evidence.
use crate::arena::{Arena, arena_error, unreachable_arena};
use crate::error::ApiError;
use axum::http::StatusCode;
use serde_json::{Value, json};

/// A started external run: its run and its (first) branch.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    pub run: String,
    pub branch: String,
}

async fn call(
    arena: &Arena,
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, ApiError> {
    let mut req = arena.http().request(method, arena.api_url(path));
    if let Some(b) = body {
        req = req.json(b);
    }
    let res = req.send().await.map_err(|_| unreachable_arena())?;
    let status = StatusCode::from_u16(res.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let text = res.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(arena_error(status, &text));
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// The benchmark's task `id` (`GET /api/tasks`), or a refusal naming it.
pub async fn task(arena: &Arena, id: &str) -> Result<Value, ApiError> {
    let tasks = call(arena, reqwest::Method::GET, "tasks", None).await?;
    tasks
        .as_array()
        .and_then(|ts| ts.iter().find(|t| t["id"] == id).cloned())
        .ok_or_else(|| {
            ApiError::bad_request(
                "arena_task_unknown",
                format!("Shakedown has no task {id:?}."),
            )
        })
}

/// `POST /api/runs` with `runner: "external"` and no agents: Arena starts the world and records it.
pub async fn start(arena: &Arena, task: &str, team: &str, name: &str) -> Result<Started, ApiError> {
    let body = json!({ "task_id": task, "runner": "external", "team": { "id": team, "name": name, "agents": [] } });
    let v = call(arena, reqwest::Method::POST, "runs", Some(&body)).await?;
    match (v["run_id"].as_str(), v["branch_id"].as_str()) {
        (Some(run), Some(branch)) => Ok(Started {
            run: run.into(),
            branch: branch.into(),
        }),
        _ => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "arena_failed",
            "Shakedown started a run but did not say which",
        )),
    }
}

/// Registers one agent; its MCP endpoint (`mcp_url`) is where its tools are.
pub async fn register(arena: &Arena, branch: &str, agent: &Value) -> Result<String, ApiError> {
    let v = call(
        arena,
        reqwest::Method::POST,
        &format!("branches/{branch}/agents"),
        Some(agent),
    )
    .await?;
    v["mcp_url"].as_str().map(str::to_string).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "arena_failed",
            "Shakedown registered an agent without an MCP endpoint",
        )
    })
}

pub async fn usage(
    arena: &Arena,
    branch: &str,
    agent: &str,
    report: &Value,
) -> Result<(), ApiError> {
    call(
        arena,
        reqwest::Method::POST,
        &format!("branches/{branch}/agents/{agent}/usage"),
        Some(report),
    )
    .await
    .map(drop)
}

pub async fn end(arena: &Arena, branch: &str, agent: &str, end: &Value) -> Result<(), ApiError> {
    call(
        arena,
        reqwest::Method::POST,
        &format!("branches/{branch}/agents/{agent}/end"),
        Some(end),
    )
    .await
    .map(drop)
}

/// Freezes, scores and stops the world; Arena's `{status, error}`.
pub async fn finish(arena: &Arena, branch: &str) -> Result<Value, ApiError> {
    call(
        arena,
        reqwest::Method::POST,
        &format!("branches/{branch}/finish"),
        Some(&json!({})),
    )
    .await
}
