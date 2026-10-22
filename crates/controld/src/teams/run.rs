//! Start and stop a team.
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::reconciler;
use crate::state::AppState;
use crate::store::RigSource;
use crate::views::StopRigOutcome;
use axum::extract::State;
use axum::response::Response;
use model::{ConnectionKind, TeamDraft};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

/// True when another agent of the team hands work to `agent`: its Job then arrives
/// with its first handoff, not at start.
pub(crate) fn has_upstream(d: &TeamDraft, agent: &str) -> bool {
    d.connections.iter().any(|c| {
        c.kind == ConnectionKind::HandsWorkTo
            && c.to == agent
            && c.from != agent
            && d.agent(&c.from).is_some()
    })
}

/// `(seat id, job)` for every agent nobody hands work to with a non-empty Job, or the project's
/// Input pointing to it (`TeamDraft::job_with_input`).
fn jobs(d: &TeamDraft) -> Vec<(String, String)> {
    d.agents
        .iter()
        .filter(|a| d.workflow.as_ref().is_none_or(|w| a.id == w.dispatcher))
        .filter(|a| !d.job_with_input(a).is_empty() && !has_upstream(d, &a.id))
        .filter_map(|a| Some((d.seat_id(&a.id)?, match &d.workflow {
            Some(w) => format!("{}\n\nOperator Input block:\n{}\n\nIf Input is empty, ask the operator for the source and goal; do not invent a textbook or dispatch placeholder work.", a.job.trim(), w.input),
            None => d.job_with_input(a),
        })))
        .collect()
}

fn team_running() -> ApiError {
    ApiError::conflict(
        "team_running",
        "This team is already running. Stop it before starting it again.",
    )
}

#[derive(Deserialize, Serialize)]
pub(super) struct StartReq {
    #[serde(default)]
    request_id: String,
    /// The revision the client saw; a newer stored draft is a conflict.
    #[serde(default)]
    revision: Option<u64>,
    /// Start although a plan this team uses is near its limit (`plan_usage::held`).
    #[serde(default)]
    over_plan_limit: bool,
    /// Each agent continues its last conversation where it has one; otherwise it starts fresh.
    #[serde(default)]
    resume: bool,
}

/// Agents of a stopped team with a last conversation to resume: (seat, name).
fn resumable_agents(st: &AppState, id: &str) -> Result<Vec<(String, String)>, ApiError> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    let Some(snap) = store.rig_snapshot(id)? else {
        return Ok(vec![]);
    };
    let draft = store.draft(id)?;
    let name = |member: &str| {
        draft
            .as_ref()
            .and_then(|d| {
                d.agents
                    .iter()
                    .find(|a| a.id == member)
                    .map(|a| a.name.clone())
            })
            .unwrap_or_else(|| member.to_string())
    };
    Ok(snap
        .seats
        .into_iter()
        .filter(|s| {
            matches!(
                s.runtime,
                rigspec::Runtime::ClaudeCode | rigspec::Runtime::Codex
            ) && s.native_session_id.is_some()
                && store.parent(&s.id).ok().flatten().is_none()
        })
        .map(|s| (s.id.clone(), name(&s.member)))
        .collect())
}

pub(super) async fn resumable(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    super::respond(resumable_agents(&st, &id).map(|a| json!({ "agents": a.into_iter().map(|(seat, name)| json!({ "seat": seat, "name": name })).collect::<Vec<_>>() })))
}

/// Remembers, for `launch_one`, each agent to resume and its session's usage so far.
fn plan_resumes(st: &AppState, id: &str) -> Result<usize, ApiError> {
    let agents = resumable_agents(st, id)?;
    for (seat, _) in &agents {
        let session = st
            .store
            .lock()
            .expect("store mutex must not be poisoned")
            .seat(seat)?
            .and_then(|v| v.native_session_id);
        if let Some(session) = session {
            let baseline = crate::restore::usage_baseline(st, seat)?;
            st.start_resumes
                .lock()
                .expect("start_resumes mutex must not be poisoned")
                .insert(
                    seat.clone(),
                    crate::state::ResumeSession {
                        native_session_id: session,
                        usage_baseline: baseline,
                    },
                );
        }
    }
    Ok(agents.len())
}

/// The draft's build, when it is the revision the client saw (if it said) and it can start:
/// no issue in the draft, its spec or its folders.
fn startable(
    st: &AppState,
    draft: &TeamDraft,
    seen: Option<u64>,
) -> Result<rigspec::DraftBuild, ApiError> {
    if let Some(rev) = seen
        && rev != draft.revision
    {
        return Err(ApiError::conflict(
            "draft_conflict",
            format!(
                "This team was changed somewhere else (it is now at version {}). Load the latest version before starting.",
                draft.revision
            ),
        ));
    }
    let folders = crate::teams::folder_issues(st, draft);
    match rigspec::from_draft(draft) {
        Ok(b) if !folders.iter().any(crate::teams::is_error) => Ok(b),
        Ok(b) => Err(ApiError::invalid_team([b.warnings, folders].concat())),
        Err(mut issues) => {
            issues.extend(folders);
            Err(ApiError::invalid_team(issues))
        }
    }
}

