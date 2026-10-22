//! Team drafts: storage-backed API for the builder. A team's rig name is its draft id;
//! a started team is linked through the rig's `RigSource::Team { id, revision }`.
mod folder_checks;
mod run;
mod textbook;
mod textbook_files;

use crate::api::{fingerprint, idempotent};
use crate::error::{ApiError, respond, to_value};
use crate::extract::{Json, Path};
use crate::state::AppState;
use crate::store::{RigSource, SaveDraft, Store, is_live};
use crate::views::{TeamImport, TeamRecord, TeamSpec, TeamSummary};
use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
pub(crate) use folder_checks::folder_issues;
use model::{DraftIssue, IssueSeverity, TeamDraft};
use run::{start_team, stop_team};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

mod service;
pub use service::routes;
pub(crate) use service::{is_error, issues_for, load, new_team_id, record};
