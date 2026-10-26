//! OpenCode via `opencode acp`: the Agent Client Protocol (ACP v1), JSON-RPC 2.0 over stdio, as recorded
//! with OpenCode 1.19.4 against a stand-in for OpenRouter (`crates/fixtures/recordings/opencode-*`).
//!
//! - **Models.** OpenCode's own `provider/model` (`openrouter/deepseek/deepseek-chat-v3.1`), chosen
//!   with `session/set_config_option`; a model OpenCode does not know fails the launch in plain words.
//!   The key comes from the agent's environment (`OPENROUTER_API_KEY` from a model connection) or the
//!   person's own OpenCode setup.
//! - **Posture (D7).** Set in OpenCode's config (`config::config`): unrestricted allows everything;
//!   restricted asks before changing files, running commands and using the web, and each ask becomes an
//!   operator question (`session/request_permission`).
//! - **Seat tools** and integrations are `mcpServers` on `session/new` / `session/resume`, over stdin:
//!   `seat_tools_config_file()` is false. OpenCode shows them to the model as `rig_<tool>`.
//! - **Queueing.** One prompt at a time; a message sent while one runs waits for its answer.
//! - **Interrupt** is `session/cancel`; the prompt answers `stopReason: cancelled`.
//! - **Resume** is `session/resume` (no history replayed). Fork is not offered: ACP v1's `session/fork`
//!   is unstable.
//! - **Usage.** Tokens are the turn's last model call only and the cost is OpenCode's estimate for the
//!   whole turn (see `parser`).
//! - **Stop** closes stdin and kills the process group after the grace period.
mod config;
mod core;
mod parser;
mod session;

use crate::{Driver, DriverError, LaunchSpec, Session};
use async_trait::async_trait;
use model::Caps;
use std::path::PathBuf;

pub use config::config as opencode_config;
pub use parser::{AcpParser, Parsed, TOOL_UNFINISHED, current_model};

/// `opencode acp`, one process per session.
pub struct OpencodeAcpDriver {
    pub program: PathBuf,
}

impl Default for OpencodeAcpDriver {
    fn default() -> Self {
        Self {
            program: PathBuf::from("opencode"),
        }
    }
}

pub(super) enum Open<'a> {
    Start,
    Resume(&'a str),
}

#[async_trait]
impl Driver for OpencodeAcpDriver {
    fn name(&self) -> &'static str {
        "opencode-acp"
    }
    fn capabilities(&self) -> Caps {
        Caps {
            interrupt: true,
            questions: false,
            resume: true,
            fork: false,
            usage: true,
            mid_turn_input: false,
            subagent_events: false,
        }
    }
    async fn launch(&self, spec: LaunchSpec) -> Result<Box<dyn Session>, DriverError> {
        self.spawn(spec, Open::Start).await
    }
    async fn resume(
        &self,
        spec: LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn Session>, DriverError> {
        self.spawn(spec, Open::Resume(native_id)).await
    }
    /// The seat-tools config goes in `mcpServers` over stdin.
    fn seat_tools_config_file(&self) -> bool {
        false
    }
}
