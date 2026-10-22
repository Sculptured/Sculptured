//! A reusable board fragment stored independently from projects and runs.
use crate::TeamDraft;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct GroupBlock {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub revision: u64,
    pub draft: TeamDraft,
    /// Cross-boundary connections from the source, for the operator to reconnect on insertion.
    pub boundary_notes: Vec<String>,
    /// The Community block (and version) this Library copy was saved from. Never published or exported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<BlockSource>,
}

/// Where a Library copy came from in Community.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct BlockSource {
    pub community_block: String,
    #[ts(type = "number")]
    pub version: u32,
}
