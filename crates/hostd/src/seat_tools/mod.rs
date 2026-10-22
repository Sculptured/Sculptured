//! Sculptured's tools for agents. `sculptured seat-tools` is a stdio MCP server (the "shim") that a
//! CLI starts for its agent; it forwards each call over hostd's private socket (`socket`), which relays
//! it to controld over the host link. One JSON line each way on the socket (`host_proto::seat_tools`).
mod collaboration;
mod pdf;
mod request;
mod socket;
mod workflow;

pub use socket::{
    NO_APPROVE, NOT_TEXT, SocketLimits, TOO_LARGE, TOO_MANY, TOO_MANY_FOR_AGENT, TOO_SLOW,
    serve_socket, serve_socket_with,
};

use host_proto::seat_tools::{APPROVE_ENV, SOCKET_ENV, TOKEN_ENV, call};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod handler;
pub use handler::{
    MAX_SOCKET_PATH, Offer, SOCKET_FILE, handle_rpc, handle_rpc_with, run_shim, socket_path,
    socket_path_in, tool_list, tool_list_for,
};
