//! The action interpreter (spec §6.4): what a tool call did, in Sculptured's words. Pure and
//! observe-only: it reads tool names and inputs (never the filesystem) and never blocks anything.
//! Everything it says is heuristic (`≈`): it reads words, so it can miss a risk or name one that
//! did not happen. An empty list means nothing stood out, never that a call was safe or correct.
pub mod shell;

use model::{ActionClass, RiskFlag};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

mod action;
pub(crate) use action::flag;
pub use action::{MAX_FLAG_TEXT, classify, command_of, consequence, outside, risks};
