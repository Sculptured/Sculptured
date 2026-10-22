//! Export: the draft's RigSpec YAML plus one trailing comment carrying what a RigSpec
//! cannot hold (display name, jobs, positions, every drawn group including empty ones).
//! OpenRig ignores comments, so the file stays a RigSpec. A Claude or Codex agent with no
//! folder is written with a blank `cwd` rather than an invented one, so such a file does
//! not compile until a folder is chosen. Import: any valid RigSpec becomes a draft (a blank
//! `cwd` imports as "no folder"); what the builder cannot represent is listed as dropped,
//! never silently lost.
mod export;
mod import;
mod layout;
mod meta;

use crate::{Diagnostic, Severity};

pub use export::export_yaml;
pub use import::import_yaml;

mod format;
pub use format::{Imported, META_PREFIX, NOT_IMPORTED};
use format::{dropped, dropped_with};
