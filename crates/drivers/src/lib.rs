pub mod claude;
pub mod codex;
mod process;
pub mod stub;

/// hostd's bearer token for controld. Agent processes must never inherit it.
pub const HOST_TOKEN_ENV: &str = "SCULPTURED_HOST_TOKEN";

use async_trait::async_trait;
use futures::stream::BoxStream;
use model::{
    Answer, Caps, ContextBasis, LimitKind, MessageIntent, PermissionPosture, QuestionPrompt, QuestionSource, SeatId,
    ThinkingForm, TurnStatus,
};
use std::path::PathBuf;
use std::time::Duration;

/// The injected MCP server a CLI should start for this agent (`sculptured seat-tools`).
#[derive(Clone, Debug, PartialEq)]
pub struct SeatToolsLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Also expose `approve`, the permission-prompt tool (restricted posture).
    pub approve: bool,
    /// Where a driver that passes the config as a file writes it (mode 0600, in hostd's private
    /// directory), so the token in `env` never appears on the command line. hostd deletes it when the
    /// token is forgotten. None: the config is passed inline.
    pub config_file: Option<PathBuf>,
}

/// The MCP server name every CLI sees for Sculptured's tools.
pub const MCP_SERVER: &str = "rig";
/// Claude's `--permission-prompt-tool` value for the `approve` tool on the `rig` server.
pub const APPROVE_TOOL: &str = "mcp__rig__approve";
/// A question may legitimately block a tool call for the whole question timeout. This is longer than the
/// longest allowed timeout (1440 min, Task 4) plus 5 min margin, so Sculptured's own expiry always fires
/// first and D8's "Expired → agent decided" path runs instead of the CLI's tool error.
pub const MCP_TOOL_TIMEOUT_MS: &str = "86700000";

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub seat: SeatId,
    pub cwd: PathBuf,
    pub model: Option<String>,
    /// Overrides the driver's default CLI path (tests point this at fixtures).
    pub program: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub extra_args: Vec<String>,
    /// Pre-assigned native session id (Claude `--session-id`). Generated if None.
    pub native_session_id: Option<String>,
    /// D7: unrestricted unless a setting says otherwise.
    pub posture: PermissionPosture,
    pub seat_tools: Option<SeatToolsLaunch>,
    /// Session-cumulative usage already counted for a resumed session (input, output, cache read, cache write).
    pub usage_baseline: Option<[u64; 4]>,
    /// The computer limits what the agent may change (a connected computer set to `sandboxed`):
    /// Codex writes only inside its folder.
    pub sandboxed: bool,
    /// More folders the agent may work in (a team's items folder, `copies: per-item`).
    pub add_dirs: Vec<PathBuf>,
    /// Claude: which of the person's own Claude Code setup to load; None = all of it.
    pub claude_setup: Option<model::ClaudeSetup>,
    /// Claude: guard the agent's tool calls with these rules, run by `guard` (the `sculptured` binary).
    pub protection: Option<ProtectionLaunch>,
    /// Claude: plugin folders loaded for this session only (the agent's skills).
    pub plugin_dirs: Vec<PathBuf>,
    /// More MCP servers on this computer the agent is given (integrations such as Kindex).
    pub mcp_servers: Vec<McpServer>,
    /// MCP servers at a web address, their secrets filled in. Codex reads each header from its own
    /// environment variable; Claude reads them from `remote_mcp_file`, never from its command line.
    pub remote_mcp: Vec<RemoteMcp>,
    /// Claude: where the remote servers' `--mcp-config` is written, readable by this user only.
    pub remote_mcp_file: Option<PathBuf>,
    /// Claude: more PreToolUse hook commands on every tool call (Signet-Eval), after Sculptured's guard.
    pub tool_hooks: Vec<String>,
}

