//! Run budgets: what one run of a team may use (plan share, tokens, time), each with a soft value
//! (no new work; every agent wraps up) and a hard value (every agent is stopped at once). Checked
//! every second and after every usage or plan report. See docs/internals/limits-and-usage.md.
pub mod gate;
pub mod measure;
pub mod pause;
pub mod raise;
pub mod record;

use crate::state::AppState;
use crate::store::Store;
use measure::Meter;
pub use record::{BudgetState, Record};
use record::{PlanTrack, Reading};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod monitor;
pub use monitor::{RunStatus, begin, check, decide, forget, run, status};
use monitor::{measure, record, running};
