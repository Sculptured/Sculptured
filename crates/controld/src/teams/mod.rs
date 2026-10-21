//! Team drafts: storage-backed API for the builder. A team's rig name is its draft id;
//! a started team is linked through the rig's `RigSource::Team { id, revision }`.
mod folder_checks;
mod textbook;
mod textbook_files;
mod run;

pub(crate) use folder_checks::folder_issues;
use run::{start_team, stop_team};
use crate::api::{fingerprint, idempotent};
use crate::error::{respond, to_value, ApiError};
use crate::state::AppState;
use crate::store::{is_live, RigSource, SaveDraft, Store};
use crate::views::{TeamImport, TeamRecord, TeamSpec, TeamSummary};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use crate::extract::{Json, Path};
use axum::Router;
use model::{DraftIssue, IssueSeverity, TeamDraft};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Team ids that would collide with fixed API paths.
const RESERVED: [&str; 3] = ["import", "import-file", "check-file"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/teams", get(list_teams).post(create_team))
        .route("/api/teams/import", post(import_team))
        .route("/api/team-templates", get(list_templates))
        .route("/api/teams/{id}", get(get_team).put(put_team))
        .route("/api/teams/{id}/validate", post(validate_team))
        .route("/api/teams/{id}/spec", get(team_spec))
        .route("/api/teams/{id}/start", post(start_team))
        .route("/api/teams/{id}/resumable", get(run::resumable))
        .route("/api/teams/{id}/stop", post(stop_team))
        .route("/api/teams/{id}/textbook/prepare", post(textbook::prepare))
}

pub(crate) fn is_error(i: &DraftIssue) -> bool {
    i.severity == IssueSeverity::Error
}

pub(crate) fn load(st: &AppState, id: &str) -> Result<TeamDraft, ApiError> {
    st.store.lock().unwrap().draft(id)?.ok_or_else(|| ApiError::not_found(format!("no team {id:?}")))
}

/// True while any agent of team `id` may have a process, or a start is in flight.
pub(crate) fn team_live(st: &AppState, id: &str) -> Result<bool, ApiError> {
    let store = st.store.lock().unwrap();
    team_live_in(st, &store, id)
}

/// `team_live` for a caller already holding the store lock. The boot claim is read
/// under that lock, so a save checked here cannot interleave with a start (which
/// claims before it reads the draft).
fn team_live_in(st: &AppState, store: &Store, id: &str) -> Result<bool, ApiError> {
    if st.is_booting(id) {
        return Ok(true);
    }
    if !matches!(store.rig_source(id)?, Some(RigSource::Team { .. })) {
        return Ok(false);
    }
    Ok(store.rig_snapshot(id)?.is_some_and(|s| s.seats.iter().any(|x| is_live(x.lifecycle))))
}

pub(crate) fn issues_for(st: &AppState, d: &TeamDraft) -> Vec<DraftIssue> {
    let mut issues = rigspec::validate_draft(d);
    if crate::launch::hosted(st) {
        // An agent with no folder works in its own computer's first allowed folder (the runner resolves ".").
        issues.retain(|i| i.code != "folder-missing");
    }
    issues.extend(folder_issues(st, d));
    issues.extend(folder_checks::computer_issues(st, d));
    issues
}

pub(crate) fn record(st: &AppState, draft: TeamDraft) -> Result<TeamRecord, ApiError> {
    let started_revision = match st.store.lock().unwrap().rig_source(&draft.id)? {
        Some(RigSource::Team { revision, .. }) => Some(revision),
        _ => None,
    };
    let live = team_live(st, &draft.id)?;
    let issues = issues_for(st, &draft);
    let origin = st.store.lock().unwrap().team_origin(&draft.id)?;
    Ok(TeamRecord { draft, started_revision, live, issues, origin })
}

/// A store error counts as "not taken": `unique_slug` searches without a bound, so
/// treating errors as taken would loop forever while holding the store lock. The
/// following `insert_draft` then surfaces the store error as a 500.
pub(crate) fn new_team_id(store: &Store, name: &str) -> String {
    rigspec::draft::unique_slug(name, "team", |c| RESERVED.contains(&c) || store.name_taken(c).unwrap_or(false))
}

/// What may change while agents run: names, jobs, positions and group labels.
/// Everything else is structure and needs Stop team first.
fn structure(d: &TeamDraft) -> Value {
    let mut workflow = d.workflow.clone();
    if let Some(w) = &mut workflow { w.input_position = None; }
    json!({
        "workflow": workflow,
        "folder": d.folder,
        "agents": d.agents.iter().map(|a| json!({
            "id": a.id, "group": a.group_id, "kind": a.kind, "folder": a.folder, "advanced": a.advanced,
        })).collect::<Vec<_>>(),
        "connections": d.connections.iter().map(|c| json!({ "id": c.id, "from": c.from, "to": c.to, "kind": c.kind })).collect::<Vec<_>>(),
        "groups": d.groups.iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
    })
}

fn summaries(st: &AppState) -> Result<Vec<TeamSummary>, ApiError> {
    let store = st.store.lock().unwrap();
    let rigs = store.list_rigs()?;
    let mut out = Vec::new();
    for (d, updated_at_ms) in store.drafts()? {
        let started = match store.rig_source(&d.id)? {
            Some(RigSource::Team { .. }) => rigs.iter().find(|r| r.name == d.id).cloned(),
            _ => None,
        };
        out.push(TeamSummary { id: d.id.clone(), name: d.name.clone(), agent_count: d.agents.len() as u32, updated_at_ms, started });
    }
    Ok(out)
}

