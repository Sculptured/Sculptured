//! Durable, bounded work-item workflows over a reusable pool of team agents.
mod assignment;
mod dispatch;
mod engine;
mod guard;
pub(crate) mod jobs;
mod routes;
mod scheduler;
pub use jobs::Jobs;
#[cfg(test)]
mod dispatch_tests;
mod stage_checks;
mod stage_copies;
mod stage_merge;
#[cfg(test)]
mod tests;
mod tools;
mod topics;

use crate::store::Store;
use model::{Desired, Subject, TaskStatus, TeamDraft};
pub use routes::routes;
pub use scheduler::run;
pub use tools::call;

mod status;
pub use status::decisions;
pub(super) use status::running;
