//! Messages between controld and a host runtime, as JSON text frames over a WebSocket.
//! hostd dials controld, sends Hello, and receives Welcome{resume_after}. It then
//! replays journal events with host_seq > resume_after and streams live events.
//! controld acks the highest ingested host_seq; hostd prunes its journal up to it.
use model::{
    Actor, Answer, Caps, EventBody, MessageIntent, PermissionPosture, Provenance, Subject,
    TenureCause,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// What a hostd can do beyond M1, advertised in `FromHost::Hello`. A hostd that started before an upgrade
/// keeps running (it owns the agents), so controld checks these before sending a command that needs one.
pub mod features;

/// The seat-tools socket: one JSON line from the agent's shim (or a Practice agent) to hostd, one back.
pub mod seat_tools;

/// An agent's process environment, and the only part of it the server may set.
mod env;
pub use env::{Env, MODEL_BASE_URLS, MODEL_ENV_KEYS, model_env_allowed};

mod protocol;
pub use protocol::{
    DriverInfo, FromHost, HostCommand, HostEvent, HostPolicyInfo, LaunchMode, LaunchParams,
    OpError, OwnCopy, PROTOCOL_VERSION, SeatToolsParams, ToHost, decode, encode,
};