async fn list_teams(State(st): State<AppState>) -> Response {
    respond(summaries(&st))
}

async fn list_templates() -> Response {
    Json(rigspec::templates::templates()).into_response()
}

async fn get_team(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    respond(load(&st, &id).and_then(|d| record(&st, d)))
}

#[derive(Deserialize, Serialize)]
struct CreateReq {
    #[serde(default)]
    request_id: String,
    template: String,
    #[serde(default)]
    practice: bool,
    #[serde(default)]
    name: Option<String>,
}

async fn create_team(State(st): State<AppState>, Json(req): Json<CreateReq>) -> Response {
    let fp = fingerprint("create-team", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move {
        let info = rigspec::templates::templates()
            .into_iter()
            .find(|t| t.id == req.template)
            .ok_or_else(|| ApiError::bad_request("unknown_template", format!("no template {:?}", req.template)))?;
        let name = req.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&info.default_name).to_string();
        let draft = {
            let mut store = st2.store.lock().unwrap();
            let id = new_team_id(&store, &name);
            let draft = rigspec::templates::template(&info.id, &id, &name, req.practice).expect("listed template exists");
            store.insert_draft(&draft)?;
            draft
        };
        crate::analytics::emit(&st2, "team.created", serde_json::json!({ "template": crate::analytics::template(&info.id) }));
        to_value(&record(&st2, draft)?)
    })
    .await
}

#[derive(Deserialize, Serialize)]
struct PutReq {
    #[serde(default)]
    request_id: String,
    revision: u64,
    draft: TeamDraft,
}

fn draft_conflict(current: u64) -> ApiError {
    ApiError::conflict(
        "draft_conflict",
        format!("This team was changed somewhere else (it is now at version {current}). Load the latest version or keep yours."),
    )
}

/// Saves the draft if its revision is current and, while the team runs, only names and jobs
/// changed. One lock for the revision check, the running check and the save, so a start
/// cannot claim the team in between and run the version before this save.
fn save_checked(st: &AppState, id: &str, req: &PutReq) -> Result<SaveDraft, ApiError> {
    let mut store = st.store.lock().unwrap();
    crate::project_visibility::ensure_available(&store, id)?;
    let current = store.draft(id)?.ok_or_else(|| ApiError::not_found(format!("no team {id:?}")))?;
    // A stale revision is a conflict whatever the edit, so it is checked first.
    if current.revision != req.revision {
        return Err(draft_conflict(current.revision));
    }
    if team_live_in(st, &store, id)? && structure(&current) != structure(&req.draft) {
        return Err(ApiError::conflict(
            "team_running",
            "Stop the team before adding, removing or changing agents, connections or groups. While it runs you can change names and jobs.",
        ));
    }
    #[cfg(feature = "test-seams")]
    crate::seams::run(&crate::seams::BETWEEN_SAVE_CHECK_AND_WRITE, id);
    Ok(store.save_draft(&req.draft, req.revision)?)
}

async fn put_team(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<PutReq>) -> Response {
    let fp = fingerprint(&format!("put-team:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move {
        if req.draft.id != id {
            return Err(ApiError::bad_request("team_id_mismatch", "the draft's id does not match the URL"));
        }
        match save_checked(&st2, &id, &req)? {
            SaveDraft::Saved(d) => {
                crate::analytics::saved(&st2, &d);
                to_value(&record(&st2, d)?)
            }
            SaveDraft::Conflict { current } => Err(draft_conflict(current)),
            SaveDraft::Missing => Err(ApiError::not_found(format!("no team {id:?}"))),
        }
    })
    .await
}

#[derive(Deserialize)]
struct ValidateReq {
    #[serde(default)]
    draft: Option<TeamDraft>,
}

fn validate(st: &AppState, id: &str, draft: Option<TeamDraft>) -> Result<Vec<DraftIssue>, ApiError> {
    let d = match draft {
        Some(d) => d,
        None => load(st, id)?,
    };
    Ok(issues_for(st, &d))
}

async fn validate_team(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<ValidateReq>) -> Response {
    respond(validate(&st, &id, req.draft))
}

async fn team_spec(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    respond(load(&st, &id).map(|d| {
        let issues = issues_for(&st, &d);
        TeamSpec { yaml: rigspec::exchange::export_yaml(&d), valid: !issues.iter().any(is_error), issues }
    }))
}

#[derive(Deserialize, Serialize)]
struct ImportReq {
    #[serde(default)]
    request_id: String,
    yaml: String,
}

async fn import_team(State(st): State<AppState>, Json(req): Json<ImportReq>) -> Response {
    let fp = fingerprint("import-team", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move {
        // Parse and compile without the store lock (every other request needs the store),
        // keeping the spec's name; then lock once to choose a free id and insert.
        let mut spec_name = String::new();
        let mut imported = rigspec::exchange::import_yaml(&req.yaml, |name| {
            #[cfg(feature = "test-seams")]
            crate::seams::run(&crate::seams::AFTER_IMPORT_PARSE, name);
            spec_name = name.to_string();
            String::new()
        })
        .map_err(|d| ApiError::unprocessable("spec_invalid", "the spec has errors", d))?;
        {
            let mut store = st2.store.lock().unwrap();
            imported.draft.id = new_team_id(&store, &spec_name);
            store.insert_draft(&imported.draft)?;
        }
        crate::analytics::emit(&st2, "team.created", json!({ "template": "import" }));
        to_value(&TeamImport { team: record(&st2, imported.draft)?, dropped: imported.dropped, warnings: Vec::new() })
    })
    .await
}
