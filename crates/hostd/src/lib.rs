//! Host runtime: supervises seat processes on one machine and streams their
//! events to controld. Agents keep running when controld is unreachable; events
//! wait in the journal until controld acknowledges them.
pub mod journal;
pub mod link;
pub mod local_stop;
pub mod mapping;
pub mod ops;
pub mod platform;
pub mod policy;
pub mod private;
pub mod run;
pub mod seat_tools;
pub mod supervisor;

pub use journal::Journal;
pub use link::{LinkError, run_link};
pub use run::{ALREADY_RUNNING, DISCONNECTED, HostdOptions, run_hostd};
pub use supervisor::Supervisor;
