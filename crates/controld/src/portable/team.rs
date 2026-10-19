//! A team's structure with the settings that belong to it (not which computer runs it): what a
//! MADL document carries, before it is written as one (`super::madl`) or after it is read.
use crate::store::Store;
use crate::error::ApiError;
use crate::state::AppState;
use crate::views::TeamSettings;
use model::{DraftIssue, TeamDraft};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PortableTeam {
    pub team: model::TeamDraft,
    #[serde(default)]
    pub settings: PortableSettings,
}

/// The team settings that are part of its structure.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PortableSettings {
    /// None: the global setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posture: Option<model::PermissionPosture>,
    /// Each Claude Code / Codex agent works in its own git copy.
    #[serde(default)]
    pub own_copies: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_timeout_min: Option<u32>,
}

/// The team as saved (`include_folders`: keep this computer's folder paths).
pub fn portable(mut team: TeamDraft, s: &TeamSettings, include_folders: bool) -> PortableTeam {
    team.revision = 0;
    if !include_folders {
        team.folder = None;
        for a in &mut team.agents {
            a.folder = None;
        }
    }
    let settings = PortableSettings { posture: s.posture, own_copies: s.own_copies, question_timeout_min: s.question_timeout_min };
    PortableTeam { team, settings }
}

pub fn export_team(st: &AppState, id: &str, include_folders: bool) -> Result<PortableTeam, ApiError> {
    let store = st.store.lock().unwrap();
    saved(&store, id, include_folders)
}

pub(crate) fn saved(store: &Store, id: &str, include_folders: bool) -> Result<PortableTeam, ApiError> {
    let team = store.draft(id)?.ok_or_else(|| ApiError::not_found(format!("no team {id:?}")))?;
    Ok(portable(team, &store.team_settings(id)?, include_folders))
}

/// The team checked, with its issues; every issue when it does not hold together. Creates nothing.
pub fn check(st: &AppState, team: PortableTeam) -> Result<(PortableTeam, Vec<DraftIssue>), ApiError> {
    let issues = crate::teams::issues_for(st, &team.team);
    if issues.iter().any(|i| crate::teams::is_error(i) && BROKEN.contains(&i.code.as_str())) {
        return Err(ApiError::invalid_team(issues));
    }
    Ok((team, issues))
}

/// Errors that mean the team does not hold together (ids, references, malformed parts). Other issues
/// — a missing folder or name, an empty team — are what Design shows on any draft, so the team is
/// created and they come back as warnings.
const BROKEN: &[&str] = &[
    "team-id-invalid", "agent-id-invalid", "group-id-invalid", "agent-group-missing", "connection-dangling",
    "connection-duplicate", "workflow-invalid", "team-config", "group-config", "agent-config", "connection-config",
];

/// Inserts the team under a fresh id, with its settings.
pub(crate) fn create(st: &AppState, team: PortableTeam) -> Result<TeamDraft, ApiError> {
    let mut draft = team.team;
    let mut store = st.store.lock().unwrap();
    draft.id = crate::teams::new_team_id(&store, &draft.name);
    draft.revision = 0;
    store.insert_draft(&draft)?;
    let s = team.settings;
    store.save_team_settings(&draft.id, &TeamSettings { posture: s.posture, question_timeout_min: s.question_timeout_min, own_copies: s.own_copies, host: None })?;
    Ok(draft)
}