pub(super) async fn start_team(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<StartReq>,
) -> Response {
    let fp = crate::api::fingerprint(&format!("start-team:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::api::idempotent(&st, &request_id, fp, move || async move {
        // Claim before reading the draft: a save checked before this claim is already
        // stored, and one checked after it sees the team as starting (`team_live_in`).
        let Some(claim) = st2.try_begin_boot(&id) else {
            return Err(team_running());
        };
        crate::project_visibility::ensure_available(&st2.store.lock().expect("store mutex must not be poisoned"), &id)?;
        let draft = crate::teams::load(&st2, &id)?;
        if st2.store.lock().expect("store mutex must not be poisoned").archived(&id)? { return Err(ApiError::conflict("project_archived","Restore this project before starting it")); }
        let mut build = startable(&st2, &draft, req.revision)?;
        let chosen = st2.store.lock().expect("store mutex must not be poisoned").team_settings(&id)?.host.filter(|h| !h.is_empty());
        let host = crate::launch::team_host(&st2, &id);
        let hosted = matches!(st2.auth, crate::auth::AuthPolicy::Gateway { .. });
        let own = st2.store.lock().expect("store mutex must not be poisoned").team_settings(&id)?.agent_hosts;
        let own_computer = |agent: &str| own.get(agent).is_some_and(|h| !h.is_empty());
        // The project folder is a path on the team's computer: an agent on another computer without
        // a folder of its own works in that computer's own folder (its runner resolves ".").
        for seat in &mut build.rig.seats {
            if own_computer(&seat.member) && draft.agents.iter().any(|a| a.id == seat.member && a.folder.as_deref().is_none_or(|f| f.trim().is_empty())) {
                seat.cwd = ".".into();
            }
        }
        let everyone_own = !draft.agents.is_empty() && draft.agents.iter().all(|a| own_computer(&a.id));
        if hosted && !everyone_own && chosen.is_none() && st2.hosts.connected().len() > 1 {
            return Err(ApiError::conflict("needs_computer", "Choose which computer runs this team: Design → Run on."));
        }
        // An agent on its own computer needs that computer; the start names it and the agent.
        if let Some((agent, h)) = draft.agents.iter().find_map(|a| own.get(&a.id).filter(|h| !h.is_empty() && !st2.hosts.is_connected(h)).map(|h| (a, h))) {
            return Err(ApiError::conflict("needs_computer", format!("{h} is not connected, and {} runs there. Open Computers to connect it, or choose another computer for {}.", agent.name, agent.name)));
        }
        if !everyone_own && (hosted || chosen.is_some()) && !st2.hosts.is_connected(&host) {
            return Err(match chosen {
                Some(h) => ApiError::conflict("needs_computer", format!("{h} is not connected. Open Computers to connect it.")),
                None => ApiError::conflict("needs_computer", "Connect a computer first: Computers → Connect a computer."),
            });
        }
        if !req.over_plan_limit {
            let mut providers: Vec<&str> = build.rig.seats.iter().filter_map(|s| crate::plan_usage::provider_of(s.runtime)).collect();
            providers.sort_unstable();
            providers.dedup();
            if let Some(why) = providers.iter().find_map(|p| crate::plan_usage::held(&st2, p)) {
                crate::analytics::emit(&st2, "plan.held", json!({}));
                return Err(ApiError::conflict("plan_limit", why));
            }
        }
        // Practice agents without a folder run in the home folder ("." resolves against it).
        let spec_dir = st2.home.as_deref().cloned().unwrap_or_else(|| PathBuf::from("/"));
        let after_seq = st2.store.lock().expect("store mutex must not be poisoned").last_seq()?;
        let source = RigSource::Team { id: id.clone(), revision: draft.revision };
        // The run's budget is recorded before any agent launches, so the gate never sees an earlier run's.
        crate::budget::begin(&st2, &id, &draft.budget, after_seq)?;
        if req.resume {
            plan_resumes(&st2, &id)?;
        }
        reconciler::start_rig_claimed(&st2, build.rig.clone(), source, spec_dir, claim).map_err(|e| {
            let _ = crate::budget::forget(&st2, &id, after_seq);
            match e.code.as_ref() {
                "rig_running" => team_running(),
                _ => e,
            }
        })?;
        let (st3, team, layers) = (st2.clone(), id.clone(), build.rig.launch_layers.len());
        let (jobs, mapped) = (jobs(&draft), draft.clone());
        // Agents start while the project map is made; their first messages carry it.
        tokio::spawn(async move {
            crate::knowledge::refresh(&st3, &team, &mapped).await;
            reconciler::deliver_jobs(st3, jobs, after_seq, layers).await;
        });
        crate::analytics::team_started(&st2, &draft);
        Ok(json!({ "rig": build.rig.name, "warnings": build.warnings }))
    })
    .await
}

#[derive(Deserialize, Serialize)]
pub(super) struct StopReq {
    #[serde(default)]
    request_id: String,
}

pub(super) async fn stop_team(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<StopReq>,
) -> Response {
    let fp = crate::api::fingerprint(&format!("stop-team:{id}"), &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::api::idempotent(&st, &request_id, fp, move || async move {
        let started = matches!(
            st2.store.lock().unwrap().rig_source(&id)?,
            Some(RigSource::Team { .. })
        );
        if !started {
            return Err(ApiError::conflict(
                "team_not_started",
                "This team has not been started yet.",
            ));
        }
        // A stop must not interleave with a start: recording "stopped" in the middle of a
        // boot (or of a restart that began after this check) would abandon that boot and
        // mark its Jobs "not sent". So a stop takes the team's boot claim for its whole
        // run: it is refused while a start holds the claim, and a start that comes while
        // the stop runs is refused as `team_running`.
        let Some(_stopping) = st2.try_begin_boot(&id) else {
            return Err(ApiError::conflict(
                "team_starting",
                "This team is still starting. Stop it once it has finished starting.",
            ));
        };
        // Same body as `POST /api/rigs/{name}/stop`: the agents that could not be stopped, with the host's reason.
        let analytics = st2.clone();
        let failed = reconciler::stop_rig(st2, id).await?;
        crate::analytics::emit(&analytics, "team.stopped", json!({}));
        crate::error::to_value(&StopRigOutcome { failed })
    })
    .await
}
