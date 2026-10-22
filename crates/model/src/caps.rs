use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What a driver can do. The UI disables controls whose capability is false and says why.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct Caps {
    pub interrupt: bool,
    pub questions: bool,
    pub resume: bool,
    pub fork: bool,
    pub usage: bool,
    pub mid_turn_input: bool,
    pub subagent_events: bool,
}
