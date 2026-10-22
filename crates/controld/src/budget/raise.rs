//! Raising a limit of a running team, for this run only: the saved budget stays what was planned. A
//! run that wrapped up or stopped at that limit continues; the agents it stopped carry on.
use super::{BudgetState, Record};
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use model::{ContinueMode, EventBody, Provenance, SeatLifecycle, Subject};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/teams/{id}/budget/raise",
            axum::routing::post(raise_action),
        )
        .route(
            "/api/teams/{id}/agents/{agent}/limits/raise",
            axum::routing::post(raise_agent_action),
        )
}

#[derive(Serialize, Deserialize)]
struct RaiseRequest {
    request_id: String,
    /// `tokens`, `minutes` or `plan`.
    limit: String,
    /// The new hard value (tokens, minutes, or percent of the plan window); the soft value keeps its share.
    hard: f64,
    /// For `plan`: which window (needed only when the run has more than one).
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    window: Option<String>,
}

async fn raise_action(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<RaiseRequest>,
) -> Response {
    let fp = crate::api::fingerprint(&format!("budget-raise:{id}"), &req);
    let (st2, request_id) = (st.clone(), req.request_id.clone());
    crate::api::idempotent(&st, &request_id, fp, move || async move {
        raise(&st2, &id, &req)
            .await
            .map(|said| json!({ "said": said }))
    })
    .await
}

fn not_a_raise() -> ApiError {
    ApiError::bad_request("not_a_raise", "Choose a higher limit than the run has now.")
}

/// The soft value at the same share of the new hard value (absent stays absent: the default share).
fn scaled(soft: Option<f64>, old_hard: f64, new_hard: f64) -> Option<f64> {
    soft.map(|s| {
        if old_hard > 0.0 {
            s * new_hard / old_hard
        } else {
            s
        }
    })
}

/// Raises `req.limit` in the run's record; the words for the run's log.
fn apply(r: &mut Record, req: &RaiseRequest) -> Result<String, ApiError> {
    let none = || {
        ApiError::bad_request(
            "no_such_limit",
            "This run has no limit of that kind to raise.",
        )
    };
    match req.limit.as_str() {
        "tokens" => {
            let t = r.budget.tokens.as_mut().ok_or_else(none)?;
            let new = req.hard.round() as u64;
            if new <= t.hard {
                return Err(not_a_raise());
            }
            t.soft = scaled(t.soft.map(|s| s as f64), t.hard as f64, new as f64)
                .map(|s| s.round() as u64);
            t.hard = new;
            Ok(format!("Tokens raised to {new} for this run"))
        }
        "minutes" => {
            let m = r.budget.minutes.as_mut().ok_or_else(none)?;
            let new = req.hard.round() as u32;
            if new <= m.hard {
                return Err(not_a_raise());
            }
            m.soft = scaled(m.soft.map(f64::from), f64::from(m.hard), f64::from(new))
                .map(|s| s.round() as u32);
            m.hard = new;
            Ok(format!("Time raised to {new} minutes for this run"))
        }
        "plan" => {
            let matches = |p: &&mut model::PlanBudget| {
                req.provider.as_deref().is_none_or(|v| v == p.provider)
                    && req.window.as_deref().is_none_or(|w| w == p.window)
            };
            let mut found: Vec<&mut model::PlanBudget> =
                r.budget.plan.iter_mut().filter(|p| matches(p)).collect();
            if found.len() != 1 {
                return Err(none());
            }
            let p = found.pop().expect("one");
            if req.hard <= p.hard || req.hard > 100.0 {
                return Err(not_a_raise());
            }
            p.soft = scaled(p.soft, p.hard, req.hard);
            p.hard = req.hard;
            Ok(format!("Plan share raised to {}% for this run", req.hard))
        }
        _ => Err(ApiError::bad_request(
            "invalid_limit",
            "Choose tokens, minutes or plan.",
        )),
    }
}

async fn raise_agent_action(
    State(st): State<AppState>,
    Path((id, agent)): Path<(String, String)>,
    Json(req): Json<RaiseRequest>,
) -> Response {
    let fp = crate::api::fingerprint(&format!("agent-raise:{id}:{agent}"), &req);
    let (st2, request_id) = (st.clone(), req.request_id.clone());
    crate::api::idempotent(&st, &request_id, fp, move || async move {
        raise_agent(&st2, &id, &agent, &req)
            .await
            .map(|said| json!({ "said": said }))
    })
    .await
}

