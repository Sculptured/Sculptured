//! Proposed changes (`propose_change`): the agents' briefing line and the team's list of changes, each with
//! its rule results and what became of it. The proposal, the checks and the decision are separate events on
//! the team (`ChangeProposed` agent-reported, `ChangeChecked` verified, `ChangeDecided` by its decider).
use crate::error::{ApiError, respond};
use crate::extract::Path;
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use model::{Actor, DraftAgent, EventBody, EventRecord, Subject, TeamDraft};
use serde_json::{Map, Value, json};

const TYPES: [&str; 3] = ["change-proposed", "change-checked", "change-decided"];
/// The most recent changes listed.
const LISTED: usize = 200;

/// For a team with rules: every agent proposes a change to a business system before making it.
#[must_use]
pub fn briefing_line(d: &TeamDraft, _me: &DraftAgent) -> Option<String> {
    (!d.mandate.is_empty()).then(|| {
        "Before you change anything in a business system (an order, a schedule, a record in the ERP or \
         another system), call propose_change with exactly what you would write and wait for its answer. \
         Carry out only what it approves, exactly as proposed; if it is not approved, do not make the change."
            .to_string()
    })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/changes", get(list))
}

fn agent_name(d: Option<&TeamDraft>, seat: &str) -> String {
    d.and_then(|d| d.agent_for_seat(seat)).map_or_else(
        || crate::seat_ids::member_of(seat).to_string(),
        |a| a.name.clone(),
    )
}

/// One change's fields from one of its events, merged into `row`.
fn merge(d: Option<&TeamDraft>, e: &EventRecord, row: &mut Map<String, Value>) {
    match &e.body {
        EventBody::ChangeProposed {
            proposal_id,
            action,
            system,
            summary,
            payload,
            reason,
        } => {
            let seat = match &e.actor {
                Actor::Seat(s) | Actor::Human(s) | Actor::System(s) => s.clone(),
            };
            row.extend([
                ("id".into(), json!(proposal_id)),
                ("at_ms".into(), json!(e.at_ms)),
                ("agent".into(), json!(seat)),
                ("agent_name".into(), json!(agent_name(d, &seat))),
                ("action".into(), json!(action)),
                ("system".into(), json!(system)),
                ("summary".into(), json!(summary)),
                ("payload".into(), payload.clone()),
                ("reason".into(), json!(reason)),
            ]);
        }
        EventBody::ChangeChecked { results, .. } => {
            row.insert("checks".into(), json!(results));
        }
        EventBody::ChangeDecided {
            approved, by, note, ..
        } => {
            row.insert(
                "decision".into(),
                json!({ "approved": approved, "by": by, "note": note, "at_ms": e.at_ms }),
            );
        }
        _ => {}
    }
}

fn proposal_of(e: &EventRecord) -> Option<&str> {
    match &e.body {
        EventBody::ChangeProposed { proposal_id, .. }
        | EventBody::ChangeChecked { proposal_id, .. }
        | EventBody::ChangeDecided { proposal_id, .. } => Some(proposal_id),
        _ => None,
    }
}

/// `GET /api/teams/{id}/changes`: the team's proposed changes, newest first. A change with no `decision`
/// is still waiting for one.
async fn list(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    respond((|| {
        let d = store.draft(&id)?;
        let mut events = store.latest_of_types(&Subject::Rig(id.clone()), &TYPES, LISTED * 3)?;
        events.reverse();
        let mut order: Vec<String> = Vec::new();
        let mut rows: std::collections::HashMap<String, Map<String, Value>> = Default::default();
        for e in &events {
            let Some(pid) = proposal_of(e) else { continue };
            if !rows.contains_key(pid) {
                order.push(pid.to_string());
            }
            merge(d.as_ref(), e, rows.entry(pid.to_string()).or_default());
        }
        let changes: Vec<Value> = order
            .iter()
            .rev()
            .filter_map(|pid| rows.remove(pid))
            .filter(|r| r.contains_key("summary"))
            .take(LISTED)
            .map(Value::Object)
            .collect();
        Ok::<_, ApiError>(json!({ "changes": changes, "mandate": d.map(|d| d.mandate) }))
    })())
}
