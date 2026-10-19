//! Local reusable groups. These records never enter the project list or start agents.
use crate::{
    error::{ApiError, respond},
    extract::{Json, Path},
    state::AppState,
};
use axum::{Router, extract::State, response::Response, routing::get};
use model::GroupBlock;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/group-blocks", get(list))
        .route("/api/group-blocks/{id}", get(read).put(save))
}
fn parsed(s: &str) -> Result<GroupBlock, ApiError> {
    serde_json::from_str(s).map_err(|_| ApiError::internal("Could not read the saved group"))
}
async fn list(State(st): State<AppState>) -> Response {
    respond((|| -> Result<Value, ApiError> {
        let store = st.store.lock().unwrap();
        let mut query = store
            .connection()
            .prepare("SELECT json FROM group_blocks ORDER BY name COLLATE NOCASE, id")?;
        let strings = query.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        let blocks = strings.iter().map(|s| parsed(s)).collect::<Result<Vec<_>, _>>()?;
        Ok(json!(blocks))
    })())
}
async fn read(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    respond((|| -> Result<Value, ApiError> {
        let store = st.store.lock().unwrap();
        let text: Option<String> = store
            .connection()
            .query_row("SELECT json FROM group_blocks WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        Ok(json!(parsed(&text.ok_or_else(|| ApiError::not_found("Group not found"))?)?))
    })())
}
#[derive(Deserialize, Serialize)]
struct Save {
    request_id: String,
    block: GroupBlock,
}
async fn save(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<Save>) -> Response {
    let fingerprint = crate::idempotent::fingerprint(&format!("group-block:{id}"), &req);
    let state = st.clone();
    let request = req.request_id.clone();
    crate::idempotent::idempotent(&st, &request, fingerprint, move || async move { save_in(&state, &id, req.block) }).await
}
fn invalid() -> ApiError {
    ApiError::bad_request("group_block_invalid", "Group must be a bounded, self-contained board fragment")
}
fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn validate(b: &GroupBlock) -> Result<(), ApiError> {
    let d = &b.draft;
    if !valid_id(&b.id)
        || b.name.trim().is_empty()
        || b.name.len() > 200
        || d.groups.len() != 1
        || d.agents.len() > 100
        || d.connections.len() > 500
        || b.boundary_notes.len() > 500
        || b.source.as_ref().is_some_and(|s| !valid_id(&s.community_block) || s.version < 1)
    {
        return Err(invalid());
    }
    let g = &d.groups[0];
    if !valid_id(&g.id) || g.w < 1 || g.h < 1 || d.workflow.as_ref().is_some_and(|w| w.stages.len() > 100) {
        return Err(invalid());
    }
    let ids: HashSet<_> = d.agents.iter().map(|a| a.id.as_str()).collect();
    if ids.len() != d.agents.len() || d.agents.iter().any(|a| !valid_id(&a.id) || a.group_id.as_deref() != Some(&g.id)) {
        return Err(invalid());
    }
    let connections: HashSet<_> = d.connections.iter().map(|c| c.id.as_str()).collect();
    if connections.len() != d.connections.len()
        || d.connections
            .iter()
            .any(|c| !valid_id(&c.id) || !ids.contains(c.from.as_str()) || !ids.contains(c.to.as_str()) || c.from == c.to)
    {
        return Err(invalid());
    }
    if d.workflow
        .as_ref()
        .is_some_and(|w| !d.connections.is_empty() || !workflow_ok(w, &ids))
    {
        return Err(invalid());
    }
    Ok(())
}
/// A group's workflow is self-contained: unique valid stage ids, every agent it names (a stage's agent,
/// workers and reviewers) is in the group, and every route and round-limit route names one of its stages.
/// Check, person and merge stages name no agent.
fn workflow_ok(w: &model::Workflow, agents: &HashSet<&str>) -> bool {
    let stages: HashSet<_> = w.stages.iter().map(|s| s.id.as_str()).collect();
    // An agent stage needs its agent or its review panel; check, person and merge stages have none.
    let staffed = |s: &model::WorkflowStage| s.kind != model::StageKind::Agent || !s.agent.is_empty() || !s.reviewers.is_empty();
    let named = |s: &model::WorkflowStage| {
        (!s.agent.is_empty())
            .then_some(&s.agent)
            .into_iter()
            .chain(&s.workers)
            .chain(&s.reviewers)
            .all(|a| agents.contains(a.as_str()))
    };
    let leads_inside = |s: &model::WorkflowStage| {
        s.routes
            .values()
            .flatten()
            .chain(&s.on_limit)
            .all(|to| stages.contains(to.as_str()))
    };
    stages.len() == w.stages.len() && w.stages.iter().all(|s| valid_id(&s.id) && staffed(s) && named(s) && leads_inside(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_workflow_may_hold_checks_people_and_panels_but_stays_self_contained() {
        let stage = |id: &str, kind: model::StageKind, agent: &str, reviewers: &[&str], to: Option<&str>| model::WorkflowStage {
            id: id.into(),
            name: id.into(),
            kind,
            agent: agent.into(),
            reviewers: reviewers.iter().map(|r| r.to_string()).collect(),
            routes: [("next".to_string(), to.map(str::to_string))].into(),
            ..Default::default()
        };
        let agents: HashSet<&str> = ["a", "b", "c"].into();
        let mut w = model::Workflow {
            stages: vec![
                stage("build", model::StageKind::Agent, "a", &[], Some("tests")),
                stage("tests", model::StageKind::Check, "", &[], Some("review")),
                stage("review", model::StageKind::Agent, "", &["b", "c"], Some("chief")),
                stage("chief", model::StageKind::Human, "", &[], None),
            ],
            ..Default::default()
        };
        assert!(workflow_ok(&w, &agents));
        w.stages[2].reviewers.push("outsider".into());
        assert!(!workflow_ok(&w, &agents), "a reviewer outside the group");
        w.stages[2].reviewers.pop();
        w.stages[0].on_limit = Some("elsewhere".into());
        assert!(!workflow_ok(&w, &agents), "a round-limit route outside the group");
        w.stages[0].on_limit = None;
        w.stages[0].agent.clear();
        assert!(!workflow_ok(&w, &agents), "an agent stage with no agent");
    }
}

fn save_in(st: &AppState, id: &str, mut b: GroupBlock) -> Result<Value, ApiError> {
    validate(&b)?;
    if b.id != id {
        return Err(invalid());
    }
    let store = st.store.lock().unwrap();
    let conn = store.connection();
    let old: Option<i64> = conn
        .query_row("SELECT revision FROM group_blocks WHERE id=?1", [id], |r| r.get(0))
        .optional()?;
    if old.unwrap_or(0) != i64::try_from(b.revision).map_err(|_| invalid())? {
        return Err(ApiError::conflict("group_block_conflict", "Reload the group before saving"));
    }
    b.name = b.name.trim().to_owned();
    b.revision = b.revision.checked_add(1).ok_or_else(invalid)?;
    let text = serde_json::to_string(&b).map_err(|_| invalid())?;
    if text.len() > 1_000_000 {
        return Err(invalid());
    }
    conn.execute("INSERT INTO group_blocks(id,name,revision,json) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET name=excluded.name, revision=excluded.revision, json=excluded.json",params![id,b.name,i64::try_from(b.revision).map_err(|_|invalid())?,text])?;
    Ok(json!(b))
}
