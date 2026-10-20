//! Codex via `codex app-server`: JSON-RPC 2.0 over stdio, as recorded in M0 (Codex 0.160.0) and
//! refreshed for M2 (Codex 0.160.1, same schema; `crates/fixtures/recordings/codex-m2-*`).
mod core;
mod parser;
mod session;

use crate::{Driver, DriverError, LaunchSpec, SeatToolsLaunch, Session};
use async_trait::async_trait;
use model::{Caps, PermissionPosture};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::time::Duration;

pub use parser::{CodexParser, Parsed};

/// What a native Codex question gets when nobody answered in time (D8). The timeout's
/// length is controld's to know; the agent is told to decide either way.
pub const EXPIRED_ANSWER: &str = "No operator answer in time. Proceed with your best judgment and state the decision you made.";

/// The result of a tool call that was still running when its turn ended.
pub const TOOL_UNFINISHED: &str = "(no result: the turn ended before this finished)";

/// How many times one message's `turn/start` is sent before the driver gives up on it. A refused
/// start (a JSON-RPC error, or an answer naming a turn that already ended) hands the message back;
/// it goes to the front of the queue and is sent again when no turn runs. A refusal that repeats
/// is not going away, so after this many tries the message fails with the server's words instead
/// of looping (Task 13 carry-forward).
pub const MAX_START_ATTEMPTS: u32 = 3;

/// The pause before a refused message is sent again, times the number of tries so far (0.5 s, then
/// 1 s). A `turn/completed` ends the pause early: a turn the driver did not know of is over.
pub const RESEND_PAUSE: Duration = Duration::from_millis(500);

/// `codex app-server` (JSON-RPC 2.0 over stdio), one process per session.
///
/// - **Posture (D7).** Unrestricted: `approvalPolicy: "never"`, `sandbox: "danger-full-access"`.
///   Restricted: `approvalPolicy: "untrusted"`, same sandbox; Codex then asks through its native
///   approvals. Both are passed on `thread/start`, `thread/resume` and `thread/fork`.
/// - **Seat tools** go in the thread `config` as dotted keys (`mcp_servers.rig.*`), confirmed by the
///   0.160.1 refresh (codex-m2-seat-config-dotted). The config travels over stdin, so the seat
///   token is never on argv and no config file is needed: `seat_tools_config_file()` is false and
///   any `SeatToolsLaunch.config_file` is ignored.
/// - **Queueing.** One message per turn: a message sent while a turn runs or is being started waits
///   here and starts the next turn on `turn/completed`. `turn/start` is never sent mid-turn (the
///   real server would steer it into the running turn) and `turn/steer` is not used. A refused
///   start is sent again after a pause (`RESEND_PAUSE`), `MAX_START_ATTEMPTS` times in all.
/// - **Resume** never passes `excludeTurns`: the usage baseline relies on the full-history resume's
///   replayed `tokenUsage` (codex-m2-resume@232), and nothing has been recorded without it.
/// - **Interrupt** ends the turn but not the command Codex is running (codex-m2-interrupt: the
///   `sleep` outlived `turn/completed: interrupted`). The command lives until the app-server exits.
///   An interrupt while a `turn/start` waits for its answer is sent as soon as the turn id arrives.
/// - **Stop** closes stdin (the server exits about 250 ms later, every recording) and kills the
///   process group after the grace period. Whenever the app-server's output ends, the rest of its
///   process group (commands an interrupt left running, MCP servers) is killed before the server is
///   reaped, so nothing it started in its group outlives the session. A command Codex moved to a
///   process group of its own is out of reach (unobserved).
pub struct CodexAppServerDriver {
    pub program: PathBuf,
    /// Start with `--enable default_mode_request_user_input` (an under-development flag, M0).
    pub native_questions: bool,
}

impl Default for CodexAppServerDriver {
    fn default() -> Self {
        Self { program: PathBuf::from("codex"), native_questions: false }
    }
}

