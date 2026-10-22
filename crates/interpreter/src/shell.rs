//! Shell commands: their purpose and risks, without running anything.
mod classify;
mod lex;
mod risk;

pub use classify::classify_command;
pub use lex::unwrap_shell;
pub use risk::risks;
