//! The CLI's own words when a usage or rate limit stops a turn.
use model::LimitKind;

/// How the installed CLI (2.1.292) starts the words it shows when the plan's allowance stops a turn
/// (its own list, lowercased). "You've hit your ${limit}" covers the session, weekly, Opus, Sonnet,
/// Fable, usage credit and monthly spend limits.
const USAGE_LIMIT_STARTS: [&str; 11] = [
    "you've hit your",
    "you've reached your",
    "you're out of usage credits",
    "you're out of extra usage",
    "your org is out of usage",
    "your seat type doesn't include usage credits",
    "your seat type doesn't include usage",
    "your seat type doesn't include extra usage",
    "your usage allocation has been disabled by your admin",
    "your group's usage limit is set to $0",
    "fable 5 requires usage credits",
];

/// The CLI's "<model> [version] requires usage credits." words.
fn requires_credits(line: &str) -> bool {
    let head: String = line.chars().take(80).collect();
    ["fable", "haiku", "mythos", "opus", "sonnet"]
        .iter()
        .any(|m| line.starts_with(m))
        && head.contains(" requires usage credits")
}

/// Which limit the CLI's words name, if any. Read only on a turn that failed, so an agent that merely
/// talks about limits in a finished turn is never taken for one. A rate limit names itself; a usage
/// limit is any line that starts like the CLI's own limit words, or words naming a limit and when it
/// resets.
/// The CLI's own usage-limit message, when an agent text starts with it (and only then): the CLI's
/// limit words reach the stream as the turn's last agent text. Substring rules never apply to agent
/// text, which may be the agent writing about limits.
#[must_use]
pub fn cli_limit_message(text: &str) -> Option<LimitKind> {
    let t = text
        .trim_start()
        .to_ascii_lowercase()
        .replace('\u{2019}', "'");
    (USAGE_LIMIT_STARTS.iter().any(|s| t.starts_with(s)) || requires_credits(&t))
        .then_some(LimitKind::Usage)
}

pub fn limit_kind(text: &str) -> Option<LimitKind> {
    let t = text.to_ascii_lowercase().replace('\u{2019}', "'");
    if t.contains("rate limit") {
        return Some(LimitKind::Rate);
    }
    let named = t
        .lines()
        .map(str::trim_start)
        .any(|l| USAGE_LIMIT_STARTS.iter().any(|s| l.starts_with(s)) || requires_credits(l));
    (named || (t.contains("limit") && t.contains("resets"))).then_some(LimitKind::Usage)
}
