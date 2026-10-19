//! A workflow's dispatch command: instead of a dispatcher agent calling `dispatch_work`, Sculptured
//! runs a command in the project folder when the team starts, and the command's output is the batch.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The command whose standard output is the batch, in the `dispatch_work` shape:
/// `{"batch_id"?, "summary", "tasks": [{id, title, context, mode?, depends_on?}]}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct WorkflowDispatch {
    /// The shell command, run on the team's computer in its project folder.
    pub command: String,
    /// Seconds before the command is stopped (default 600, at most 3600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout_s: Option<u32>,
}

/// What came of running the dispatch command for one start. Sculptured ran the command (so that it
/// ran, and what it printed, is observed); the tasks themselves are the command's own output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct DispatchRecord {
    pub command: String,
    #[ts(type = "number")]
    pub at_ms: i64,
    /// True once its output became the batch.
    pub dispatched: bool,
    /// The command, its exit status and time, then the end of what it printed on stderr.
    pub evidence: String,
    /// Why nothing was dispatched (it failed, timed out, or printed something that is not a batch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub problem: Option<String>,
}
