//! Settings, for everyone and per team.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use model::PermissionPosture;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Settings {
    #[serde(default)]
    #[ts(optional)]
    pub community_url: Option<String>,
    /// D7: unrestricted by default.
    pub posture: PermissionPosture,
    /// D8: 10 minutes by default. 1 to 1440.
    pub question_timeout_min: u32,
    /// Where agents' own git copies go (`copies`); None = `~/.sculptured/copies`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub copies_folder: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { community_url: None, posture: PermissionPosture::Unrestricted, question_timeout_min: 10, copies_folder: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TeamSettings {
    /// None: use the global setting.
    pub posture: Option<PermissionPosture>,
    pub question_timeout_min: Option<u32>,
    /// Each Claude Code / Codex agent works in its own git worktree and branch (`copies`).
    #[serde(default)]
    pub own_copies: bool,
    /// The computer (host id) this team runs on; None: automatic (hosted: the only connected computer).
    #[serde(default)]
    pub host: Option<String>,
    /// Agents that run on another computer than the team's: agent id → host id. Like `host`, it says
    /// where this team runs, so it is never part of a team file.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    #[ts(as = "Option<std::collections::BTreeMap<String, String>>", optional)]
    pub agent_hosts: std::collections::BTreeMap<String, String>,
}
