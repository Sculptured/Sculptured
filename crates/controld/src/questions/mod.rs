//! Questions and the Needs-you inbox (spec §5.3, §6.5, D8). The first of "answer" and "timeout" wins
//! (a process-wide claim per question); an answer that loses becomes a late, overriding instruction.
mod answers;
mod inbox;
mod watch;

use crate::api::{fingerprint, idempotent};
use crate::error::{ApiError, respond, to_value};
use crate::extract::{Json, Path};
use crate::projections::questions::{QuestionRow, question, questions_where};
use crate::state::AppState;
use crate::views::QuestionView;
use axum::Router;
use axum::extract::State;
use axum::response::Response;
use axum::routing::{get, post};
use model::{Actor, Answer, EventBody, Provenance, QuestionSource, Subject};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

pub use answers::{acknowledge, answer, expire, send_again};
pub use inbox::inbox;
pub use watch::run;

mod inbox_service;
pub use inbox_service::routes;
use inbox_service::{
    DECISION_CHARS, RESTARTED, claim, human, load, record, record_or_log, timeout_actor,
    wait_for_claim,
};
pub(crate) use inbox_service::{tool_claim, view};
