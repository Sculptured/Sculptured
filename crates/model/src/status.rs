use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum SeatLifecycle {
    Planned,
    Launching,
    Running,
    Stopping,
    Stopped,
    Stale,
    Attention,
    Failed,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum SeatActivity {
    Working,
    WaitingInput,
    Idle,
    Unknown,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum WorkState {
    Pending,
    InProgress,
    Blocked,
    Done,
    Failed,
    Canceled,
    HandedOff,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum OutcomeEvidence {
    ReportedComplete,
    CapturedTool,
    Verified,
    SettledUnverified,
    CheckFailed,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TenureCause {
    Fresh,
    Resume,
    Handover,
    Fork,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TurnStatus {
    Completed,
    Failed,
    Interrupted,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum MessageIntent {
    Inform,
    Instruct,
}

/// Delivery only moves forward: queued → delivered → consumed. `failed` is terminal.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum MessageState {
    Queued,
    Delivered,
    Consumed,
    Failed,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Desired {
    Running,
    Stopped,
}
