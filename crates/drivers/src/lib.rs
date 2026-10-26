pub mod claude;
pub mod codex;
pub mod opencode;
mod process;
pub mod stub;

use async_trait::async_trait;
use futures::stream::BoxStream;
use model::{
    Answer, Caps, ContextBasis, LimitKind, MessageIntent, PermissionPosture, QuestionPrompt,
    QuestionSource, SeatId, ThinkingForm, TurnStatus,
};
use std::path::PathBuf;
use std::time::Duration;

mod driver;
pub(crate) use driver::check_cwd;
pub use driver::{
    APPROVE_TOOL, Driver, DriverError, DriverEvent, HARD_STOP_FORCE_AFTER, HOST_TOKEN_ENV,
    LaunchSpec, MCP_SERVER, MCP_TOOL_TIMEOUT_MS, McpServer, OutboundMessage, ProtectionLaunch,
    RemoteMcp, SeatToolsLaunch, Session,
};
