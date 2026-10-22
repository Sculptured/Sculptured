//! Portable formats: a team as a MADL document (`madl`), and a run (`sculptured.session/v1`), whose
//! JSON Schema is generated from these same types and published under `web/public/schema/`.
pub mod convert;
pub mod madl;
pub mod session;
pub mod team;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};
use schemars::Schema;

mod formats;
pub use formats::{routes, schemas};
