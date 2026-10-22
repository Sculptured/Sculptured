//! After a restart: reattach agents that kept running, and offer the rest to the operator's one
//! "Resume N agents?" answer per team (`decide`). Nothing is relaunched without that answer.
mod decide;
pub(crate) use decide::{relaunch_and_send, usage_baseline};

use crate::hosts::{LOCAL_HOST, controld, record_lifecycle};
use crate::state::AppState;
use crate::store::RigSource;
use crate::views::SeatView;
use axum::Router;
use model::{Desired, EventBody, Provenance, SeatLifecycle, Subject};

mod recovery;
pub use recovery::{on_hello, routes};
