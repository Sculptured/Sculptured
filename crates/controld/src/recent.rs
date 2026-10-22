//! Each agent's latest events for a whole team in one request, with long text shortened: what the board
//! needs to show what every agent is doing, without every agent's tool outputs and replies in full.
use crate::error::ApiError;
use crate::extract::{Path, Query};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Text longer than this many characters is cut.
const BRIEF: usize = 300;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/recent", axum::routing::get(recent))
}

#[derive(Deserialize)]
struct RecentQuery {
    tail: Option<usize>,
}

async fn recent(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<RecentQuery>,
) -> Response {
    let tail = q.tail.unwrap_or(60).min(500);
    let read = tokio::task::spawn_blocking(move || read(&st, &id, tail)).await;
    crate::error::respond(
        read.map_err(|_| ApiError::internal("reading recent events failed"))
            .and_then(|r| r.map_err(ApiError::from)),
    )
}

/// Seat → its newest `tail` events about itself, oldest first, shortened.
fn read(st: &AppState, team: &str, tail: usize) -> rusqlite::Result<BTreeMap<String, Vec<Value>>> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let seats: Vec<String> = store
        .rig_snapshot(team)?
        .map(|s| s.seats.into_iter().map(|v| v.id).collect())
        .unwrap_or_default();
    let mut out = BTreeMap::new();
    for seat in seats {
        let events = store.latest_events_for_seat(&seat, tail)?;
        let own: Vec<Value> = events
            .into_iter()
            .filter(|e| matches!(&e.subject, model::Subject::Seat(s) if *s == seat))
            .map(|e| brief(serde_json::to_value(e).expect("events serialize")))
            .collect();
        out.insert(seat, own);
    }
    Ok(out)
}

/// Every string longer than `BRIEF` characters cut, keeping each event's shape.
pub(crate) fn brief(v: Value) -> Value {
    match v {
        Value::String(s) if s.chars().count() > BRIEF => {
            Value::String(s.chars().take(BRIEF).chain(['…']).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(brief).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, brief(v))).collect()),
        other => other,
    }
}
