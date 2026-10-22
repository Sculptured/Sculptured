//! SQLite event store. Each event and its projection commit in one transaction.
mod handoffs;
mod origins;
mod project;
mod rigs;
mod rows;
mod runs;
mod schema;
mod stall;
pub use stall::Silent;
pub mod account;
mod collaboration;
mod decisions;
mod schedules;
mod workflow;

use crate::views::{Receipt, SeatView, StreamItem};
use host_proto::HostEvent;
use model::{Actor, EventBody, EventRecord, MessageState, Provenance, SeatLifecycle, Subject};
use project::{append_local_in, insert_event, item_for, local_event};
use rows::{cursor_in, events_of_types_in, message_column, query_events, seat_in, type_list};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub use collaboration::{ControlMode, SoftStop};
pub use decisions::DecisionEntry;
pub use handoffs::{HandedOverRecord, HandoffWrite, ResendText};
pub use project::STOPPED_BEFORE_READING;
pub use rigs::{RigSource, SaveDraft};
pub(crate) use rows::{enum_parse, enum_text, json_parse};
pub use schedules::ScheduleRun;
pub use schema::SCHEMA_VERSION;

mod event_store;
pub use event_store::{LOCAL_EVENT_HOST, Store, is_live};
