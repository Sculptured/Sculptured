//! The agent tools (spec §6.3): `request_input`, `record_decision`, `report` and, in restricted posture,
//! `approve`. Calls arrive from hostd as `FromHost::SeatTool`; every reply is `{"text": …}` (the text the
//! agent reads). An invalid call is stored as `report-rejected` evidence and answered with a tool error.
//!
//! Delivery of an answer to a waiting call: controld knows a reply cannot arrive when the call's host
//! connection has closed (`SeatWait::send` refuses, an answer handed over then is recorded as failed,
//! and the question is withdrawn). A per-reply acknowledgement from hostd is deferred: a reply handed to
//! a connection that dies before hostd reads it (or a hostd that dies) is not detected here, and stays
//! unconfirmed like any other delivery without a reply.
mod ask;
mod propose;
mod records;

use crate::state::AppState;
use host_proto::{SeatToolsParams, ToHost};
use model::{Actor, Answer, EventBody, PermissionPosture, Provenance, Subject};
use rigspec::{CompiledSeat, Runtime};
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

mod tool_calls;
use tool_calls::{
    Outcome, ToolError, current_turn, cut, invalid, record, record_system, string_arg,
};
pub use tool_calls::{SeatWait, handle_call, launch_params};
