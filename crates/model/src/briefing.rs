//! How much of the team an agent is told about with its first message.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `full`: the team's architecture, neighbours' jobs and the working rules (the default).
/// `minimal`: only the agent's own role, job, data contract and how to finish, for agents that need
/// nothing but their assignment. Its managed children get the minimal briefing too.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum BriefingLevel {
    #[default]
    Full,
    Minimal,
}
