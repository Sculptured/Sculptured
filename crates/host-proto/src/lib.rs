//! Messages between controld and a host runtime, as JSON text frames over a WebSocket.
//! hostd dials controld, sends Hello, and receives Welcome{resume_after}. It then
//! replays journal events with host_seq > resume_after and streams live events.
//! controld acks the highest ingested host_seq; hostd prunes its journal up to it.
use model::{Actor, Answer, Caps, EventBody, MessageIntent, PermissionPosture, Provenance, Subject, TenureCause};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostEvent {
    pub host_seq: u64,
    pub at_ms: i64,
    pub actor: Actor,
    pub subject: Subject,
    pub provenance: Provenance,
    pub body: EventBody,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DriverInfo {
    pub name: String,
    pub caps: Caps,
}

/// Seat-tools options controld chooses for a launch; hostd mints the token.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SeatToolsParams {
    /// Expose the `approve` permission-prompt tool (restricted posture).
    pub approve: bool,
    /// Offer the dynamic-workflow tools (`dispatch_work`, `complete_work`, `workflow_status`): the team
    /// has a workflow. Missing (an older server) means offered, as before.
    #[serde(default = "offered")]
    pub workflow: bool,
}

fn offered() -> bool {
    true
}

/// Environment for an agent's process. It can hold a key, so Debug shows the names only; on the wire
/// it is the plain list of pairs.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Env(pub Vec<(String, String)>);

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.0.iter().map(|(k, _)| format!("{k}=…"))).finish()
    }
}
impl std::ops::Deref for Env {
    type Target = Vec<(String, String)>;
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl std::ops::DerefMut for Env {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.0 }
}
impl From<Vec<(String, String)>> for Env {
    fn from(v: Vec<(String, String)>) -> Self { Env(v) }
}
impl FromIterator<(String, String)> for Env {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(i: I) -> Self { Env(i.into_iter().collect()) }
}
impl<'a> IntoIterator for &'a Env {
    type Item = &'a (String, String);
    type IntoIter = std::slice::Iter<'a, (String, String)>;
    fn into_iter(self) -> Self::IntoIter { self.0.iter() }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LaunchParams {
    pub cwd: String,
    pub model: Option<String>,
    pub program: Option<String>,
    /// A model connection's settings for the agent's process (`MODEL_ENV_KEYS`). They hold a key: never
    /// log, store or record them.
    pub env: Env,
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub posture: PermissionPosture,
    #[serde(default)]
    pub seat_tools: Option<SeatToolsParams>,
    #[serde(default)]
    pub usage_baseline: Option<[u64; 4]>,
    /// Hosted teams with own copies: the computer makes the agent's worktree and runs it there.
    #[serde(default)]
    pub own_copy: Option<OwnCopy>,
    /// copies: per-item on a connected computer: the agent may also work in this team's items folder.
    #[serde(default)]
    pub item_copies: Option<String>,
    /// Local mode only: more folders the agent may work in. A connected computer ignores it.
    #[serde(default)]
    pub add_dirs: Vec<String>,
    /// Claude agents: which of the person's own Claude Code setup to load. None = all of it (as before).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_setup: Option<model::ClaudeSetup>,
    /// Claude agents: the protections to apply to the agent's tool calls. None = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<model::GuardRules>,
    /// Codex agents: compact the conversation past this many tokens of context. None = Codex's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_tokens: Option<u64>,
    /// The agent's skills. Claude Code loads them natively; other CLIs read one with `read_skill`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<model::Skill>,
    /// Tools on the agent's computer it is given (by name, such as `kindex`); the runner finds them.
    /// Sent for runners from before `components`; built-in ones only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub integrations: Vec<String>,
    /// The components it is given, as manifests (feature `components`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<model::Component>,
    /// Claude agents with protections: the agent's briefing, given back to it after Claude compacts its
    /// context. An older runner ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub briefing: Option<String>,
}

/// How a new tenure gets its native session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LaunchMode {
    #[default]
    Fresh,
    Resume { native_session_id: String },
    Fork { native_session_id: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum HostCommand {
    Launch { seat: String, driver: String, params: LaunchParams },
    /// A new tenure for a seat with no live process: resume, fork, or fresh with a stated cause.
    Restart { seat: String, driver: String, params: LaunchParams, mode: LaunchMode, cause: TenureCause },
    Send { seat: String, message_id: String, intent: MessageIntent, text: String },
    Interrupt { seat: String },
    /// `hard`: stop now, like Ctrl+C (interrupt the agent and what it started, force after 2 s);
    /// otherwise wait up to `grace_ms` for it to finish. Runners without `hard` ignore it.
    Stop {
        seat: String,
        grace_ms: u64,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        hard: bool,
    },
    /// Answers a question the seat's session raised (`DriverEvent::QuestionRaised`).
    Answer { seat: String, question_id: String, answer: Answer },
    /// Runs the seat's check command in `cwd` on this computer; the result arrives as `CheckStarted` /
    /// `CheckRan` events for the seat. Acknowledged once queued (a check can take ten minutes).
    /// With `own_copy`, the check runs in the agent's own copy of `cwd`, where it works.
    RunCheck {
        seat: String,
        command: String,
        cwd: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        own_copy: Option<OwnCopy>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum FromHost {
    /// `live_seats`: seats with a process, including ones still starting (`launching_seats`, a subset).
    /// `features`: what this hostd can do beyond M1 (`features::*`); an M1 hostd sends neither field.
    Hello {
        host_id: String,
        protocol: u32,
        drivers: Vec<DriverInfo>,
        live_seats: Vec<String>,
        #[serde(default)]
        launching_seats: Vec<String>,
        #[serde(default)]
        features: Vec<String>,
        /// A connected computer's own rules (None for a local `serve`'s runner).
        #[serde(default)]
        policy: Option<HostPolicyInfo>,
    },
    Event { event: HostEvent },
    CommandResult { command_id: String, ok: bool, error: Option<String> },
    /// An agent called one of Sculptured's tools; controld answers with `ToHost::SeatToolReply`.
    SeatTool { call_id: String, seat: String, tool: String, args: serde_json::Value },
    /// The answer to `ToHost::Op`: the value, or why it was refused or failed.
    OpResult {
        op_id: String,
        ok: bool,
        #[serde(default)]
        value: serde_json::Value,
        #[serde(default)]
        error: Option<OpError>,
    },
}

/// What a connected computer allows, as its owner set it (`runner.json`), for the Computers page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct HostPolicyInfo {
    pub allowed_folders: Vec<String>,
    /// `full` or `sandboxed`.
    pub access: String,
    /// Enabled: `git`, `copies`, `checks`, `cli_sign_in`, `plan_usage`, `generate`, `usage_reports`.
    pub capabilities: Vec<String>,
    /// `linux`, `macos`, `windows`.
    pub platform: String,
}

/// Why an op was refused or failed, with the HTTP status and code the API answers with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum ToHost {
    Welcome { resume_after: u64 },
    Command { command_id: String, command: HostCommand },
    Ack { upto: u64 },
    SeatToolReply { call_id: String, ok: bool, result: serde_json::Value, error: Option<String> },
    /// Work on the computer (a serialized `localops::LocalOp`); answered with `FromHost::OpResult`.
    Op { op_id: String, request: serde_json::Value },
}

/// What a hostd can do beyond M1, advertised in `FromHost::Hello`. A hostd that started before an upgrade
/// keeps running (it owns the agents), so controld checks these before sending a command that needs one.
pub mod features {
    /// Honours `LaunchParams.posture` (a restricted launch is not run unrestricted).
    pub const POSTURE: &str = "posture";
    /// `HostCommand::Restart` (resume, fork, or fresh with a cause).
    pub const RESTART: &str = "restart";
    /// `HostCommand::Answer`.
    pub const ANSWER: &str = "answer";
    /// Offers Sculptured's tools to agents (`LaunchParams.seat_tools`).
    pub const SEAT_TOOLS: &str = "seat-tools";
    /// Seeds a resumed session's usage (`LaunchParams.usage_baseline`).
    pub const USAGE_BASELINE: &str = "usage-baseline";
    /// Answers `ToHost::Op` (local operations: folders, git, copies, CLIs, plan usage).
    pub const LOCAL_OPS: &str = "local-ops";
    /// Honours `LaunchParams.claude_setup` (leaves out the person's own Claude Code setup).
    pub const CLAUDE_SETUP: &str = "claude-setup";
    /// Answers `LocalOp::AnalyzeRun` (where a run's tokens went).
    pub const USAGE_REPORTS: &str = "usage-reports";
    /// Honours `LaunchParams.protection` (guards the agent's tool calls).
    pub const PROTECTION: &str = "protection";
    /// Honours `LaunchParams.skills`.
    pub const SKILLS: &str = "skills";
    /// Honours `LaunchParams.integrations`.
    pub const INTEGRATIONS: &str = "integrations";
    /// Honours `LaunchParams.components`.
    pub const COMPONENTS: &str = "components";
    /// Starts remote MCP servers and Claude plugins among `LaunchParams.components`; an older runner
    /// cannot read them at all.
    pub const COMPONENT_KINDS: &str = "component-kinds";
    /// Passes a model connection's settings (`LaunchParams.env`, `MODEL_ENV_KEYS` only) to the agent's
    /// CLI. An older runner refuses any environment, so a launch with one is refused before it is sent.
    pub const MODEL_ENV: &str = "model-env";
}

/// The only environment the server may give an agent: a model connection's key, and where its
/// requests go. Anything else in `LaunchParams.env` is refused by a connected computer.
pub const MODEL_ENV_KEYS: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL"];

/// The addresses `ANTHROPIC_BASE_URL` may name (OpenRouter's Anthropic-compatible API).
pub const MODEL_BASE_URLS: &[&str] = &["https://openrouter.ai/api"];

/// True when every entry is a model connection setting (`MODEL_ENV_KEYS`), each named once, with a
/// known `ANTHROPIC_BASE_URL`.
pub fn model_env_allowed(env: &[(String, String)]) -> bool {
    env.iter().enumerate().all(|(i, (k, v))| {
        MODEL_ENV_KEYS.contains(&k.as_str())
            && !env[..i].iter().any(|(o, _)| o == k)
            && (k != "ANTHROPIC_BASE_URL" || MODEL_BASE_URLS.contains(&v.as_str()))
    })
}

/// Run the agent in its own git copy (worktree) of the folder, made on the computer that runs it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnCopy {
    pub team: String,
    /// The spawning agent, whose branch a subagent's copy starts from.
    pub parent: Option<String>,
}

impl LaunchParams {
    /// The host features these parameters need; an M1 launch needs none.
    pub fn required_features(&self) -> Vec<&'static str> {
        let mut f = Vec::new();
        if self.posture != PermissionPosture::Unrestricted {
            f.push(features::POSTURE);
        }
        if self.seat_tools.is_some() {
            f.push(features::SEAT_TOOLS);
        }
        if self.usage_baseline.is_some() {
            f.push(features::USAGE_BASELINE);
        }
        if self.claude_setup.is_some() {
            f.push(features::CLAUDE_SETUP);
        }
        if self.protection.is_some() {
            f.push(features::PROTECTION);
        }
        if !self.skills.is_empty() {
            f.push(features::SKILLS);
        }
        if !self.integrations.is_empty() {
            f.push(features::INTEGRATIONS);
        }
        if !self.components.is_empty() {
            f.push(features::COMPONENTS);
        }
        if self.components.iter().any(|c| c.kind.is_newer()) {
            f.push(features::COMPONENT_KINDS);
        }
        if self.own_copy.is_some() {
            f.push(features::LOCAL_OPS);
        }
        if !self.env.is_empty() {
            f.push(features::MODEL_ENV);
        }
        f
    }
}