/// A local MCP server started over stdio for one agent.
#[derive(Clone, Debug, PartialEq)]
pub struct McpServer {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// An MCP server at a web address (streamable HTTP), with its headers' values filled in.
#[derive(Clone, PartialEq)]
pub struct RemoteMcp {
    pub name: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for RemoteMcp {
    /// Header values are secrets: left out of logs.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteMcp").field("name", &self.name).field("url", &self.url).field("headers", &self.headers.iter().map(|(k, _)| k).collect::<Vec<_>>()).finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProtectionLaunch {
    pub rules: model::GuardRules,
    /// The `sculptured` binary; its `guard` subcommand is the hook.
    pub guard: std::path::PathBuf,
    /// The agent's briefing, given back by `sculptured rebrief` after Claude compacts its context.
    pub briefing_file: Option<std::path::PathBuf>,
}

impl LaunchSpec {
    pub fn new(seat: SeatId, cwd: PathBuf) -> Self {
        Self {
            seat,
            cwd,
            model: None,
            program: None,
            env: vec![],
            extra_args: vec![],
            native_session_id: None,
            posture: PermissionPosture::Unrestricted,
            seat_tools: None,
            usage_baseline: None,
            sandboxed: false,
            add_dirs: vec![],
            claude_setup: None,
            protection: None,
            plugin_dirs: vec![],
            mcp_servers: vec![],
            remote_mcp: vec![],
            remote_mcp_file: None,
            tool_hooks: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutboundMessage {
    pub message_id: String,
    pub intent: MessageIntent,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DriverEvent {
    SessionReady { native_session_id: String },
    TurnStarted { turn_id: String },
    MessageConsumed { message_id: String, turn_id: String },
    Text { turn_id: Option<String>, text: String },
    ToolCall { turn_id: Option<String>, call_id: String, name: String, input: serde_json::Value, parent_call_id: Option<String> },
    ToolResult { turn_id: Option<String>, call_id: String, output: String, is_error: bool },
    /// `in_turn`: counted from one model call while the turn runs; the turn's final report (false)
    /// carries the rest, so the turn's reports still add up to the CLI's total.
    Usage { turn_id: Option<String>, input_tokens: u64, output_tokens: u64, cache_read_tokens: u64, cache_write_tokens: u64, cost: Option<model::Cost>, in_turn: bool },
    TurnEnded { turn_id: String, status: TurnStatus },
    /// The CLI (or a Practice agent) asks the operator something and waits.
    QuestionRaised { question_id: String, source: QuestionSource, prompt: QuestionPrompt, turn_id: Option<String> },
    /// The CLI took an answer for the question and carries on.
    QuestionResolved { question_id: String },
    /// The turn ended on the provider's usage or rate limit; the CLI's words verbatim. Sent just before
    /// the turn's `TurnEnded { Failed }`. Nothing retries it.
    LimitReached { turn_id: Option<String>, kind: LimitKind, resets: Option<String>, text: String },
    /// The CLI never started a turn for this message and the driver gave up on it (Codex: its
    /// `turn/start` was refused too many times). `reason` carries the CLI's words verbatim.
    MessageFailed { message_id: String, reason: String },
    Unrecognized { raw: serde_json::Value },
    /// The model this session asked for and the one the CLI reports (`EventBody::ModelObserved`).
    /// Send it after `SessionReady`: the seat's model is cleared when a session starts.
    ModelObserved {
        requested: Option<String>,
        observed: Option<String>,
        provider: Option<String>,
        effort: Option<String>,
        rerouted_from: Option<String>,
        reason: Option<String>,
    },
    /// Thinking the CLI showed, verbatim (`EventBody::ThinkingRecorded`; hostd caps the text).
    Thinking {
        turn_id: Option<String>,
        item_id: Option<String>,
        parent_call_id: Option<String>,
        form: ThinkingForm,
        text: String,
        estimated_tokens: Option<u64>,
    },
    /// How full the context was after a model call (`EventBody::ContextReported`).
    Context { turn_id: Option<String>, used_tokens: u64, window_tokens: Option<u64>, basis: ContextBasis },
    /// The CLI's report of how much of the signed-in plan is used (`EventBody::PlanUsageReported`).
    PlanUsage(model::PlanUsage),
    /// Always the last event of a session.
    Exited { exit_code: Option<i32>, signal: Option<i32>, stderr_tail: String },
}

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("failed to start {program}: {source}")]
    Spawn { program: String, #[source] source: std::io::Error },
    #[error("working directory does not exist: {0}")]
    Cwd(String),
    #[error("session is closed")]
    Closed,
    #[error("{0} is not supported by this driver")]
    Unsupported(&'static str),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait Driver: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> Caps;
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError>;
    async fn resume(&self, spec: LaunchSpec, native_id: &str) -> Result<Box<dyn Session>, DriverError>;
    /// A new session that starts from a copy of `native_id`'s history. Only drivers with `caps.fork`.
    async fn fork(&self, _spec: LaunchSpec, _native_id: &str) -> Result<Box<dyn Session>, DriverError> {
        Err(DriverError::Unsupported("fork"))
    }
    /// Whether this driver passes the seat-tools config as a file (`SeatToolsLaunch.config_file`), so
    /// hostd should name one. A driver that hands the config to the CLI another way that keeps the
    /// token off argv (Codex: the thread `config`, over stdin) says false and gets None.
    fn seat_tools_config_file(&self) -> bool {
        true
    }
}

#[async_trait]
pub trait Session: Send {
    async fn send(&mut self, msg: OutboundMessage) -> Result<(), DriverError>;
    async fn interrupt(&mut self) -> Result<(), DriverError>;
    async fn stop(&mut self, grace: Duration) -> Result<(), DriverError>;
    /// Stops now, like Ctrl+C in a terminal: interrupts the program and everything it started,
    /// then forces whatever is left after `HARD_STOP_FORCE_AFTER`. Default: stop with no grace.
    async fn hard_stop(&mut self) -> Result<(), DriverError> {
        self.stop(Duration::ZERO).await
    }
    /// Answers a question this session raised with `DriverEvent::QuestionRaised`.
    async fn answer(&mut self, _question_id: &str, _answer: Answer) -> Result<(), DriverError> {
        Err(DriverError::Unsupported("answer"))
    }
    /// Returns the event stream exactly once; it ends after `DriverEvent::Exited`.
    fn take_events(&mut self) -> Option<BoxStream<'static, DriverEvent>>;
    /// The pid leading the process group the driver made for this session alone, if it has one.
    fn process_id(&self) -> Option<u32> {
        None
    }
}

/// How long a hard stop waits after its interrupt before it forces the stop.
pub const HARD_STOP_FORCE_AFTER: Duration = Duration::from_secs(2);

pub(crate) fn check_cwd(spec: &LaunchSpec) -> Result<(), DriverError> {
    if spec.cwd.is_dir() { Ok(()) } else { Err(DriverError::Cwd(spec.cwd.display().to_string())) }
}
