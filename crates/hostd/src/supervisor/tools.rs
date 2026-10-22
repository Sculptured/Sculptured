//! Seat tools on the supervisor side: the tokens that live while an agent's process runs, the
//! config files that name them, and the relay of tool calls to controld.
use super::Supervisor;
use host_proto::seat_tools::shim_env;
use host_proto::{FromHost, SeatToolsParams};
use drivers::SeatToolsLaunch;
use model::EventBody;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::sync::{broadcast, oneshot};

/// What a live seat-tools token grants: its seat, whether it may call `approve` (a restricted Claude
/// agent), and the config file naming it (deleted with the token).
pub(super) struct Grant {
    seat: String,
    approve: bool,
    config_file: Option<PathBuf>,
}

/// Where the seat-tools shim reaches this hostd (Task 6 serves the socket).
#[derive(Clone, Debug)]
pub struct SeatToolsHost {
    /// The socket the shim dials. Its folder is hostd's private (0700) directory; the per-agent config
    /// files that name a token are written to `seat-tools/` next to it.
    pub socket: PathBuf,
    /// The `sculptured` binary; its `seat-tools` subcommand is the MCP shim.
    pub shim: PathBuf,
}

/// What a waiting tool call returns when its agent's process ended first (nobody reads it then).
pub const CALL_ENDED: &str = "This agent's process ended before Sculptured answered.";

impl Supervisor {
    /// The seat a seat-tools token belongs to, while that seat's process runs.
    pub fn seat_for_token(&self, token: &str) -> Option<String> {
        self.inner.tokens.lock().unwrap().get(token).map(|g| g.seat.clone())
    }

    /// The seat a live token belongs to and whether it was launched with the `approve` tool.
    pub fn tool_grant(&self, token: &str) -> Option<(String, bool)> {
        self.inner.tokens.lock().unwrap().get(token).map(|g| (g.seat.clone(), g.approve))
    }

    /// Records a seat-tool call hostd refused before it reached controld (`report-rejected` evidence).
    pub fn reject_tool_call(&self, seat: &str, tool: &str, reason: &str, raw: Value) {
        let body = EventBody::ReportRejected { tool: tool.chars().take(200).collect(), reason: reason.to_string(), raw };
        self.emit_system(seat, body);
    }

