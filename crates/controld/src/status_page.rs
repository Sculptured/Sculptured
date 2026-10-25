//! The team's status page: one Markdown page that says where the work stands, so someone coming back
//! later understands the state and the changes without reading any transcript. Only its owner
//! (`TeamDraft::status_owner`) writes it, with `write_status`; each write replaces the page and is kept
//! as a version (`EventBody::StatusPageWritten` on the team). People read it under Results → Status.
use crate::error::{ApiError, respond};
use crate::extract::Path;
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use model::{Actor, EventBody, EventRecord, Provenance, Subject, TeamDraft};
use serde_json::{Value, json};

/// The largest page an agent may write.
pub const MAX_BYTES: usize = 64 * 1024;
/// Versions listed, newest first.
const VERSIONS: usize = 100;
const TYPE: &str = "status-page-written";

/// The team `seat` writes the status page of, or why it may not: only the page's owner writes it.
fn owned_team<'a>(st: &AppState, seat: &'a str) -> Result<&'a str, String> {
    let team = crate::seat_ids::rig_of(seat).ok_or("Unknown agent.")?;
    let draft = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .draft(team)
        .map_err(|e| e.to_string())?
        .ok_or("This team has no status page.")?;
    let owner = draft
        .status_owner()
        .ok_or("This team has no status page.")?;
    if draft.agent_for_seat(seat).map(|a| a.id.as_str()) != Some(owner.id.as_str()) {
        return Err(format!(
            "Only {} writes this team's status page. Hand your results to them instead.",
            owner.name
        ));
    }
    Ok(team)
}

/// `write_status { markdown, done? }`: the owner replaces the team's status page.
pub fn write_status(st: &AppState, seat: &str, args: &Value) -> Result<Value, String> {
    let team = owned_team(st, seat)?;
    let markdown = args
        .get("markdown")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .ok_or("markdown must be the whole page, as non-empty Markdown")?;
    if markdown.len() > MAX_BYTES {
        return Err(format!(
            "The status page is at most {} KB; this one is {} KB. Shorten it.",
            MAX_BYTES / 1024,
            markdown.len().div_ceil(1024)
        ));
    }
    let done = args.get("done").and_then(Value::as_bool).unwrap_or(false);
    st.local_event(
        Actor::Seat(seat.to_string()),
        Subject::Rig(team.to_string()),
        Provenance::AgentReported,
        EventBody::StatusPageWritten {
            markdown: markdown.to_string(),
            done,
        },
    )
    .map_err(|e| format!("Sculptured could not save the status page: {e}"))?;
    Ok(json!({ "text": if done {
        "Saved the final status page. People see it under Results → Status."
    } else {
        "Saved the status page. People see it under Results → Status."
    } }))
}

/// The owner's briefing line; None for every other agent.
pub(crate) fn briefing_line(d: &TeamDraft, me: &model::DraftAgent) -> Option<String> {
    (d.status_owner()?.id == me.id).then(|| {
        "You own this team's status page: the one page people read under Results → Status to understand where the work stands and what changed, without reading any transcript. Write it with write_status, in Markdown: the goal, what is done, what changed (files, decisions and why), what is in progress, what is blocked or open, and what comes next. Each write replaces the whole page, so always write all of it. Update it when the state changes in a way someone coming back would need to know, not after every step. When the team's work is finished, write it once more with done: true as the final word. Only you write it; teammates hand you their results.".to_string()
    })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams/{id}/status", get(view))
        .route("/api/teams/{id}/status/{seq}", get(version))
}

fn author(d: Option<&TeamDraft>, e: &EventRecord) -> (String, String) {
    let seat = match &e.actor {
        Actor::Seat(s) | Actor::Human(s) | Actor::System(s) => s.clone(),
    };
    let name = d.and_then(|d| d.agent_for_seat(&seat)).map_or_else(
        || crate::seat_ids::member_of(&seat).to_string(),
        |a| a.name.clone(),
    );
    (seat, name)
}

fn page(d: Option<&TeamDraft>, e: &EventRecord) -> Option<Value> {
    let EventBody::StatusPageWritten { markdown, done } = &e.body else {
        return None;
    };
    let (seat, name) = author(d, e);
    Some(
        json!({ "seq": e.seq, "at_ms": e.at_ms, "author": seat, "author_name": name, "markdown": markdown, "done": done }),
    )
}

/// `GET /api/teams/{id}/status`: who writes the page, the page as it is now, and its versions (newest first).
async fn view(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    respond((|| {
        let d = store.draft(&id)?;
        let events = store.latest_of_types(&Subject::Rig(id.clone()), &[TYPE], VERSIONS)?;
        let owner = d
            .as_ref()
            .and_then(TeamDraft::status_owner)
            .map(|a| json!({ "id": a.id, "name": a.name, "chosen": a.advanced.writes_status }));
        let versions: Vec<Value> = events
            .iter()
            .filter_map(|e| page(d.as_ref(), e))
            .map(|mut v| {
                v.as_object_mut().map(|o| o.remove("markdown"));
                v
            })
            .collect();
        Ok::<_, ApiError>(json!({
            "owner": owner,
            "page": events.first().and_then(|e| page(d.as_ref(), e)),
            "versions": versions,
        }))
    })())
}

/// `GET /api/teams/{id}/status/{seq}`: one earlier version of the page.
async fn version(State(st): State<AppState>, Path((id, seq)): Path<(String, i64)>) -> Response {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    respond((|| {
        let d = store.draft(&id)?;
        store
            .latest_of_types(&Subject::Rig(id.clone()), &[TYPE], VERSIONS)?
            .iter()
            .find(|e| e.seq == seq)
            .and_then(|e| page(d.as_ref(), e))
            .ok_or_else(|| ApiError::not_found("That version of the status page is not kept."))
    })())
}
