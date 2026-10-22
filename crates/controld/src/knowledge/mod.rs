//! Knowledge: what a team's agents know about their project and each other, with nobody maintaining
//! it. A project map generated on the agents' computer at every start (`localops::map`), who has been
//! working where, lessons agents recorded, and a search of what the team already did. Agents get the
//! short version in their briefing and look up the rest; people see it in the Knowledge view.
mod tools;
pub use tools::{record_lesson, search_history};

use crate::error::{ApiError, respond};
use crate::extract::Path;
use crate::state::AppState;
use crate::store::DecisionEntry;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use localops::map::ProjectMap;
use model::TeamDraft;
use rusqlite::params;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

mod lessons;
pub(crate) use lessons::{briefing_lines, lesson_lines, new_lessons};
pub use lessons::{refresh, routes};
