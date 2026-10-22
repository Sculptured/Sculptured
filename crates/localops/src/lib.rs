//! What Sculptured does on the computer that runs a team: folders, git, agents' own copies, checks,
//! CLI sign-in and plan usage. controld calls it in-process (local mode); a connected computer's
//! runner calls it for its owner's hosted Sculptured (after its own policy check).
pub mod analyze;
pub mod checks;
pub mod clis;
mod clone;
pub mod command;
pub mod copies;
pub mod folders;
pub mod generate;
pub mod git;
pub mod map;
pub mod plan;
mod process;
pub mod programs;

mod context;
mod executor;
mod local_error;
mod local_op;

pub use context::Context;
pub use executor::execute;
pub use local_error::LocalError;
pub use local_op::{ItemRef, LocalOp};
