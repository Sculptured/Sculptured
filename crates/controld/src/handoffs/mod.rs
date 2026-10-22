//! Handoffs along "hands work to" connections (`delegates_to` edges) of a started team.
//!
//! **Work.** An agent's work is the Job or handoff message it read (for a first agent
//! with no Job: the first message you send it). That work is unfinished until a turn
//! ends Completed after the agent read it; that turn is a **work turn** and its last reply
//! is the result passed along. A turn that ends Interrupted or Failed, or a process that
//! dies, leaves the work unfinished: the agent is "stuck", and the next Completed turn
//! (after "continue", or after Restart re-sends the work) finishes it. A turn answering the
//! operator otherwise never hands anything over.
//!
//! **First handoff.** A downstream agent's first handoff of a start waits until every
//! upstream agent has a result; it carries the agent's Job and each upstream's latest
//! result.
//!
//! **Waves.** After that, "More work" moves in waves. A wave starts at one agent: its work
//! turn that was not itself part of a wave, or a "Hand over again" press on it. Every
//! agent downstream of that origin gets one message for the wave, once each of its
//! upstreams that the wave reaches has finished its turn for the wave, so each agent runs
//! once per wave.
//!
//! **Durability.** Nothing here is kept only in memory (except which sends are in flight
//! right now). A start is the team rig's latest `RigBooted` after the handoff cutoff; what
//! was handed over is the `HandedOver` events (each names its start and wave), recorded
//! with each message in one transaction (`Store::record_handoff`). So the dispatcher can
//! recompute what is due from the store at any time: after a controld restart, after a
//! lagged hub, or on a replayed event, without handing the same work over twice.
//!
//! **Stuck.** An agent whose handoffs cannot move on by themselves says why: its own work
//! did not finish, a handoff never reached it, or an agent it waits for is stuck, not
//! running, or running with nothing left to do (a wait that would never end by itself).
//!
//! Cost: every relevant event re-plans its whole team from the store, reading only the
//! events of the start being planned (never earlier starts). That is fine for teams of a
//! few dozen agents; an incremental planner can come later if long starts show it up.
mod operator;
mod owed;
mod plan;
mod start_plan;
mod starts;
mod team;
mod texts;
mod work;

use crate::hosts::controld;
use crate::seat_ids::rig_of;
use crate::state::AppState;
use crate::store::{HandoffWrite, Store};
use host_proto::HostCommand;
use model::{
    Actor, EventBody, EventRecord, HandoffSource, HandoffStuck, MessageIntent, Provenance, Subject,
    Wave,
};
use owed::restart_key;
use std::sync::atomic::Ordering;
use std::time::Duration;
use team::TeamNames;
use tokio::sync::broadcast::error::RecvError;

pub use operator::{request_handover, resend};
pub use owed::{OwedMode, Restarting, continue_text, deliver_owed};
pub use texts::{cap_result, first_text, more_text};

mod handoff;
pub(crate) use handoff::TEAM_START_ACTOR;
pub(super) use handoff::{Action, InFlight, OWED_WAIT, send};
pub use handoff::{
    CONTINUE_PREFIX, HANDOFF_ACTOR, LOOP_ROUNDS, RESULT_CAP, TRUNCATED, current_start,
    ensure_running, process_rig,
};
