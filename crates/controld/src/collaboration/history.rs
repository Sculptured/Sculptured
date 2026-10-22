//! Deployment evidence survives seats being removed on a later project start.
use crate::error::ApiError;
use crate::extract::{Path, Query};
use crate::state::AppState;
use crate::store::Store;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use model::{EventBody, EventRecord, SeatLifecycle, Subject};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeploymentRun {
    #[ts(type = "number")]
    pub start_seq: i64,
    #[ts(type = "number")]
    pub started_ms: i64,
    pub initial_agents: u32,
    /// Imported from a session file: its source (`sculptured`, `imported:claude-code`, `imported:codex`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<String>,
}
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeploymentAgent {
    pub id: String,
    pub name: Option<String>,
    pub parent: Option<String>,
    pub runtime: Option<String>,
    pub lifecycle: Option<SeatLifecycle>,
    #[ts(type = "number | null")]
    pub launched_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub last_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub output_tokens: Option<u64>,
}
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeploymentHistory {
    pub run: DeploymentRun,
    pub agents: Vec<DeploymentAgent>,
    pub has_snapshot: bool,
    #[ts(type = "number | null")]
    pub stopped_ms: Option<i64>,
    pub summary_truncated: bool,
}
#[derive(Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeploymentEvents {
    pub events: Vec<EventRecord>,
    pub has_more: bool,
}
#[derive(Default, Deserialize)]
struct Page {
    before: Option<i64>,
    seat: Option<String>,
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/deployments", axum::routing::get(runs))
        .route(
            "/api/teams/{id}/deployments/{seq}",
            axum::routing::get(detail),
        )
        .route(
            "/api/teams/{id}/deployments/{seq}/events",
            axum::routing::get(events),
        )
}
pub(crate) fn parse(text: String) -> rusqlite::Result<EventRecord> {
    serde_json::from_str(&text).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}
