//! `sculptured serve` runs controld and supervises a local hostd child process.
//! `sculptured hostd` runs the host runtime; controld normally starts it.
mod connect;
mod doctor;
mod guard;
mod run;
mod runners;
mod web_cache;
use axum::Router;
use clap::{Parser, Subcommand};
use controld::hosts::{HostRegistry, LOCAL_HOST};
use controld::store::Store;
use controld::{AppState, app};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use tower_http::services::{ServeDir, ServeFile};
use tracing_subscriber::EnvFilter;

mod application;
pub(crate) use application::start_local;
use application::{main, run};
