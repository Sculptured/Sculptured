//! Host connections: hostd dials `GET /host` with a bearer token, says Hello, and
//! receives Welcome{resume_after}. Events are then ingested and acked one by one,
//! and commands flow back with request/response correlation.
mod registry;
mod session;

use crate::state::AppState;
use host_proto::{LaunchParams, features};
use model::{Actor, EventBody, Provenance, SeatLifecycle, Subject};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

pub use registry::{CallStart, CommandFailure, GateFn, HostRegistry, OpFailure, RECONNECT_GRACE};
pub use session::host_ws;

mod host_link;
use host_link::{HELLO_TIMEOUT, Pending, mark_stale, runner};
pub use host_link::{LOCAL_HOST, fit_params, not_connected, older_runner};
pub(crate) use host_link::{controld, record_lifecycle};
