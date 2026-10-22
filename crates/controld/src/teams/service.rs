use super::{
    ApiError, AppState, Deserialize, DraftIssue, IntoResponse, IssueSeverity, Json, Path, Response,
    RigSource, Router, SaveDraft, Serialize, State, Store, TeamDraft, TeamImport, TeamRecord,
    TeamSpec, TeamSummary, Value, folder_checks, json, run, textbook,
};

/// Team ids that would collide with fixed API paths.
pub(super) const RESERVED: [&str; 3] = ["import", "import-file", "check-file"];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/teams",
            crate::teams::get(list_teams).post(create_team),
        )
        .route("/api/teams/import", crate::teams::post(import_team))
        .route("/api/team-templates", crate::teams::get(list_templates))
        .route("/api/teams/{id}", crate::teams::get(get_team).put(put_team))
        .route(
            "/api/teams/{id}/validate",
            crate::teams::post(validate_team),
        )
        .route("/api/teams/{id}/spec", crate::teams::get(team_spec))
        .route(
            "/api/teams/{id}/start",
            crate::teams::post(crate::teams::start_team),
        )
        .route(
            "/api/teams/{id}/resumable",
            crate::teams::get(run::resumable),
        )
        .route(
            "/api/teams/{id}/stop",
            crate::teams::post(crate::teams::stop_team),
        )
        .route(
            "/api/teams/{id}/textbook/prepare",
            crate::teams::post(textbook::prepare),
        )
}

pub(crate) fn is_error(i: &DraftIssue) -> bool {
    i.severity == IssueSeverity::Error
}

pub(crate) fn load(st: &AppState, id: &str) -> Result<TeamDraft, ApiError> {
    st.store
        .lock()
        .expect("store mutex must not be poisoned")
        .draft(id)?
        .ok_or_else(|| ApiError::not_found(format!("no team {id:?}")))
}

/// True while any agent of team `id` may have a process, or a start is in flight.
pub(crate) fn team_live(st: &AppState, id: &str) -> Result<bool, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    team_live_in(st, &store, id)
}

/// `team_live` for a caller already holding the store lock. The boot claim is read
/// under that lock, so a save checked here cannot interleave with a start (which
/// claims before it reads the draft).
pub(super) fn team_live_in(st: &AppState, store: &Store, id: &str) -> Result<bool, ApiError> {
    if st.is_booting(id) {
        return Ok(true);
    }
    if !matches!(store.rig_source(id)?, Some(RigSource::Team { .. })) {
        return Ok(false);
    }
    Ok(store
        .rig_snapshot(id)?
        .is_some_and(|s| s.seats.iter().any(|x| crate::teams::is_live(x.lifecycle))))
}

pub(crate) fn issues_for(st: &AppState, d: &TeamDraft) -> Vec<DraftIssue> {
    let mut issues = rigspec::validate_draft(d);
    if crate::launch::hosted(st) {
        // An agent with no folder works in its own computer's first allowed folder (the runner resolves ".").
        issues.retain(|i| i.code != "folder-missing");
    }
    issues.extend(crate::teams::folder_issues(st, d));
    issues.extend(folder_checks::computer_issues(st, d));
    issues
}

pub(crate) fn record(st: &AppState, draft: TeamDraft) -> Result<TeamRecord, ApiError> {
    let started_revision = match st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .rig_source(&draft.id)?
    {
        Some(RigSource::Team { revision, .. }) => Some(revision),
        _ => None,
    };
    let live = team_live(st, &draft.id)?;
    let issues = issues_for(st, &draft);
    let origin = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .team_origin(&draft.id)?;
    Ok(TeamRecord {
        draft,
        started_revision,
        live,
        issues,
        origin,
    })
}

/// A store error counts as "not taken": `unique_slug` searches without a bound, so
/// treating errors as taken would loop forever while holding the store lock. The
/// following `insert_draft` then surfaces the store error as a 500.
pub(crate) fn new_team_id(store: &Store, name: &str) -> String {
    rigspec::draft::unique_slug(name, "team", |c| {
        RESERVED.contains(&c) || store.name_taken(c).unwrap_or(false)
    })
}

/// What may change while agents run: names, jobs, positions and group labels.
/// Everything else is structure and needs Stop team first.
pub(super) fn structure(d: &TeamDraft) -> Value {
    let mut workflow = d.workflow.clone();
    if let Some(w) = &mut workflow {
        w.input_position = None;
    }
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

pub(super) fn summaries(st: &AppState) -> Result<Vec<TeamSummary>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let rigs = store.list_rigs()?;
    let mut out = Vec::new();
    for (d, updated_at_ms) in store.drafts()? {
        let started = match store.rig_source(&d.id)? {
            Some(RigSource::Team { .. }) => rigs.iter().find(|r| r.name == d.id).cloned(),
            _ => None,
        };
        out.push(TeamSummary {
            id: d.id.clone(),
            name: d.name.clone(),
            agent_count: d.agents.len() as u32,
            updated_at_ms,
            started,
        });
    }
    Ok(out)
}

pub(super) async fn list_teams(State(st): State<AppState>) -> Response {
    crate::teams::respond(summaries(&st))
}

