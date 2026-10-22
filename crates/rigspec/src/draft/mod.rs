//! Team drafts → RigSpec. Ids come from the draft's immutable ids; names become labels.
//! Every diagnostic maps back to the draft element it concerns, in plain words.
mod budget;
mod build;
mod check;
mod locate;
mod names;
mod workflow;

use crate::{CompiledRig, Diagnostic, RigSpec, Severity, compile};
use build::build;
use locate::map_with;
use model::{DraftIssue, IssueSeverity, TeamDraft};

pub use check::check_draft;
pub use names::{display_name, slug, unique_slug, verb};

mod compiler;
pub use compiler::{
    AGENT_REF, DraftBuild, GENERIC_AGENT, GENERIC_CONNECTION, GENERIC_GROUP, GENERIC_TEAM,
    RESTRICTED_POLICY, SPEC_VERSION, draft_spec, from_draft, map_diagnostic, spec_yaml,
    validate_draft,
};
