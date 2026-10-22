//! Converting a Claude Code or Codex session file (plus its subagents) into a `sculptured.session/v1`
//! run: each agent becomes a *thread* of steps, and the writer turns the threads into one run whose
//! every line is `origin: converted`. Nothing here touches the store; `/api/runs/import` stores it.
mod claude;
mod claude_links;
mod codex;
mod team;
mod time;
mod write;

use super::session::{Source, TokenTotals, at_line};
use crate::error::ApiError;
use model::{EventBody, Provenance};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) use time::ts_ms;

mod conversion;
pub use conversion::{Converted, Ev, Preview, Read, Step, Thread, Upload, convert};
pub(crate) use conversion::{invalid, post_convert};
