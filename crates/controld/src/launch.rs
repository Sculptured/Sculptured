//! How controld launches a seat: which driver, with which parameters. One place for every launch path
//! (boot, Restart this agent, new session, restore), so posture and seat tools apply the same way everywhere.
use crate::error::ApiError;
use crate::hosts::LOCAL_HOST;
use crate::state::AppState;
use crate::store::RigSource;
use host_proto::{HostCommand, LaunchMode, LaunchParams};
use model::{SeatLifecycle, TenureCause};
use rigspec::{CompiledRig, CompiledSeat, Runtime};
use std::path::{Path, PathBuf};

/// The hostd driver for an agent type; None while that type cannot run.
pub fn driver_for(rt: Runtime) -> Option<&'static str> {
    match rt {
        Runtime::ClaudeCode => Some("claude-stream"),
        Runtime::Codex => Some("codex-app-server"),
        Runtime::Stub => Some("stub"),
        _ => None,
    }
}

/// The launch parameters for `seat` on `host`, fitted to what it can do (`hosts::fit_params`). Err (in
/// plain words) when the host is an older hostd that cannot honour them: a restricted agent is refused,
/// never run unrestricted.
pub fn launch_params(st: &AppState, host: &str, rig: &str, seat: &CompiledSeat, spec_dir: &Path) -> Result<LaunchParams, String> {
    fitted_params(st, host, rig, seat, spec_dir, None)
}

/// `launch_params` with a usage baseline (a resumed session's usage so far), fitted with the rest: an
/// older runner that cannot seed it drops it with a note instead of refusing the launch.
fn fitted_params(
    st: &AppState,
    host: &str,
    rig: &str,
    seat: &CompiledSeat,
    spec_dir: &Path,
    usage_baseline: Option<[u64; 4]>,
) -> Result<LaunchParams, String> {
    if st.store.lock().unwrap().budget_exhausted(&seat.id).map_err(|e|e.to_string())? {return Err("Agent output budget exhausted; start a new project run to reset it".into());}
    let posture = crate::posture::for_seat(st, rig, seat);
    let mut cwd = cwd_for(st, spec_dir, &seat.cwd);
    // copies: per-item gives agents the team's items folder instead of their own copies.
    let per_item = st.store.lock().unwrap().draft(rig).ok().flatten().and_then(|d| d.workflow).is_some_and(|w| w.copies == model::CopiesMode::PerItem);
    let local = host == LOCAL_HOST && !hosted(st);
    if local && !per_item && matches!(seat.runtime, Runtime::ClaudeCode | Runtime::Codex) && crate::copies::enabled(st, rig) {
        cwd = crate::copies::prepare(st, rig, &seat.id, Path::new(&cwd))?.display().to_string();
    }
    let own_copy = own_copy_for(st, rig, &seat.id, seat.runtime).filter(|_| !per_item);
    let (item_copies, add_dirs) = match (per_item, local) {
        (false, _) => (None, vec![]),
        (true, false) => (Some(rig.to_string()), vec![]),
        (true, true) => {
            let dir = localops::copies::items_root(&crate::copies::copies_root(st)?, rig);
            std::fs::create_dir_all(&dir).map_err(|e| format!("Can't make {}: {e}", dir.display()))?;
            (None, vec![dir.display().to_string()])
        }
    };
    let (claude_setup, protection) = claude_settings(st, rig, seat);
    let compact_tokens = codex_compaction(st, rig, seat);
    // Given back after a compaction, which can summarize it away (Claude only: its SessionStart hook).
    let briefing = protection.as_ref().and_then(|_| st.store.lock().unwrap().draft(rig).ok().flatten()).and_then(|d| crate::team_context::briefing(&d, &seat.id));
    let (skills, integrations, components) = agent_extras(st, rig, seat);
    let mut params = LaunchParams {
        skills,
        integrations,
        components,
        protection,
        briefing,
        claude_setup,
        compact_tokens,
        cwd,
        model: seat.model.clone(),
        posture,
        seat_tools: crate::seat_tools::launch_params(st, seat, posture),
        usage_baseline,
        own_copy,
        item_copies,
        add_dirs,
        ..Default::default()
    };
    crate::hosts::fit_params(st, host, &seat.id, &mut params)?;
    Ok(params)
}