    /// Forgets a token and deletes the config file that names it.
    pub(super) fn forget_token(&self, token: &Option<String>) {
        let Some(t) = token else { return };
        let removed = self.inner.tokens.lock().unwrap().remove(t);
        // The process is gone: its waiting calls end, and no later link sends them again.
        if let Some(g) = &removed {
            self.inner.tool_pending.lock().unwrap().retain(|_, (call, _)| !matches!(call, FromHost::SeatTool { seat, .. } if *seat == g.seat));
        }
        if let Some(Grant { seat, config_file: Some(file), .. }) = removed
            && let Err(e) = std::fs::remove_file(&file)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%seat, file = %file.display(), error = %e, "cannot delete the seat-tools config file");
        }
    }

    /// Relays one seat-tool call to controld and waits for its reply (which may take as long as a question).
    /// The call outlives a dropped link: while the computer is offline it waits, and every link sends the
    /// calls still waiting again (same id; controld answers each id once).
    pub async fn tool_call(&self, seat: &str, tool: &str, args: Value) -> Result<Value, String> {
        let call_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let frame = FromHost::SeatTool { call_id: call_id.clone(), seat: seat.to_string(), tool: tool.to_string(), args };
        self.inner.tool_pending.lock().unwrap().insert(call_id, (frame.clone(), tx));
        // No link is subscribed while offline; the next one sends it from `pending_tool_calls`.
        let _ = self.inner.tool_out.send(frame);
        rx.await.unwrap_or_else(|_| Err(CALL_ENDED.to_string()))
    }

    pub fn subscribe_tool_calls(&self) -> broadcast::Receiver<FromHost> {
        self.inner.tool_out.subscribe()
    }

    /// Every call still waiting for controld's reply (in no particular order).
    pub fn pending_tool_calls(&self) -> Vec<FromHost> {
        self.inner.tool_pending.lock().unwrap().values().map(|(call, _)| call.clone()).collect()
    }

    /// Hands controld's reply to the waiting call; a call already answered (or unknown) ignores it.
    pub fn resolve_tool_call(&self, call_id: &str, reply: Result<Value, String>) {
        let waiting = self.inner.tool_pending.lock().unwrap().remove(call_id);
        if let Some((_, tx)) = waiting {
            let _ = tx.send(reply);
        }
    }

    /// True while the call still waits for its reply.
    pub fn tool_call_pending(&self, call_id: &str) -> bool {
        self.inner.tool_pending.lock().unwrap().contains_key(call_id)
    }

    /// A new token for an agent launched with seat tools, when this hostd serves them.
    pub(super) fn new_token(&self, params: Option<&SeatToolsParams>) -> Option<String> {
        (params.is_some() && self.inner.seat_tools.is_some()).then(|| uuid::Uuid::new_v4().simple().to_string())
    }

    /// The seat-tools launch for token `t`, recorded as a live grant. A driver that passes the config
    /// as a file gets a unique one, so a late cleanup of an earlier process never deletes this one's
    /// (Codex passes it over stdin).
    /// The program whose `guard` subcommand checks an agent's tool calls: the seat-tools shim (the
    /// `sculptured` binary), else this process when it is that binary. None: protections can't run here.
    pub(super) fn guard_program(&self) -> Option<PathBuf> {
        if let Some(host) = &self.inner.seat_tools {
            return Some(host.shim.clone());
        }
        std::env::current_exe().ok().filter(|p| p.file_stem().is_some_and(|n| n == "sculptured"))
    }

    pub(super) fn grant_tools(&self, seat: &str, as_file: bool, p: &SeatToolsParams, t: &str) -> Result<Option<SeatToolsLaunch>, String> {
        let Some(host) = &self.inner.seat_tools else { return Ok(None) };
        let config_file = if as_file {
            let f = private_dir(host).map(|d| d.join(format!("{seat}.{}.json", &uuid::Uuid::new_v4().simple().to_string()[..8])));
            Some(f.map_err(|e| format!("cannot prepare the agent's tools: {e}"))?)
        } else {
            None
        };
        let launch = SeatToolsLaunch {
            program: host.shim.clone(),
            args: vec!["seat-tools".into()],
            env: {
                let mut env = shim_env(&host.socket, t, p.approve);
                if !p.workflow {
                    env.push((host_proto::seat_tools::WORKFLOW_ENV.to_string(), "0".to_string()));
                }
                env.into()
            },
            approve: p.approve,
            config_file: config_file.clone(),
        };
        self.inner.tokens.lock().unwrap().insert(t.to_string(), Grant { seat: seat.to_string(), approve: p.approve, config_file });
        Ok(Some(launch))
    }
}

fn tools_dir(host: &SeatToolsHost) -> PathBuf {
    host.socket.parent().unwrap_or_else(|| Path::new(".")).join("seat-tools")
}

/// `seat-tools/` next to the socket, created owner-only (0700): the agents' config files name tokens.
fn private_dir(host: &SeatToolsHost) -> std::io::Result<PathBuf> {
    let dir = tools_dir(host);
    std::fs::create_dir_all(&dir)?;
    crate::private::narrow_dir(&dir)?;
    Ok(dir)
}

/// Deletes config files an earlier hostd left behind (it died before it could): they name tokens
/// that no longer exist. Called when a supervisor with seat tools starts, before any agent launches.
pub(super) fn sweep_config_files(host: &SeatToolsHost) {
    let dir = tools_dir(host);
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(file = %path.display(), error = %e, "cannot delete a leftover seat-tools config file");
        }
    }
}
