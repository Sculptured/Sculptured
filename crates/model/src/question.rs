//! Questions an agent puts to the operator (spec §4, §6.5, decision D8).
//! A question is open until answered, expired (the agent then decides) or withdrawn.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum QuestionSource {
    /// The agent called its `request_input` tool.
    RequestInput,
    /// The CLI's own ask-the-user prompt (Codex `item/tool/requestUserInput`, a Practice agent's `ask:`).
    NativeQuestion,
    /// The CLI asks before running a tool. Only a restricted posture produces these.
    NativePermission,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum QuestionState {
    Open,
    Answered,
    Expired,
    Withdrawn,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

/// What the agent asked, verbatim (capped). Shown as evidence, never rewritten.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct QuestionPrompt {
    pub header: Option<String>,
    pub text: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    /// Free text is accepted besides the options.
    #[serde(default)]
    pub allow_other: bool,
    /// Permission prompts: the tool the agent wants to run.
    pub tool: Option<String>,
    /// Permission prompts: the tool's input, verbatim.
    pub tool_input: Option<serde_json::Value>,
    /// How urgent the asking agent says it is (agent-reported); urgent ones come first in Needs you.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub urgency: Option<Urgency>,
}

/// An agent's own word on how much a question holds up (an idea from Gas Town's escalation severity).
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Urgency {
    /// A preference it could decide itself.
    Low,
    Normal,
    /// The work cannot go on, or others wait on the answer.
    High,
}

/// Cuts `s` to at most `max` bytes on a char boundary, marking the cut with `…`.
pub(crate) fn cut(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut out = s;
    out.truncate(crate::cap::head(&out, max).len());
    out.push('…');
    out
}

impl QuestionPrompt {
    pub const MAX_TEXT: usize = 4096;
    pub const MAX_OPTIONS: usize = 20;
    pub const MAX_LABEL: usize = 200;

    /// Bounds agent-supplied text so a runaway or hostile prompt cannot bloat the store or the inbox.
    pub fn capped(self) -> Self {
        // A permission prompt (it names a tool) is cut in the middle, so the end of a command stays visible.
        let permission = self.tool.is_some();
        Self {
            urgency: self.urgency,
            header: self.header.map(|h| cut(h, Self::MAX_LABEL)),
            text: if !permission {
                cut(self.text, Self::MAX_TEXT)
            } else if self
                .tool_input
                .as_ref()
                .is_some_and(crate::cap::approval_input_kept_whole)
            {
                // The whole command is kept in `tool_input`, so the marker can point to it.
                crate::cap::approval_cut(self.text, Self::MAX_TEXT)
            } else {
                crate::cap::approval_cut_unstored(self.text, Self::MAX_TEXT)
            },
            options: self
                .options
                .into_iter()
                .take(Self::MAX_OPTIONS)
                .map(|o| QuestionOption {
                    label: cut(o.label, Self::MAX_LABEL),
                    description: o.description.map(|d| cut(d, Self::MAX_TEXT / 4)),
                })
                .collect(),
            allow_other: self.allow_other,
            tool: self.tool.map(|t| cut(t, Self::MAX_LABEL)),
            // Bounded like a captured tool call (`ToolCalled`), but a long command keeps its end.
            tool_input: self.tool_input.map(crate::cap::cap_approval_input),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[ts(export)]
pub enum Answer {
    /// A typed answer or a chosen option's label.
    Text { text: String },
    /// Permission prompts: run the tool.
    Allow,
    /// Permission prompts: do not run it.
    Deny { reason: Option<String> },
    /// Nobody answered in time (D8): an agent question is told to decide; a permission prompt is allowed.
    Expired,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum DecisionSource {
    /// The agent called `record_decision` (structured, agent-reported).
    RecordDecision,
    /// The agent's next message after the timeout (unstructured, agent-reported).
    NextMessage,
    /// A permission prompt that Sculptured allowed on timeout (D8).
    AllowedOnTimeout,
    /// The agent did not say what it decided.
    Unknown,
}

/// How a new session for an agent starts (spec §6.8 "handover"; "New session" in the UI).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum SessionMode {
    /// A new session primed with a handover note, then sent its unfinished work.
    Fresh,
    /// A native fork of the current session (Codex, Practice; not Claude in M2).
    Fork,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum RestoreDecision {
    /// Try resuming the agent's previous session (again).
    Resume,
    /// Start a fresh session; unfinished work is sent with the continue message.
    Fresh,
    /// Leave the agent stopped.
    Leave,
}

/// The message an agent question gets when nobody answered in time (spec §6.5, D8).
#[must_use]
pub fn timeout_instruction(timeout_ms: u64) -> String {
    let after = if timeout_ms >= 60_000 {
        format!("{}m", timeout_ms / 60_000)
    } else {
        format!("{}s", timeout_ms.div_ceil(1000))
    };
    format!(
        "No operator answer within {after}. Proceed with your best judgment and state the decision you made."
    )
}
