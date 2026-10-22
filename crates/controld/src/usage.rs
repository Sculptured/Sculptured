//! Token use over time for a team's agents: each agent's running totals after every usage report
//! the CLI made, and its latest context fill. Read from the event journal; nothing is estimated
//! here. Long runs are thinned to `MAX_POINTS` per agent, always keeping the first and last report.
use crate::error::ApiError;
use crate::state::AppState;
use axum::Router;
use axum::extract::{Path, State};
use axum::response::Response;
use model::{ContextBasis, EventBody};
use rusqlite::params;
use serde::Serialize;
use std::collections::BTreeMap;
use ts_rs::TS;

pub const MAX_POINTS: usize = 240;

/// Totals so far, after the report at `at_ms`. `input` excludes cached input.
#[derive(Clone, Debug, Default, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct UsagePoint {
    #[ts(type = "number")]
    pub at_ms: i64,
    #[ts(type = "number")]
    pub input: u64,
    #[ts(type = "number")]
    pub output: u64,
    #[ts(type = "number")]
    pub cache_read: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ContextReading {
    #[ts(type = "number")]
    pub at_ms: i64,
    #[ts(type = "number")]
    pub used_tokens: u64,
    #[ts(type = "number | null")]
    pub window_tokens: Option<u64>,
    /// True when the CLI gave no per-call total and the fill is Sculptured's estimate.
    pub estimated: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct AgentUsage {
    pub seat: String,
    /// Running totals, oldest first (thinned to at most `MAX_POINTS`).
    pub points: Vec<UsagePoint>,
    /// How many usage reports the CLI made.
    pub reports: u32,
    pub context: Option<ContextReading>,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TeamUsage {
    pub agents: Vec<AgentUsage>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/usage", axum::routing::get(team_usage))
}

async fn team_usage(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::error::respond(
        tokio::task::spawn_blocking(move || read(&st, &id))
            .await
            .map_err(|_| ApiError::internal("usage read failed"))
            .and_then(|r| r.map_err(ApiError::from)),
    )
}

fn read(st: &AppState, team: &str) -> rusqlite::Result<TeamUsage> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let mut stmt = store.connection().prepare(
        "SELECT s.id, e.at_ms, e.json FROM seats s JOIN events e ON e.subject='seat:'||s.id
         WHERE s.rig=?1 AND json_extract(e.json, '$.body.type') IN ('usage-reported','context-reported')
         ORDER BY e.seq",
    )?;
    let rows = stmt.query_map(params![team], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut agents: BTreeMap<String, AgentUsage> = BTreeMap::new();
    let mut totals: BTreeMap<String, UsagePoint> = BTreeMap::new();
    for row in rows {
        let (seat, at_ms, json) = row?;
        let Ok(rec) = serde_json::from_str::<model::EventRecord>(&json) else {
            continue;
        };
        let a = agents.entry(seat.clone()).or_insert_with(|| AgentUsage {
            seat: seat.clone(),
            points: vec![],
            reports: 0,
            context: None,
        });
        match rec.body {
            EventBody::UsageReported {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                ..
            } => {
                let t = totals.entry(seat).or_default();
                t.at_ms = at_ms;
                t.input += input_tokens;
                t.output += output_tokens;
                t.cache_read += cache_read_tokens;
                a.points.push(t.clone());
                a.reports += 1;
            }
            EventBody::ContextReported {
                used_tokens,
                window_tokens,
                basis,
                ..
            } => {
                a.context = Some(ContextReading {
                    at_ms,
                    used_tokens,
                    window_tokens,
                    estimated: basis == ContextBasis::Estimated,
                });
            }
            _ => {}
        }
    }
    let mut agents: Vec<AgentUsage> = agents.into_values().collect();
    for a in &mut agents {
        a.points = thin(std::mem::take(&mut a.points), MAX_POINTS);
    }
    Ok(TeamUsage { agents })
}

/// At most `max` points spread evenly, keeping the first and the last.
fn thin(points: Vec<UsagePoint>, max: usize) -> Vec<UsagePoint> {
    if points.len() <= max || max < 2 {
        return points;
    }
    let last = points.len() - 1;
    (0..max)
        .map(|i| points[i * last / (max - 1)].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinning_keeps_the_ends_and_the_count() {
        let pts: Vec<UsagePoint> = (0..1000)
            .map(|i| UsagePoint {
                at_ms: i,
                output: i as u64,
                ..Default::default()
            })
            .collect();
        let t = thin(pts, 10);
        assert_eq!(t.len(), 10);
        assert_eq!((t[0].at_ms, t[9].at_ms), (0, 999));
        assert!(t.windows(2).all(|w| w[0].at_ms < w[1].at_ms));
    }
}
