//! Claude Code via `claude -p --input-format stream-json --output-format stream-json`.
mod config;
mod limits;
mod parser;

use crate::process::{self, StderrTail};
use crate::{Driver, DriverError, DriverEvent, LaunchSpec, OutboundMessage, Session, check_cwd};
use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};
use model::Caps;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_stream::wrappers::UnboundedReceiverStream;

pub use limits::{cli_limit_message, limit_kind};
pub use parser::ClaudeParser;

mod driver;
pub use driver::ClaudeStreamDriver;