enum Open<'a> {
    Start,
    Resume(&'a str),
    Fork(&'a str),
}

impl CodexAppServerDriver {
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["app-server".to_string()];
        if self.native_questions {
            a.extend(["--enable".to_string(), "default_mode_request_user_input".to_string()]);
        }
        a
    }

    /// `thread/start|resume|fork` parameters. D7 (spec §6.2): unrestricted means approvals off and
    /// no sandbox; restricted means `untrusted`, the policy M0 saw ask before a file write.
    pub fn thread_params(spec: &LaunchSpec) -> Value {
        let approval = match spec.posture {
            PermissionPosture::Unrestricted => "never",
            PermissionPosture::Restricted => "untrusted",
        };
        // A sandboxed computer (its owner's choice) lets Codex write only inside the folder.
        let sandbox = if spec.sandboxed { "workspace-write" } else { "danger-full-access" };
        let mut p = json!({ "cwd": spec.cwd.display().to_string(), "approvalPolicy": approval, "sandbox": sandbox });
        if let Some(m) = &spec.model {
            p["model"] = json!(m);
        }
        if let Some(t) = &spec.seat_tools {
            p["config"] = Self::mcp_config(t);
            // All delegation uses the managed MCP tool and therefore appears on the live board.
            p["config"]["features.multi_agent"] = json!(false);
            p["config"]["features.multi_agent_v2"] = json!(false);
        }
        if let Some(limit) = spec.compact_tokens {
            if !p["config"].is_object() {
                p["config"] = json!({});
            }
            p["config"]["model_auto_compact_token_limit"] = json!(limit);
        }
        for s in &spec.mcp_servers {
            if !p["config"].is_object() {
                p["config"] = json!({});
            }
            p["config"][format!("mcp_servers.{}.command", s.name)] = json!(s.program.display().to_string());
            p["config"][format!("mcp_servers.{}.args", s.name)] = json!(s.args);
        }
        for (i, s) in spec.remote_mcp.iter().enumerate() {
            if !p["config"].is_object() {
                p["config"] = json!({});
            }
            p["config"][format!("mcp_servers.{}.url", s.name)] = json!(s.url);
            if !s.headers.is_empty() {
                let from_env: Map<String, Value> = s.headers.iter().enumerate().map(|(j, (k, _))| (k.clone(), json!(remote_header_env(i, j)))).collect();
                p["config"][format!("mcp_servers.{}.env_http_headers", s.name)] = Value::Object(from_env);
            }
        }
        p
    }

    /// The header values of the remote servers, as the environment variables `env_http_headers` names,
    /// so they are in neither the command line nor the thread's config.
    pub fn remote_env(spec: &LaunchSpec) -> Vec<(String, String)> {
        spec.remote_mcp.iter().enumerate().flat_map(|(i, s)| s.headers.iter().enumerate().map(move |(j, (_, v))| (remote_header_env(i, j), v.clone()))).collect()
    }

    /// Config overrides that start the `rig` MCP server (`sculptured seat-tools`) for this thread,
    /// as dotted keys (codex-m2-seat-config-dotted: started, listed and called).
    pub fn mcp_config(t: &SeatToolsLaunch) -> Value {
        let env: Map<String, Value> = t.env.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
        let key = |k: &str| format!("mcp_servers.{}.{k}", crate::MCP_SERVER);
        let timeout_sec = crate::MCP_TOOL_TIMEOUT_MS.parse::<u64>().unwrap_or(86_700_000) / 1000;
        let mut m = Map::new();
        m.insert(key("command"), json!(t.program.display().to_string()));
        m.insert(key("args"), json!(t.args));
        m.insert(key("env"), Value::Object(env));
        m.insert(key("tool_timeout_sec"), json!(timeout_sec));
        Value::Object(m)
    }
}

#[async_trait]
impl Driver for CodexAppServerDriver {
    fn name(&self) -> &'static str {
        "codex-app-server"
    }
    fn capabilities(&self) -> Caps {
        // M0 Caps table: interrupt, resume, fork and usage observed; questions only with the flag;
        // steer exists but M2 keeps one message per turn; no subagent events were observed.
        Caps {
            interrupt: true,
            questions: self.native_questions,
            resume: true,
            fork: true,
            usage: true,
            mid_turn_input: false,
            subagent_events: false,
        }
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        self.spawn(spec, Open::Start).await
    }
    async fn resume(&self, spec: LaunchSpec, native_id: &str) -> Result<Box<dyn Session>, DriverError> {
        self.spawn(spec, Open::Resume(native_id)).await
    }
    async fn fork(&self, spec: LaunchSpec, native_id: &str) -> Result<Box<dyn Session>, DriverError> {
        self.spawn(spec, Open::Fork(native_id)).await
    }
    /// The seat-tools config goes in the thread `config` over stdin (see the type's doc).
    fn seat_tools_config_file(&self) -> bool {
        false
    }
}


/// The environment variable holding header `j` of remote server `i`.
fn remote_header_env(i: usize, j: usize) -> String {
    format!("SCULPTURED_MCP_HEADER_{i}_{j}")
}

#[cfg(test)]
mod sandbox_tests {
    use super::*;
    #[test]
    fn a_sandboxed_launch_lets_codex_write_only_in_its_folder() {
        let mut spec = LaunchSpec::new("dev.a@r".parse().unwrap(), "/w".into());
        assert_eq!(CodexAppServerDriver::thread_params(&spec)["sandbox"], "danger-full-access");
        spec.sandboxed = true;
        assert_eq!(CodexAppServerDriver::thread_params(&spec)["sandbox"], "workspace-write");
    }

    #[test]
    fn codex_compacts_its_context_at_the_teams_limit() {
        let mut spec = LaunchSpec::new("dev.a@r".parse().unwrap(), "/w".into());
        assert!(CodexAppServerDriver::thread_params(&spec)["config"].get("model_auto_compact_token_limit").is_none());
        spec.compact_tokens = Some(120_000);
        assert_eq!(CodexAppServerDriver::thread_params(&spec)["config"]["model_auto_compact_token_limit"], 120_000);
    }

    #[test]
    fn an_integration_is_started_as_a_codex_mcp_server() {
        let mut spec = LaunchSpec::new("dev.a@r".parse().unwrap(), "/w".into());
        spec.mcp_servers = vec![crate::McpServer { name: "kindex".into(), program: "/usr/bin/kin-mcp".into(), args: vec![] }];
        spec.remote_mcp = vec![crate::RemoteMcp { name: "openfoam".into(), url: "https://mcp.example.com/mcp".into(), headers: vec![("Authorization".into(), "Bearer s3cret".into())] }];
        let p = CodexAppServerDriver::thread_params(&spec);
        assert_eq!(p["config"]["mcp_servers.kindex.command"], "/usr/bin/kin-mcp");
        assert_eq!(p["config"]["mcp_servers.openfoam.url"], "https://mcp.example.com/mcp");
        let var = p["config"]["mcp_servers.openfoam.env_http_headers"]["Authorization"].as_str().unwrap().to_string();
        assert!(!p.to_string().contains("s3cret"), "the secret is not in the thread's config");
        assert_eq!(CodexAppServerDriver::remote_env(&spec), [(var, "Bearer s3cret".to_string())], "it is in the environment the header names");
    }
}