/// The computer a team runs on: its setting, else (hosted) the only connected computer, else this machine.
pub fn team_host(st: &AppState, rig: &str) -> String {
    if let Some(h) = st.store.lock().unwrap().team_settings(rig).ok().and_then(|t| t.host).filter(|h| !h.is_empty()) {
        return h;
    }
    if hosted(st) {
        if let [only] = st.hosts.connected().as_slice() {
            return only.clone();
        }
    }
    LOCAL_HOST.to_string()
}

/// The computer an agent runs on: its own (`TeamSettings.agent_hosts`), else the team's.
pub fn agent_host(st: &AppState, seat: &str) -> String {
    let rig = crate::seat_ids::rig_or_empty(seat);
    let own = st.store.lock().unwrap().team_settings(rig).ok().and_then(|t| t.agent_hosts.get(crate::seat_ids::member_of(seat)).cloned());
    own.filter(|h| !h.is_empty()).unwrap_or_else(|| team_host(st, rig))
}

fn hosted(st: &AppState) -> bool {
    matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. })
}

/// Where the agent runs. Hosted: the team's folder (or a relative path) means something only on the
/// user's computer, whose runner resolves it within its allowed folders.
pub fn cwd_for(st: &AppState, spec_dir: &Path, seat_cwd: &str) -> String {
    if hosted(st) { seat_cwd.to_string() } else { crate::reconciler::resolve_cwd(spec_dir, seat_cwd).display().to_string() }
}

/// Hosted teams with own copies: the computer that runs the agent makes its copy (`LaunchParams.own_copy`).
pub fn own_copy_for(st: &AppState, rig: &str, seat: &str, runtime: Runtime) -> Option<host_proto::OwnCopy> {
    (hosted(st) && matches!(runtime, Runtime::ClaudeCode | Runtime::Codex) && crate::copies::enabled(st, rig))
        .then(|| host_proto::OwnCopy { team: rig.to_string(), parent: st.store.lock().unwrap().parent(seat).ok().flatten() })
}

/// Where a rig's relative folders resolve: a team's agents run from the home folder; a spec file's from its folder.
pub fn spec_dir(st: &AppState, rig: &str) -> PathBuf {
    let home = || st.home.as_deref().cloned().unwrap_or_else(|| PathBuf::from("/"));
    match st.store.lock().unwrap().rig_source(rig).ok().flatten() {
        Some(RigSource::File { path }) => {
            Path::new(&path).canonicalize().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(home)
        }
        _ => home(),
    }
}

/// The compiled rig and seat for a seat id.
pub fn compiled_seat(st: &AppState, seat: &str) -> Result<(CompiledRig, CompiledSeat), ApiError> {
    let missing = || ApiError::not_found(format!("no agent {seat:?}"));
    let rig = crate::seat_ids::rig_of(seat).ok_or_else(missing)?;
    let snap = st.store.lock().unwrap().rig_snapshot(rig)?.ok_or_else(missing)?;
    let spec = snap.rig.seats.iter().find(|s| s.id == seat).cloned().ok_or_else(missing)?;
    Ok((snap.rig, spec))
}

pub fn seat_host(st: &AppState, seat: &str) -> Result<String, ApiError> {
    let view = st.store.lock().unwrap().seat(seat)?.ok_or_else(|| ApiError::not_found(format!("no agent {seat:?}")))?;
    Ok(view.host.unwrap_or_else(|| agent_host(st, seat)))
}

/// A new tenure for a seat that has no live process (new session, restore), then waits for it to settle.
/// Returns the lifecycle it settled in.
pub async fn relaunch(
    st: &AppState,
    seat: &str,
    mode: LaunchMode,
    cause: TenureCause,
    usage_baseline: Option<[u64; 4]>,
) -> Result<SeatLifecycle, ApiError> {
    let (rig, spec) = compiled_seat(st, seat)?;
    let driver = driver_for(spec.runtime).ok_or_else(|| {
        ApiError::conflict("agent_not_running", format!("{} agents can't run in this version yet", crate::reconciler::runtime_words(spec.runtime)))
    })?;
    let host = seat_host(st, seat)?;
    // An older runner cannot restart an agent; refuse before anything is recorded for this launch.
    st.hosts.check_features(&host, &[host_proto::features::RESTART]).map_err(|e| ApiError::conflict("older_runner", e))?;
    let params = fitted_params(st, &host, &rig.name, &spec, &spec_dir(st, &rig.name), usage_baseline)
        .map_err(|e| ApiError::conflict("older_runner", e))?;
    st.store.lock().unwrap().set_seat_cwd(seat, &params.cwd)?;
    let cmd = HostCommand::Restart { seat: seat.to_string(), driver: driver.to_string(), params, mode, cause };
    st.hosts.command(&host, cmd).await.map_err(|e| {
        if e == crate::hosts::older_runner(&host) { ApiError::conflict("older_runner", e) } else { ApiError::bad_gateway("host_unreachable", e) }
    })?;
    crate::reconciler::wait_settled(st, &[seat.to_string()]).await;
    crate::reconciler::lifecycle(st, seat).ok_or_else(|| ApiError::internal(format!("agent {seat} has no lifecycle")))
}

