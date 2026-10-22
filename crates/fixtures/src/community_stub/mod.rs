//! A stand-in for the Sculptured server (gateway device pairing plus Community's `/api/community/*`)
//! for controld tests and e2e. It records every request, answers like the real server in the cases
//! the app handles, and has control routes (`/stub/*`) for Playwright. No real network.
// A refusal is an axum `Response` (a test fixture: its size does not matter).
#![allow(clippy::result_large_err)]
mod api;
mod devices;
mod seed;

use axum::body::{Body, to_bytes};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use seed::{SEED_BLOCK, SEED_OTHER_KIND};

mod server;
pub use server::{Block, PairingMode, Seen, Stub, StubState, User, router, spawn, spawn_on};
pub(crate) use server::{Shared, error};
