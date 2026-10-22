//! `CodexParser` is pure: the driver feeds it every stdout line and tells it which
//! `turn/start` request carried which message; it returns driver events and the replies
//! the driver must write back at once (refusals of server requests nobody here handles).
mod items;
mod requests;
mod turns;

use turns::Totals;

use super::EXPIRED_ANSWER;
use crate::DriverEvent;
use model::Answer;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet, VecDeque};

mod events;
pub use events::{CodexParser, Parsed};
use events::{Pending, rpc_text, s};