impl HostCommand {
    /// The host features this command needs (`features::*`); controld refuses it for a host without them.
    pub fn required_features(&self) -> Vec<&'static str> {
        match self {
            HostCommand::Launch { params, .. } => params.required_features(),
            HostCommand::Restart { params, .. } => {
                let mut f = vec![features::RESTART];
                f.extend(params.required_features());
                f
            }
            HostCommand::Answer { .. } => vec![features::ANSWER],
            HostCommand::RunCheck { .. } => vec![features::LOCAL_OPS],
            HostCommand::Send { .. } | HostCommand::Interrupt { .. } | HostCommand::Stop { .. } => vec![],
        }
    }
}

pub fn encode<T: Serialize>(m: &T) -> String {
    serde_json::to_string(m).expect("protocol types always serialize")
}

pub fn decode<T: DeserializeOwned>(s: &str) -> Result<T, serde_json::Error> {
    serde_json::from_str(s)
}

/// The seat-tools socket: one JSON line from the agent's shim (or a Practice agent) to hostd, one back.
pub mod seat_tools {
    use serde::{Deserialize, Serialize};
    use serde_json::Value;
    use std::io::{BufRead, BufReader, Write};
    #[cfg(unix)]
    use std::os::unix::net::UnixStream;
    use std::path::Path;

