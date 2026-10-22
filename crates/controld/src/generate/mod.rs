//! Generating a team with the user's own Claude Code or Codex: optional clarifying questions, then
//! 1 or 3 team sketches, each turned into a laid-out team draft with its issues, which the person
//! trims and creates (`/api/generate/check`, `/api/generate/create`).
//! The CLI runs on the user's computer (`LocalOp::Generate`), with no tools.
mod accept;
mod assistant;
mod assistant_layout;
mod improve;
pub mod sketch;

use crate::error::{ApiError, respond};
use crate::extract::Json;
use crate::ops::{self, Target};
use crate::state::AppState;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::post;
pub(super) use generation::design_rules;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sketch::{Kinds, Sketch};

mod generation;
pub(super) use generation::{ABOUT, Offered};
pub use generation::{Answer, Options, routes};
use generation::{alternatives_parser, call, offered};
