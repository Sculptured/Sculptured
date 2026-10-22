//! An agent's contract (MADL `data_access` and `authority`): what it reads and writes, what else it
//! changes, and the decisions it owns. Sculptured tells the team, and its protections refuse file
//! reads and writes outside the contract where they can tell.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Paths relative to the agent's folder; a trailing `/` or `/**` covers a folder.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct DataAccess {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub reads: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub writes: Vec<String>,
    /// Effects beyond its files, in plain words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub side_effects: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rationale: Option<String>,
    /// Values other agents rely on, each with the file that holds it (an idea from Constrain's
    /// boundary register). Every start checks the file still contains it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<SharedValue>>", optional)]
    pub shared: Vec<SharedValue>,
}

impl DataAccess {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reads.is_empty()
            && self.writes.is_empty()
            && self.side_effects.is_empty()
            && self.rationale.is_none()
            && self.shared.is_empty()
    }
}

/// A value at a boundary between agents, such as a port, an endpoint or a field name, and the file
/// (relative to the agent's folder) that must contain it exactly.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct SharedValue {
    pub name: String,
    pub value: String,
    pub file: String,
}

impl SharedValue {
    /// `NAME = value @ path`, the way people write them.
    #[must_use]
    pub fn parse(line: &str) -> Option<SharedValue> {
        let (name, rest) = line.split_once('=')?;
        let (value, file) = rest.rsplit_once('@')?;
        let v = SharedValue {
            name: name.trim().into(),
            value: value.trim().into(),
            file: file.trim().into(),
        };
        (!v.name.is_empty() && !v.value.is_empty() && !v.file.is_empty()).then_some(v)
    }
}

/// The decisions an agent owns, in plain words.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct Authority {
    #[serde(default)]
    pub domains: Vec<String>,
    /// How far its decisions reach: a group id, or `system` for the whole team. None: its own work.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rationale: Option<String>,
}
