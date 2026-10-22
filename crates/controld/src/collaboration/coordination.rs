use super::{
    ApiError, AppState, Control, Deserialize, Json, Path, Response, Router, Serialize, State,
    budgets, controls, history, json,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(history::routes())
        .route(
            "/api/teams/{id}/budget",
            crate::collaboration::get(budget_status).post(set_budget),
        )
        .route(
            "/api/teams/{id}/stop-now",
            crate::collaboration::post(stop_team_now),
        )
        .route(
            "/api/teams/{id}/archive",
            crate::collaboration::post(archive),
        )
        .route(
            "/api/teams/{id}/communications",
            crate::collaboration::get(events),
        )
        .route(
            "/api/teams/{id}/children",
            crate::collaboration::get(children),
        )
        .route(
            "/api/seats/{id}/control",
            crate::collaboration::post(control).get(control_status),
        )
}
#[derive(Serialize, Deserialize)]
pub(super) struct Archive {
    pub(super) request_id: String,
    #[serde(default = "yes")]
    pub(super) archived: bool,
}
pub(super) fn yes() -> bool {
    true
}
pub(super) async fn archive(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<Archive>,
) -> Response {
    let fp = crate::collaboration::fingerprint(&format!("archive:{id}"), &req);
    let st2 = st.clone();
    crate::collaboration::idempotent(&st, &req.request_id, fp, move || async move {
        crate::teams::load(&st2, &id)?;
        let _claim = st2.try_begin_boot(&id).ok_or_else(|| {
            ApiError::conflict("team_starting", "Wait for project changes to finish")
        })?;
        let started = st2
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .rig_snapshot(&id)?
            .is_some();
        if req.archived && started {
            let failed = crate::reconciler::stop_rig(st2.clone(), id.clone()).await?;
            if !failed.is_empty() {
                return Err(ApiError::conflict(
                    "stop_failed",
                    "Some agents could not stop. Project was not archived.",
                ));
            }
        }
        st2.store
            .lock()
            .expect("store mutex must not be poisoned")
            .set_archived(&id, req.archived)?;
        Ok(json!({"archived":req.archived,"id":id}))
    })
    .await
}
#[derive(Deserialize)]
pub(super) struct CommunicationsQuery {
    #[serde(default)]
    pub(super) usage: bool,
    /// `latest`: only the team's latest run (the Run board's arrows need no more).
    #[serde(default)]
    pub(super) run: Option<String>,
    /// Long text (each handoff carries the team briefing) cut, for a view that only counts messages.
    #[serde(default)]
    pub(super) brief: bool,
}
pub(super) async fn events(
    State(st): State<AppState>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<CommunicationsQuery>,
) -> Response {
    let read = || -> rusqlite::Result<Vec<serde_json::Value>> {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let after = if q.run.as_deref() == Some("latest") {
            store.latest_run_start(&id)?.map_or(0, |s| s - 1)
        } else {
            0
        };
        let events = store.communication_events(&id, 10000, q.usage, after)?;
        Ok(events
            .into_iter()
            .map(|e| serde_json::to_value(e).expect("events serialize"))
            .map(|v| if q.brief { crate::recent::brief(v) } else { v })
            .collect())
    };
    crate::collaboration::respond(read().map_err(ApiError::from))
}
#[derive(Serialize, Deserialize)]
pub(super) struct ControlBody {
    pub(super) request_id: String,
    pub(super) mode: String,
}
pub(super) async fn control(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ControlBody>,
) -> Response {
    let fp = crate::collaboration::fingerprint(&format!("control:{id}"), &req);
    let st2 = st.clone();
    crate::collaboration::idempotent(&st, &req.request_id, fp, move || async move {
        crate::collaboration::control_agent(&st2, &id, Control::parse(&req.mode)?).await?;
        Ok(json!({"mode":req.mode}))
    })
    .await
}