/// Raises one agent's own token, turn or minute limit for this run, and lets it carry on if it was stopped or wrapping up.
async fn raise_agent(
    st: &AppState,
    rig: &str,
    agent: &str,
    req: &RaiseRequest,
) -> Result<String, ApiError> {
    let none = || {
        ApiError::bad_request(
            "no_such_limit",
            "This agent has no limit of that kind to raise.",
        )
    };
    let (mut r, adv) = {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let r = store.run_budget(rig)?.ok_or_else(|| {
            ApiError::conflict("not_running", "This team has no run to raise a limit for.")
        })?;
        let a = store
            .draft(rig)?
            .and_then(|d| d.agents.into_iter().find(|a| a.id == agent))
            .ok_or_else(|| ApiError::not_found(format!("no agent {agent:?}")))?;
        let adv = crate::collaboration::token_limits::effective(&store, rig, agent, &a.advanced)?;
        (r, adv)
    };
    let entry = r.agent_limits.entry(agent.to_string()).or_default();
    let said = match req.limit.as_str() {
        "tokens" => {
            let mut t = adv.token_limit.ok_or_else(none)?;
            let new = req.hard.round() as u64;
            if new <= t.tokens {
                return Err(not_a_raise());
            }
            t.soft = scaled(t.soft.map(|s| s as f64), t.tokens as f64, new as f64)
                .map(|s| s.round() as u64);
            t.tokens = new;
            entry.token_limit = Some(t);
            format!("{agent}: tokens raised to {new} for this run")
        }
        "turns" | "minutes" => {
            let turns = req.limit == "turns";
            let mut l = (if turns {
                adv.turn_limit
            } else {
                adv.minute_limit
            })
            .ok_or_else(none)?;
            let new = req.hard.round() as u32;
            if new <= l.hard {
                return Err(not_a_raise());
            }
            l.soft = scaled(l.soft.map(f64::from), f64::from(l.hard), f64::from(new))
                .map(|s| s.round() as u32);
            l.hard = new;
            if turns {
                entry.turn_limit = Some(l)
            } else {
                entry.minute_limit = Some(l)
            }
            format!("{agent}: {} raised to {new} for this run", req.limit)
        }
        _ => {
            return Err(ApiError::bad_request(
                "invalid_limit",
                "Choose tokens, turns or minutes.",
            ));
        }
    };
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .put_run_budget(rig, &r)?;
    let body = EventBody::BudgetStateChanged {
        state: "raised".into(),
        reason: said.clone(),
    };
    st.local_event(
        crate::hosts::controld(),
        Subject::Rig(rig.to_string()),
        Provenance::ProtocolObserved,
        body,
    )?;
    let seat = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_snapshot(rig)?
        .and_then(|s| s.seats.into_iter().find(|v| v.member == agent))
        .map(|v| v.id);
    if let Some(seat) = seat.filter(|s| {
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .control(s)
            .ok()
            .flatten()
            .is_some()
    }) {
        let st = st.clone();
        tokio::spawn(async move {
            if let Err(e) = carry_on(&st, &seat).await {
                tracing::warn!(%seat, error = %e.message, "agent did not continue after the raise");
            }
        });
    }
    Ok(said)
}

async fn raise(st: &AppState, rig: &str, req: &RaiseRequest) -> Result<String, ApiError> {
    let mut r = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .run_budget(rig)?
        .ok_or_else(|| {
            ApiError::conflict("not_running", "This team has no run to raise a limit for.")
        })?;
    let said = apply(&mut r, req)?;
    let resume = matches!(
        r.state,
        BudgetState::WrapUp { .. } | BudgetState::Stopped { .. }
    );
    let seats = if resume {
        std::mem::take(&mut r.paused_seats)
    } else {
        vec![]
    };
    if resume {
        (r.state, r.wrap_up_ms) = (BudgetState::Normal, None);
    }
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .put_run_budget(rig, &r)?;
    let body = EventBody::BudgetStateChanged {
        state: "raised".into(),
        reason: said.clone(),
    };
    st.local_event(
        crate::hosts::controld(),
        Subject::Rig(rig.to_string()),
        Provenance::ProtocolObserved,
        body,
    )?;
    // In the background: an agent still being stopped is waited for, and the answer should not be.
    for s in seats {
        let st = st.clone();
        tokio::spawn(async move {
            if let Err(e) = carry_on(&st, &s).await {
                tracing::warn!(seat = %s, error = %e.message, "agent did not continue after the raise");
            }
        });
    }
    Ok(said)
}

/// An agent still finishing its wrap-up goes on working; one the budget stopped (or is stopping) starts
/// again where it was, once its stop has finished.
async fn carry_on(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let text =
        "The run's limit was raised, so you can carry on: continue your job where you left off."
            .to_string();
    let stopping = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .control(seat)?
        == Some(crate::store::ControlMode::Hard);
    if !stopping && is_live(st, seat)? {
        st.store
            .lock()
            .expect("store mutex must not be poisoned")
            .clear_control(seat)?;
        return crate::reconciler::send_message_as(
            st,
            seat,
            text,
            model::MessageIntent::Instruct,
            false,
            model::Actor::System("budget".into()),
        )
        .await
        .map(|_| ());
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    while is_live(st, seat)? && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    super::pause::restart(st, seat, ContinueMode::Resume, text).await
}

fn is_live(st: &AppState, seat: &str) -> Result<bool, ApiError> {
    Ok(st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)?
        .is_some_and(|v| {
            matches!(
                v.lifecycle,
                SeatLifecycle::Running | SeatLifecycle::Launching
            )
        }))
}