/// The agent's data contract added to its protections. Relative paths stay relative: the guard reads
/// them against the folder the agent actually works in (its own copy, on a connected computer).
fn with_contract(mut rules: model::GuardRules, agent: Option<&model::DraftAgent>) -> model::GuardRules {
    let Some(c) = agent.and_then(|a| a.advanced.data_access.as_ref()) else { return rules };
    let clean = |p: &String| p.trim().to_string();
    rules.reads = c.reads.iter().map(clean).filter(|p| !p.is_empty()).collect();
    rules.writes = c.writes.iter().map(clean).filter(|p| !p.is_empty()).collect();
    rules
}

/// The agent's skills and integrations, for agents of a real CLI.
fn agent_extras(st: &AppState, rig: &str, seat: &CompiledSeat) -> (Vec<model::Skill>, Vec<String>, Vec<model::Component>) {
    if !matches!(seat.runtime, Runtime::ClaudeCode | Runtime::Codex) {
        return (vec![], vec![], vec![]);
    }
    let draft = st.store.lock().unwrap().draft(rig).ok().flatten();
    let Some((d, a)) = draft.as_ref().and_then(|d| d.agent_for_seat(&seat.id).map(|a| (d, a))) else { return (vec![], vec![], vec![]) };
    // With what each needs (Signet brings Kindex); tool hooks and plugins only for Claude Code, since
    // Codex runs hooks only after the person trusts them. Skills components join the agent's skills here.
    let components: Vec<model::Component> = d.agent_components(a).into_iter()
        .filter(|c| (!c.claude_only || seat.runtime == Runtime::ClaudeCode) && c.kind != model::ComponentKind::Skills)
        .collect();
    // The built-in ones by name too, for runners from before components.
    let names = components.iter().filter(|c| c.is_builtin()).map(|c| c.id.clone()).collect();
    (d.agent_skills(a), names, components)
}

/// Codex agents: the team's context ceiling acts between turns (notes, then a fresh conversation); a
/// long turn can grow far past it, so Codex compacts within a turn at twice the ceiling.
fn codex_compaction(st: &AppState, rig: &str, seat: &CompiledSeat) -> Option<u64> {
    if seat.runtime != Runtime::Codex {
        return None;
    }
    let draft = st.store.lock().unwrap().draft(rig).ok().flatten();
    draft.map(|d| d.protection).unwrap_or_default().context_ceiling().map(|c| 2 * c)
}

/// Claude agents only: the part of the person's own Claude Code setup the team allows (None when all of
/// it is, as before), and the team's protections with the agent's contract (on unless turned off).
fn claude_settings(st: &AppState, rig: &str, seat: &CompiledSeat) -> (Option<model::ClaudeSetup>, Option<model::GuardRules>) {
    if seat.runtime != Runtime::ClaudeCode {
        return (None, None);
    }
    let draft = st.store.lock().unwrap().draft(rig).ok().flatten();
    let setup = draft.as_ref().map(|d| d.claude_setup).unwrap_or_default();
    let protection = draft.as_ref().map(|d| d.protection).unwrap_or_default().guard_rules()
        .map(|rules| with_contract(rules, draft.as_ref().and_then(|d| d.agent_for_seat(&seat.id))))
        .map(|mut rules| { rules.safety = draft.as_ref().map(|d| d.safety.clone()).unwrap_or_default(); rules });
    ((!(setup.global && setup.project)).then_some(setup), protection)
}