pub(super) async fn control_status(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::collaboration::respond(
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .control(&id)
            .map(|mode| json!({"mode":mode}))
            .map_err(ApiError::from),
    )
}

#[derive(Serialize, ts_rs::TS)]
#[ts(export)]
pub struct ManagedChild {
    pub parent: String,
    pub child: String,
}
pub(super) async fn children(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::collaboration::respond((|| -> Result<Vec<ManagedChild>, ApiError> {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let Some(snapshot) = store.rig_snapshot(&id)? else {
            return Ok(vec![]);
        };
        let mut out = vec![];
        for seat in snapshot.rig.seats {
            if let Some(parent) = store.parent(&seat.id)? {
                out.push(ManagedChild {
                    parent,
                    child: seat.id,
                });
            }
        }
        Ok(out)
    })())
}

#[derive(Serialize, Deserialize)]
pub(super) struct StopNowRequest {
    pub(super) request_id: String,
}

/// Stop everything now: every running agent of the team and everything it started, at once, like Ctrl+C.
pub(super) async fn stop_team_now(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<StopNowRequest>,
) -> Response {
    let fp = crate::collaboration::fingerprint(&format!("stop-now:{id}"), &req);
    let st2 = st.clone();
    crate::collaboration::idempotent(&st, &req.request_id, fp, move || async move {
        let roots: Vec<String> = {
            let store = st2.store.lock().expect("store mutex must not be poisoned");
            let Some(snap) = store.rig_snapshot(&id)? else {
                return Err(ApiError::conflict(
                    "team_not_started",
                    "This team has not been started yet.",
                ));
            };
            let live: Vec<String> = snap
                .seats
                .into_iter()
                .filter(|s| crate::reconciler::stoppable(s.lifecycle))
                .map(|s| s.id)
                .collect();
            live.iter()
                .filter(|s| {
                    store
                        .parent(s)
                        .ok()
                        .flatten()
                        .is_none_or(|p| !live.contains(&p))
                })
                .cloned()
                .collect()
        };
        if !roots.is_empty() {
            controls::stop_now(&st2, &id, &roots).await?;
        }
        crate::analytics::emit(&st2, "team.stopped_now", json!({}));
        Ok(json!({ "stopped": roots.len() }))
    })
    .await
}

pub(super) async fn budget_status(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::collaboration::respond(budgets::status(&st, &id))
}
#[derive(Serialize, Deserialize)]
pub(super) struct BudgetRequest {
    pub(super) request_id: String,
    pub(super) output_limit: Option<u32>,
}
pub(super) async fn set_budget(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<BudgetRequest>,
) -> Response {
    let fp = crate::collaboration::fingerprint(&format!("budget:{id}"), &req);
    let st2 = st.clone();
    crate::collaboration::idempotent(&st, &req.request_id, fp, move || async move {
        crate::teams::load(&st2, &id)?;
        let _claim = st2
            .try_begin_boot(&id)
            .ok_or_else(|| ApiError::conflict("team_starting", "Project is changing"))?;
        let store = st2.store.lock().expect("store mutex must not be poisoned");
        if store
            .rig_snapshot(&id)?
            .is_some_and(|s| s.seats.iter().any(|a| crate::store::is_live(a.lifecycle)))
        {
            return Err(ApiError::conflict(
                "team_running",
                "Stop the project before changing its budget",
            ));
        }
        if let Some(limit) = req.output_limit {
            if !(1..=1_000_000).contains(&limit) {
                return Err(ApiError::bad_request(
                    "invalid_budget",
                    "Choose 1–1,000,000 output tokens per agent",
                ));
            }
            store.connection().execute(
                "INSERT OR REPLACE INTO team_output_budgets VALUES (?1,?2)",
                rusqlite::params![id, limit],
            )?;
        } else {
            store
                .connection()
                .execute("DELETE FROM team_output_budgets WHERE rig=?1", [id])?;
        }
        Ok(json!({"output_limit":req.output_limit}))
    })
    .await
}
