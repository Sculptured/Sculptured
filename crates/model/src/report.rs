//! What an agent reports about its work through the `report` seat tool (spec §6.3).
//! Always agent-reported: shown as `◇ reported`, never as verified.
use crate::QuestionPrompt;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_FINDINGS: usize = 50;
pub const MAX_FINDING: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Finding {
    /// The finding in the agent's words.
    pub text: String,
    /// Where, as the agent put it (a file and line, a page, a part name). None when not given.
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ReportVerdict {
    NoProblems,
    ProblemsFound,
    Unsure,
}

/// Bounds agent-supplied findings: at most `MAX_FINDINGS`, each text `MAX_FINDING` bytes.
#[must_use]
pub fn cap_findings(findings: Vec<Finding>) -> Vec<Finding> {
    findings
        .into_iter()
        .take(MAX_FINDINGS)
        .map(|f| Finding {
            text: crate::question::cut(f.text, MAX_FINDING),
            location: f
                .location
                .map(|l| crate::question::cut(l, QuestionPrompt::MAX_LABEL)),
        })
        .collect()
}
