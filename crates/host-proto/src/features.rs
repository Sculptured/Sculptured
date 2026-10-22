/// Honours `LaunchParams.posture` (a restricted launch is not run unrestricted).
pub const POSTURE: &str = "posture";
/// `HostCommand::Restart` (resume, fork, or fresh with a cause).
pub const RESTART: &str = "restart";
/// `HostCommand::Answer`.
pub const ANSWER: &str = "answer";
/// Offers Sculptured's tools to agents (`LaunchParams.seat_tools`).
pub const SEAT_TOOLS: &str = "seat-tools";
/// Seeds a resumed session's usage (`LaunchParams.usage_baseline`).
pub const USAGE_BASELINE: &str = "usage-baseline";
/// Answers `ToHost::Op` (local operations: folders, git, copies, CLIs, plan usage).
pub const LOCAL_OPS: &str = "local-ops";
/// Honours `LaunchParams.claude_setup` (leaves out the person's own Claude Code setup).
pub const CLAUDE_SETUP: &str = "claude-setup";
/// Answers `LocalOp::AnalyzeRun` (where a run's tokens went).
pub const USAGE_REPORTS: &str = "usage-reports";
/// Honours `LaunchParams.protection` (guards the agent's tool calls).
pub const PROTECTION: &str = "protection";
/// Honours `LaunchParams.skills`.
pub const SKILLS: &str = "skills";
/// Honours `LaunchParams.integrations`.
pub const INTEGRATIONS: &str = "integrations";
/// Honours `LaunchParams.components`.
pub const COMPONENTS: &str = "components";
/// Starts remote MCP servers and Claude plugins among `LaunchParams.components`; an older runner
/// cannot read them at all.
pub const COMPONENT_KINDS: &str = "component-kinds";
