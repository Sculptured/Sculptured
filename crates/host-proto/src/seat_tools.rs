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
#[must_use]
pub fn shim_env(socket: &Path, token: &str, approve: bool) -> Vec<(String, String)> {
    let mut env = vec![
        (SOCKET_ENV.to_string(), socket.display().to_string()),
        (TOKEN_ENV.to_string(), token.to_string()),
    ];
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
    let req = serde_json::to_string(&ToolRequest {
        token: token.into(),
        tool: tool.into(),
        args,
    })
    .expect("request serializes");
    s.write_all(format!("{req}\n").as_bytes())
        .map_err(unreachable)?;
    let mut line = String::new();
    BufReader::new(s)
        .read_line(&mut line)
        .map_err(|e| format!("no reply from Sculptured's agent runner: {e}"))?;
    let reply: ToolReply = serde_json::from_str(line.trim())
        .map_err(|e| format!("unreadable reply from Sculptured's agent runner: {e}"))?;
    if reply.ok {
        Ok(reply.result)
    } else {
        Err(reply.error.unwrap_or_else(|| "the tool failed".into()))
    }
}