pub(crate) fn bounds(store: &Store, id: &str, seq: i64) -> Result<(DeploymentRun, i64), ApiError> {
    let conn = store.connection();
    let text: Option<String> = conn
        .query_row(
            "SELECT json FROM events WHERE subject=?1 AND seq=?2 AND json_extract(json,'$.body.type')='rig-booted'",
            params![format!("rig:{id}"), seq],
            |r| r.get(0),
        )
        .optional()?;
    let event = parse(text.ok_or_else(|| ApiError::not_found("Deployment run not found"))?)?;
    let EventBody::RigBooted { seat_count, .. } = event.body else {
        unreachable!()
    };
    let end:i64=conn.query_row("SELECT COALESCE(MIN(seq),9223372036854775807) FROM events WHERE subject=?1 AND seq>?2 AND json_extract(json,'$.body.type')='rig-booted'",params![format!("rig:{id}"),seq],|r|r.get(0))?;
    Ok((
        DeploymentRun {
            start_seq: seq,
            started_ms: event.at_ms,
            initial_agents: seat_count,
            source: None,
        },
        end,
    ))
}
async fn runs(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> Response {
    crate::error::respond((|| -> Result<Vec<DeploymentRun>, ApiError> {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        let mut q=store.connection().prepare("SELECT json FROM events WHERE subject=?1 AND seq<?2 AND json_extract(json,'$.body.type')='rig-booted' ORDER BY seq DESC LIMIT 50")?;
        let rows = q.query_map(
            params![format!("rig:{id}"), page.before.unwrap_or(i64::MAX)],
            |r| r.get::<_, String>(0),
        )?;
        let mut out = vec![];
        for row in rows {
            let e = parse(row?)?;
            if let EventBody::RigBooted { seat_count, .. } = e.body {
                out.push(DeploymentRun {
                    start_seq: e.seq,
                    started_ms: e.at_ms,
                    initial_agents: seat_count,
                    source: None,
                });
            }
        }
        if page.before.is_none() {
            out.extend(crate::portable::session::imported_runs(&store, &id)?);
            out.sort_by_key(|r| std::cmp::Reverse(r.started_ms));
        }
        Ok(out)
    })())
}
pub(crate) fn blank(id: String) -> DeploymentAgent {
    DeploymentAgent {
        id,
        name: None,
        parent: None,
        runtime: None,
        lifecycle: None,
        launched_ms: None,
        last_ms: None,
        output_tokens: None,
    }
}
pub(crate) fn load_detail(
    store: &Store,
    id: &str,
    seq: i64,
) -> Result<DeploymentHistory, ApiError> {
    let (run, end) = bounds(store, id, seq)?;
    let conn = store.connection();
    let snapshot: Option<String> = conn
        .query_row(
            "SELECT json FROM deployment_specs WHERE rig=?1 AND start_seq=?2",
            params![id, seq],
            |r| r.get(0),
        )
        .optional()?;
    let mut agents = BTreeMap::<String, DeploymentAgent>::new();
    if let Some(text) = &snapshot {
        let rig: rigspec::CompiledRig = serde_json::from_str(text).map_err(|_| {
            ApiError::bad_request("invalid_history", "Saved deployment is unreadable")
        })?;
        for seat in rig.seats {
            let mut a = blank(seat.id.clone());
            a.name = seat.label;
            a.runtime = Some(
                serde_json::to_value(seat.runtime)
                    .expect("a runtime enum serializes")
                    .as_str()
                    .expect("a runtime enum serializes as its protocol name")
                    .into(),
            );
            agents.insert(seat.id, a);
        }
    }
    // Query event identities, not current seats: that projection intentionally discards old children.
    let suffix = format!("@{id}");
    let mut q=conn.prepare("SELECT json FROM events WHERE seq>=?1 AND seq<?2 AND (subject=?3 OR (substr(subject,1,5)='seat:' AND substr(subject,-length(?4))=?4)) AND json_extract(json,'$.body.type') IN ('team-topology-changed','rig-desired-changed','seat-lifecycle-changed','tenure-started','tenure-ended','usage-reported') ORDER BY seq LIMIT 50001")?;
    let rows = q
        .query_map(params![seq, end, format!("rig:{id}"), suffix], |r| {
            r.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let truncated = rows.len() > 50000;
    let mut stopped_ms = None;
    let limited_rows = rows.into_iter().take(50000);
    for text in limited_rows {
        let e = parse(text)?;
        if matches!(
            e.body,
            EventBody::RigDesiredChanged {
                desired: model::Desired::Stopped
            }
        ) {
            stopped_ms = Some(e.at_ms);
        }
        fold(&mut agents, e);
    }
    // Historical spawn receipts retain the original requested label even after the seat is removed.
    let matching_agents = agents.values_mut().filter(|a| a.name.is_none());
    for a in matching_agents {
        let label:Option<String>=conn.query_row("SELECT json_extract(t.payload,'$.name') FROM agent_children c JOIN agent_tool_receipts t ON t.id=c.request_key WHERE c.rig=?1 AND c.child=?2",params![id,a.id],|r|r.get(0)).optional()?.flatten();
        a.name = label;
    }
    let mut agents: Vec<_> = agents.into_values().collect();
    agents.sort_by_key(|a| (a.launched_ms.unwrap_or(i64::MAX), a.id.clone()));
    Ok(DeploymentHistory {
        run,
        agents,
        has_snapshot: snapshot.is_some(),
        stopped_ms,
        summary_truncated: truncated,
    })
}
/// One event's effect on a run's agent summary (who spawned whom, lifecycle, runtime, output tokens).
pub(crate) fn fold(agents: &mut BTreeMap<String, DeploymentAgent>, e: EventRecord) {
    if let EventBody::TeamTopologyChanged { parent, child } = &e.body {
        agents
            .entry(parent.clone())
            .or_insert_with(|| blank(parent.clone()));
        agents
            .entry(child.clone())
            .or_insert_with(|| blank(child.clone()))
            .parent = Some(parent.clone());
    }
    if let Subject::Seat(seat) = e.subject {
        let a = agents.entry(seat.clone()).or_insert_with(|| blank(seat));
        a.last_ms = Some(e.at_ms);
        match e.body {
            EventBody::SeatLifecycleChanged { lifecycle, .. } => a.lifecycle = Some(lifecycle),
            EventBody::TenureStarted { driver, .. } => {
                a.launched_ms.get_or_insert(e.at_ms);
                if a.runtime.is_none() {
                    a.runtime = Some(driver);
                }
            }
            EventBody::TenureEnded { .. } => a.lifecycle = Some(SeatLifecycle::Stopped),
            EventBody::UsageReported { output_tokens, .. } => {
                a.output_tokens = Some(a.output_tokens.unwrap_or(0).saturating_add(output_tokens))
            }
            _ => {}
        }
    }
}
async fn detail(State(st): State<AppState>, Path((id, seq)): Path<(String, i64)>) -> Response {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    if seq < 0 {
        return crate::error::respond(crate::portable::session::imported_detail(&store, &id, -seq));
    }
    crate::error::respond(load_detail(&store, &id, seq))
}
async fn events(
    State(st): State<AppState>,
    Path((id, seq)): Path<(String, i64)>,
    Query(page): Query<Page>,
) -> Response {
    crate::error::respond((|| -> Result<DeploymentEvents, ApiError> {
        let store = st.store.lock().expect("store mutex must not be poisoned");
        if seq < 0 {
            return crate::portable::session::imported_events(
                &store,
                &id,
                -seq,
                page.before,
                page.seat.as_deref(),
            );
        }
        let (_, end) = bounds(&store, &id, seq)?;
        let mut q=store.connection().prepare("SELECT json FROM events WHERE seq>=?1 AND seq<?2 AND seq<?3 AND (subject=?4 OR (substr(subject,1,5)='seat:' AND substr(subject,-length(?5))=?5)) AND (?6 IS NULL OR subject='seat:'||?6) ORDER BY seq DESC LIMIT 201")?;
        let rows = q
            .query_map(
                params![
                    seq,
                    end,
                    page.before.unwrap_or(i64::MAX),
                    format!("rig:{id}"),
                    format!("@{id}"),
                    page.seat
                ],
                |r| r.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = rows.len() > 200;
        let events = rows
            .into_iter()
            .take(200)
            .map(parse)
            .collect::<Result<_, _>>()?;
        Ok(DeploymentEvents { events, has_more })
    })())
}
