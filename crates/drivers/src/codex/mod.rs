//! Codex via `codex app-server`: JSON-RPC 2.0 over stdio, as recorded in M0 (Codex 0.160.0) and
//! refreshed for M2 (Codex 0.160.1, same schema; `crates/fixtures/recordings/codex-m2-*`).
mod config;
mod core;
mod parser;
mod session;

use crate::{Driver, DriverError, LaunchSpec, Session};
use async_trait::async_trait;
use model::Caps;
use std::path::PathBuf;
use std::time::Duration;

pub use parser::{CodexParser, Parsed};

mod driver;
use driver::Open;
pub use driver::{
    CodexAppServerDriver, EXPIRED_ANSWER, MAX_START_ATTEMPTS, RESEND_PAUSE, TOOL_UNFINISHED,
};