pub(super) async fn list_templates() -> Response {
    Json(rigspec::templates::templates()).into_response()
}

pub(super) async fn get_team(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::teams::respond(load(&st, &id).and_then(|d| record(&st, d)))
}

#[derive(Deserialize, Serialize)]
pub(super) struct CreateReq {
    #[serde(default)]
    pub(super) request_id: String,
    pub(super) template: String,
    #[serde(default)]
    pub(super) practice: bool,
    #[serde(default)]
    pub(super) name: Option<String>,
}

pub(super) async fn create_team(
    State(st): State<AppState>,
    Json(req): Json<CreateReq>,
) -> Response {
    let fp = crate::teams::fingerprint("create-team", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::teams::idempotent(&st, &request_id, fp, move || async move {
        let info = rigspec::templates::templates()
            .into_iter()
            .find(|t| t.id == req.template)
            .ok_or_else(|| {
                ApiError::bad_request(
                    "unknown_template",
                    format!("no template {:?}", req.template),
                )
            })?;
        let name = req
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(&info.default_name)
            .to_string();
        let draft = {
            let mut store = st2.store.lock().expect("store mutex must not be poisoned");
            let id = new_team_id(&store, &name);
            let draft = rigspec::templates::template(&info.id, &id, &name, req.practice)
                .expect("listed template exists");
            store.insert_draft(&draft)?;
            draft
        };
        crate::analytics::emit(
            &st2,
            "team.created",
            serde_json::json!({ "template": crate::analytics::template(&info.id) }),
        );
        crate::teams::to_value(&record(&st2, draft)?)
    })
    .await
}

#[derive(Deserialize, Serialize)]
pub(super) struct PutReq {
    #[serde(default)]
    pub(super) request_id: String,
    pub(super) revision: u64,
    pub(super) draft: TeamDraft,
}

pub(super) fn draft_conflict(current: u64) -> ApiError {
    ApiError::conflict(
        "draft_conflict",
        format!(
            "This team was changed somewhere else (it is now at version {current}). Load the latest version or keep yours."
        ),
    )
}

/// Saves the draft if its revision is current and, while the team runs, only names and jobs
/// changed. One lock for the revision check, the running check and the save, so a start
/// cannot claim the team in between and run the version before this save.
pub(super) fn save_checked(st: &AppState, id: &str, req: &PutReq) -> Result<SaveDraft, ApiError> {
    let mut store = st.store.lock().expect("store mutex must not be poisoned");
    crate::project_visibility::ensure_available(&store, id)?;
    let current = store
        .draft(id)?
        .ok_or_else(|| ApiError::not_found(format!("no team {id:?}")))?;
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

pub(super) async fn put_team(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<PutReq>,
) -> Response {
    let fp = crate::teams::fingerprint(&format!("put-team:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::teams::idempotent(&st, &request_id, fp, move || async move {
        if req.draft.id != id {
            return Err(ApiError::bad_request(
                "team_id_mismatch",
                "the draft's id does not match the URL",
            ));
        }
        match save_checked(&st2, &id, &req)? {
            SaveDraft::Saved(d) => {
                crate::analytics::saved(&st2, &d);
                crate::teams::to_value(&record(&st2, *d)?)
            }
            SaveDraft::Conflict { current } => Err(draft_conflict(current)),
            SaveDraft::Missing => Err(ApiError::not_found(format!("no team {id:?}"))),
        }
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct ValidateReq {
    #[serde(default)]
    pub(super) draft: Option<TeamDraft>,
}

pub(super) fn validate(
    st: &AppState,
    id: &str,
    draft: Option<TeamDraft>,
) -> Result<Vec<DraftIssue>, ApiError> {
    let d = match draft {
        Some(d) => d,
        None => load(st, id)?,
    };
    Ok(issues_for(st, &d))
}

pub(super) async fn validate_team(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ValidateReq>,
) -> Response {
    crate::teams::respond(validate(&st, &id, req.draft))
}

pub(super) async fn team_spec(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    crate::teams::respond(load(&st, &id).map(|d| {
        let issues = issues_for(&st, &d);
        TeamSpec {
            yaml: rigspec::exchange::export_yaml(&d),
            valid: !issues.iter().any(is_error),
            issues,
        }
    }))
}

#[derive(Deserialize, Serialize)]
pub(super) struct ImportReq {
    #[serde(default)]
    pub(super) request_id: String,
    pub(super) yaml: String,
}

pub(super) async fn import_team(
    State(st): State<AppState>,
    Json(req): Json<ImportReq>,
) -> Response {
    let fp = crate::teams::fingerprint("import-team", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::teams::idempotent(&st, &request_id, fp, move || async move {
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
            let mut store = st2.store.lock().expect("store mutex must not be poisoned");
            imported.draft.id = new_team_id(&store, &spec_name);
            store.insert_draft(&imported.draft)?;
        }
        crate::analytics::emit(&st2, "team.created", json!({ "template": "import" }));
        crate::teams::to_value(&TeamImport {
            team: record(&st2, imported.draft)?,
            dropped: imported.dropped,
            warnings: Vec::new(),
        })
    })
    .await
}
