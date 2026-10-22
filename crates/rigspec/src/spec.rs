use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RigSpec {
    pub version: String,
    pub name: String,
    pub summary: Option<String>,
    pub culture_file: Option<String>,
    pub permission_policy: Option<String>,
    pub managed_blocks: Option<Value>,
    pub workspace: Option<Value>,
    pub docs: Option<Value>,
    pub startup: Option<Value>,
    pub services: Option<Value>,
    pub pods: Vec<PodSpec>,
    #[serde(default)]
    pub edges: Vec<EdgeSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PodSpec {
    pub id: String,
    pub label: String,
    pub summary: Option<String>,
    pub continuity_policy: Option<Value>,
    pub startup: Option<Value>,
    pub members: Vec<MemberSpec>,
    #[serde(default)]
    pub edges: Vec<EdgeSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberSpec {
    pub id: String,
    pub agent_ref: String,
    pub profile: String,
    pub runtime: String,
    pub cwd: String,
    pub label: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub permission_policy: Option<String>,
    pub role: Option<String>,
    pub restore_policy: Option<String>,
    pub compaction_strategy: Option<String>,
    pub mechanic: Option<String>,
    pub codex_config_profile: Option<String>,
    pub session_source: Option<Value>,
    pub starter_ref: Option<Value>,
    pub startup: Option<Value>,
    /// Sculptured: the person's model connection (an id the gateway keeps) a Claude Code member
    /// runs with. Never written to a team file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeSpec {
    pub kind: String,
    pub from: String,
    pub to: String,
}
