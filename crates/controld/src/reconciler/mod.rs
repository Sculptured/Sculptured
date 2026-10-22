//! M1 reconciler: imperative boot (layer by layer) and stop. The continuous
//! desired-vs-observed loop and restore-on-boot arrive in M2.
mod jobs;
mod seats;

pub(crate) use seats::{stop_on_host, stoppable};

use crate::error::ApiError;
use crate::hosts::controld;
use crate::launch::launch_params_with_connection;
use crate::seat_ids::rig_of;
use crate::state::{AppState, BootGuard};
use crate::store::{RigSource, is_live};
use host_proto::HostCommand;
use model::{Actor, Desired, EventBody, Provenance, SeatLifecycle, Subject};
use rigspec::{CompiledRig, Runtime};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub use crate::launch::driver_for;

pub use jobs::{HANDOFF_ACTOR, STARTED_AGAIN, TEAM_START_ACTOR, deliver_jobs};
pub use seats::{
    interrupt_seat, restart_seat, send_message, send_message_as, stop_rig, stop_seat, stop_seat_now,
};

mod reconcile;
use reconcile::{READY_TIMEOUT, STOP_GRACE_MS};
pub use reconcile::{boot, boot_claimed, resolve_cwd, runtime_words, start_rig, start_rig_claimed};
pub(crate) use reconcile::{launch_one, lifecycle, wait_settled};