    pub const SOCKET_ENV: &str = "SCULPTURED_SEAT_SOCKET";
    pub const TOKEN_ENV: &str = "SCULPTURED_SEAT_TOKEN";
    pub const APPROVE_ENV: &str = "SCULPTURED_SEAT_APPROVE";
    /// "0": the team has no dynamic workflow, so its tools are not offered.
    pub const WORKFLOW_ENV: &str = "SCULPTURED_SEAT_WORKFLOW";
    /// "1": the agent has skills it reads with `read_skill` (its CLI cannot load them natively).
    pub const SKILLS_ENV: &str = "SCULPTURED_SEAT_SKILLS";

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct ToolRequest {
        pub token: String,
        pub tool: String,
        pub args: Value,
    }

    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    pub struct ToolReply {
        pub ok: bool,
        pub result: Value,
        pub error: Option<String>,
    }

    /// The environment the shim needs to reach hostd as this agent.
    pub fn shim_env(socket: &Path, token: &str, approve: bool) -> Vec<(String, String)> {
        let mut env = vec![(SOCKET_ENV.to_string(), socket.display().to_string()), (TOKEN_ENV.to_string(), token.to_string())];
        if approve {
            env.push((APPROVE_ENV.to_string(), "1".to_string()));
        }
        env
    }

