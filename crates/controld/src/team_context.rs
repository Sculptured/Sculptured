//! Operator-authored architecture, delivered with work to every team member.
use crate::state::AppState;
use model::{ConnectionKind, DraftAgent, TeamDraft, Workflow};

mod minimal;

mod briefing;
use briefing::own_contract;
pub(crate) use briefing::{
    briefed, briefing, briefing_with, decision_log_note, held_notes_in, with_context,
};
