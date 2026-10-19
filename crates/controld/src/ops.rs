//! Where local operations run: in this process (local mode), or on the team's connected computer
//! (hosted mode), as the same `localops::LocalOp`. Routes call `run` and never care which.
use crate::error::ApiError;
use crate::state::AppState;
use axum::http::StatusCode;
use localops::LocalOp;
use std::time::Duration;

/// Which computer an op is for, in hosted mode.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
    /// The team's computer (`launch::team_host`).
    Team(&'a str),
    /// A computer by id (`?host=`).
    Host(&'a str),
    /// The only connected computer.
    Default,
}

pub fn hosted(st: &AppState) -> bool {
    matches!(st.auth, crate::auth::AuthPolicy::Gateway { .. })
}

/// How long a computer may take before the request answers `computer_timeout`.
pub fn timeout_for(op: &LocalOp) -> Duration {
    Duration::from_secs(match op {
        LocalOp::Describe | LocalOp::ListFolder { .. } | LocalOp::CreateFolder { .. } | LocalOp::PlanUsage => 15,
        LocalOp::GitStatus { .. } | LocalOp::GitDiff { .. } | LocalOp::GitAct { .. } => 30,
        LocalOp::Clis => 40,
        LocalOp::Generate { .. } => 310,
        LocalOp::AnalyzeRun { .. } => 60,
        LocalOp::ProjectMap { .. } => 30,
        LocalOp::KeepLesson { .. } => 15,
        LocalOp::CliLogin { .. } => 20,
        LocalOp::CliLoginCode { .. } => 15,
        LocalOp::CopyPrepare { .. } | LocalOp::CopiesRead { .. } | LocalOp::CopyMerge { .. } | LocalOp::CopyRemove { .. } => 120,
        LocalOp::ItemCopyPrepare { .. } | LocalOp::ItemMerge { .. } => 120,
        LocalOp::RunCheck { timeout_s, .. } => u64::from(*timeout_s) + 30,
    })
}

/// This machine's context for local mode.
pub fn local_context(st: &AppState) -> Result<localops::Context, ApiError> {
    let home = st.home.as_deref().cloned().ok_or_else(|| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "home_unknown", "Your home folder is unknown (HOME is not set)"))?;
    let copies_root = crate::copies::copies_root(st).map_err(|e| ApiError::conflict("copy_unavailable", e))?;
    let codex_home = std::env::var_os("CODEX_HOME").map(Into::into).unwrap_or_else(|| home.join(".codex"));
    Ok(localops::Context { home, copies_root, claude: st.cli_program("claude"), codex: st.cli_program("codex"), codex_home })
}

/// The computer an op goes to in hosted mode, or why there is none.
pub fn host_for(st: &AppState, target: Target<'_>) -> Result<String, ApiError> {
    let host = match target {
        Target::Team(team) => crate::launch::team_host(st, team),
        Target::Host(h) => h.to_string(),
        Target::Default => match st.hosts.connected().as_slice() {
            [only] => only.clone(),
            [] => return Err(ApiError::conflict("needs_computer", "Connect a computer first: Computers → Connect a computer.")),
            _ => return Err(ApiError::conflict("needs_computer", "Choose which computer: more than one is connected.")),
        },
    };
    if !st.hosts.is_connected(&host) {
        return Err(ApiError::conflict("needs_computer", format!("{host} is not connected. Open Computers to connect it.")));
    }
    Ok(host)
}

/// Runs `op` here (local mode) or on the target computer (hosted mode).
pub async fn run(st: &AppState, target: Target<'_>, op: LocalOp) -> Result<serde_json::Value, ApiError> {
    if !hosted(st) {
        return Ok(localops::execute(&local_context(st)?, op).await?);
    }
    let host = host_for(st, target)?;
    let timeout = timeout_for(&op);
    let request = serde_json::to_value(&op).expect("ops serialize");
    match st.hosts.op(&host, request, timeout).await {
        Ok(v) => Ok(v),
        Err(crate::hosts::OpFailure::NotConnected) => Err(ApiError::conflict("needs_computer", format!("{host} disconnected. Open Computers to reconnect it."))),
        Err(crate::hosts::OpFailure::OlderRunner) => Err(ApiError::conflict("older_runner", format!("{host}'s Sculptured is too old for this. Update it and run `sculptured connect` again."))),
        Err(crate::hosts::OpFailure::Timeout) => Err(ApiError::new(StatusCode::GATEWAY_TIMEOUT, "computer_timeout", format!("{host} did not answer in {} s.", timeout.as_secs()))),
        Err(crate::hosts::OpFailure::Failed(e)) => Err(localops::LocalError { status: e.status, code: e.code, message: e.message }.into()),
    }
}