    /// Unix: the socket itself.
    #[cfg(unix)]
    fn connect(socket: &Path) -> std::io::Result<UnixStream> {
        UnixStream::connect(socket)
    }

    /// Windows: `socket` is a file holding the loopback address hostd listens on (each call still
    /// carries the agent's own token).
    #[cfg(windows)]
    fn connect(socket: &Path) -> std::io::Result<std::net::TcpStream> {
        let addr = std::fs::read_to_string(socket)?;
        std::net::TcpStream::connect(addr.trim())
    }

    /// Calls one tool and waits for its reply. Blocking: call it from a blocking thread.
    pub fn call(socket: &Path, token: &str, tool: &str, args: Value) -> Result<Value, String> {
        let unreachable = |e: std::io::Error| format!("cannot reach Sculptured's agent runner: {e}");
        let mut s = connect(socket).map_err(unreachable)?;
        let req = serde_json::to_string(&ToolRequest { token: token.into(), tool: tool.into(), args }).expect("request serializes");
        s.write_all(format!("{req}\n").as_bytes()).map_err(unreachable)?;
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).map_err(|e| format!("no reply from Sculptured's agent runner: {e}"))?;
        let reply: ToolReply =
            serde_json::from_str(line.trim()).map_err(|e| format!("unreadable reply from Sculptured's agent runner: {e}"))?;
        if reply.ok { Ok(reply.result) } else { Err(reply.error.unwrap_or_else(|| "the tool failed".into())) }
    }
}
