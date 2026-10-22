//! Run budgets and agent limits, each with a soft value (no new work; agents wrap up) and a hard
//! value (agents are stopped at once). Absent = no budget: Sculptured sets none of its own.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The share of the hard value that the soft value defaults to.
pub const SOFT_SHARE: f64 = 0.85;

/// A limit with a soft and a hard value. `soft` absent = 85% of `hard`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct SoftHard<T> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub soft: Option<T>,
    pub hard: T,
}

macro_rules! soft_hard_int {
    ($t:ty) => {
        impl SoftHard<$t> {
            /// The soft value; absent, 85% of hard rounded down, but at least 1 when hard is.
            pub fn soft_value(&self) -> $t {
                self.soft.unwrap_or_else(|| {
                    ((self.hard as f64 * SOFT_SHARE).floor() as $t).max(self.hard.min(1))
                })
            }
            /// What is wrong with it, in words naming `what`; None when it is usable.
            pub fn problem(&self, what: &str) -> Option<String> {
                if self.hard == 0 {
                    return Some(format!("{what}: the hard value must be above 0"));
                }
                (self.soft_value() > self.hard)
                    .then(|| format!("{what}: the soft value is above the hard value"))
            }
        }
    };
}
soft_hard_int!(u32);
soft_hard_int!(u64);

/// A share of one plan window that a run may add, in percent of the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct PlanBudget {
    /// `claude` or `codex`.
    pub provider: String,
    /// `five_hour` or `seven_day`.
    pub window: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub soft: Option<f64>,
    pub hard: f64,
    /// At the hard value: absent, every agent is stopped; set, the run pauses and continues by itself
    /// once the window resets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pause: Option<PlanPause>,
}

/// How a run paused at a plan budget continues after its window resets.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ContinueMode {
    /// Each agent starts a fresh conversation from the note it wrote when the run wrapped up.
    #[default]
    Notes,
    /// Each agent continues its own conversation (it re-reads all of it on every call).
    Resume,
}

/// Pause at the hard value, then continue after the window resets, for at most `max_windows` windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct PlanPause {
    #[serde(default)]
    pub continue_with: ContinueMode,
    #[serde(default = "three_windows")]
    pub max_windows: u32,
}

fn three_windows() -> u32 {
    3
}

// Percents are checked to be finite numbers, so equality is total.
impl Eq for PlanBudget {}

impl PlanBudget {
    #[must_use]
    pub fn soft_value(&self) -> f64 {
        self.soft.unwrap_or(self.hard * SOFT_SHARE)
    }

    fn problem(&self) -> Option<String> {
        let ok = |p: f64| p.is_finite() && p > 0.0 && p <= 100.0;
        if !matches!(self.provider.as_str(), "claude" | "codex") {
            return Some(format!(
                "Plan budget: unknown plan {:?} (use claude or codex)",
                self.provider
            ));
        }
        if !matches!(self.window.as_str(), "five_hour" | "seven_day") {
            return Some(format!(
                "Plan budget: unknown window {:?} (use five_hour or seven_day)",
                self.window
            ));
        }
        if !ok(self.hard) || self.soft.is_some_and(|s| !ok(s)) {
            return Some(format!(
                "Plan budget: {} is not a percent between 0 and 100",
                self.soft.filter(|s| !ok(*s)).unwrap_or(self.hard)
            ));
        }
        if self.pause.is_some_and(|p| p.max_windows == 0) {
            return Some("Plan budget: a paused run needs at least 1 window".to_string());
        }
        (self.soft_value() > self.hard)
            .then(|| "Plan budget: the soft value is above the hard value".to_string())
    }
}

/// What one run of a team may use, all agents and their children together.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS,
)]
#[ts(export)]
pub struct RunBudget {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<PlanBudget>>", optional)]
    pub plan: Vec<PlanBudget>,
    /// Tokens of every kind (input, output, cache reads and writes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "SoftHard<number>")]
    pub tokens: Option<SoftHard<u64>>,
    /// Wall-clock minutes from Start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub minutes: Option<SoftHard<u32>>,
    /// Most agents working at the same time; the rest wait for a free slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_working: Option<u32>,
}

impl RunBudget {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plan.is_empty()
            && self.tokens.is_none()
            && self.minutes.is_none()
            && self.max_working.is_none()
    }

    /// Everything wrong with it, in words; empty when it is usable.
    pub fn problems(&self) -> Vec<String> {
        let mut out: Vec<String> = self.plan.iter().filter_map(PlanBudget::problem).collect();
        out.extend(self.tokens.and_then(|t| t.problem("Token budget")));
        out.extend(self.minutes.and_then(|m| m.problem("Time budget")));
        if self.max_working == Some(0) {
            out.push("Most agents working at once must be at least 1".to_string());
        }
        out
    }
}

/// Which of the person's own Claude Code setup Claude agents load. Sculptured's own tools load either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct ClaudeSetup {
    /// `~/.claude`: plugins, skills, hooks, settings and MCP servers. Off by default: a team's agents
    /// rarely need them, and every agent re-reads them on every model call.
    #[serde(default)]
    pub global: bool,
    /// The agent folder's `.claude/` (settings, plugins, skills) and local settings. On by default.
    #[serde(default = "on")]
    pub project: bool,
}

fn on() -> bool {
    true
}

impl Default for ClaudeSetup {
    fn default() -> Self {
        Self {
            global: false,
            project: true,
        }
    }
}

impl ClaudeSetup {
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The Claude Code flags for this setup: which setting files load, and (without the person's
    /// global setup) only the MCP servers Sculptured passes.
    #[must_use]
    pub fn claude_args(&self) -> Vec<String> {
        let mut sources = Vec::new();
        if self.global {
            sources.push("user");
        }
        if self.project {
            sources.extend(["project", "local"]);
        }
        let mut a = vec!["--setting-sources".to_string(), sources.join(",")];
        if !self.global {
            a.push("--strict-mcp-config".to_string());
        }
        a
    }
}
