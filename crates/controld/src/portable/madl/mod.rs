//! MADL (the Multi-Agent Descriptor Language) is Sculptured's team file: a team exports as a
//! `.madl.json` document, and MADL documents in JSON or YAML import as teams. What changes while a
//! team runs stays in Sculptured's store; everything that describes the team is in MADL's core.
mod agents;
mod care;
mod export;
mod import;
mod limits;
mod links;
mod tools;
mod workflow;

pub use export::{MADL_SCHEMA_URL, to_madl};
pub use import::{Imported, from_madl};

use crate::api::{fingerprint, idempotent};
use crate::error::{ApiError, respond, to_value};
use crate::extract::{Json, Path, Query};
use crate::state::AppState;
use crate::views::TeamImport;
use axum::extract::State;
use axum::response::Response;
use rigspec::{Diagnostic, Severity};
use serde::{Deserialize, Serialize};

mod format;
pub(super) use format::{check_madl, get_madl, import_madl};
pub use format::{export, read, read_document};
