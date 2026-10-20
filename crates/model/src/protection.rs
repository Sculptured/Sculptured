//! Protections: rules Sculptured applies between an agent and its tools. The first ones keep an agent's
//! context small (efficiency); the same layer takes safety rules later. A rule that cannot decide lets
//! the call through: a protection never stalls an agent.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Every refusal's reason starts with this, so the runner can tell a protection from other errors.
pub const MARK: &str = "Sculptured protection:";

/// A team's protections (Advanced). On by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Protection {
    #[serde(default = "yes")]
    pub on: bool,
    /// A read with no line range of a file larger than this many KB is refused.
    #[serde(default = "read_kb")]
    pub read_kb: u32,
    /// The most shell output, in characters, that reaches an agent's context.
    #[serde(default = "shell_chars")]
    pub shell_chars: u32,
    /// Past this many tokens of context, an agent writes its notes and continues fresh.
    #[serde(default = "context_tokens")]
    pub context_tokens: u32,
}

fn yes() -> bool {
    true
}
fn read_kb() -> u32 {
    100
}
fn shell_chars() -> u32 {
    8000
}
fn context_tokens() -> u32 {
    60_000
}

impl Default for Protection {
    fn default() -> Self {
        Self { on: true, read_kb: read_kb(), shell_chars: shell_chars(), context_tokens: context_tokens() }
    }
}

impl Protection {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// What a runner needs to guard an agent's calls; None when protections are off.
    pub fn guard_rules(&self) -> Option<GuardRules> {
        self.on.then(|| GuardRules { max_read_bytes: u64::from(self.read_kb) * 1024, shell_output_chars: self.shell_chars, ..GuardRules::default() })
    }

    /// The context ceiling in tokens; None when protections are off.
    pub fn context_ceiling(&self) -> Option<u64> {
        self.on.then_some(u64::from(self.context_tokens))
    }
}

/// The rules a runner applies to one agent's tool calls (sent with its launch).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardRules {
    pub max_read_bytes: u64,
    pub shell_output_chars: u32,
    /// The agent's data contract: paths relative to its folder, or absolute (a folder covers what is
    /// inside it). Empty = no limit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reads: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writes: Vec<String>,
    /// The team's safety rules, checked first on every tool call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub safety: Vec<crate::SafetyRule>,
}

impl GuardRules {
    /// The tools the hook must see: every tool with safety rules, edits too with a write contract.
    pub fn hook_matcher(&self) -> &'static str {
        if !self.safety.is_empty() {
            ""
        } else if !self.writes.is_empty() {
            "Read|Bash|Write|Edit|MultiEdit|NotebookEdit"
        } else {
            "Read|Bash"
        }
    }
}

/// Files read as a whole by nature (an image cannot be read in pieces): never refused.
const WHOLE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "pdf", "ipynb"];

/// Commands that print whole files.
const PRINTERS: &[&str] = &["cat", "tac", "nl", "less", "more", "bat", "batcat", "type"];

/// The refusal reason for a pending tool call (`hook` is the PreToolUse input), or None to let it run.
/// `size` gives a file's size in bytes (None if it cannot be read).
pub fn decide(rules: &GuardRules, hook: &Value, size: &dyn Fn(&Path) -> Option<u64>) -> Option<String> {
    let input = hook.get("tool_input")?;
    if let Some(refusal) = crate::safety::check(&rules.safety, hook.get("tool_name")?.as_str()?, input) {
        return Some(format!("{MARK} {refusal}"));
    }
    let cwd = hook.get("cwd").and_then(Value::as_str).map(PathBuf::from);
    let big = |file: &str| -> Option<u64> {
        let p = Path::new(file);
        let p = if p.is_absolute() { p.to_path_buf() } else { cwd.clone()?.join(p) };
        let whole = p.extension().and_then(|e| e.to_str()).is_some_and(|e| WHOLE.contains(&e.to_ascii_lowercase().as_str()));
        size(&p).filter(|n| !whole && *n > rules.max_read_bytes)
    };
    let absolute = |file: &str| Some(if Path::new(file).is_absolute() { PathBuf::from(file) } else { cwd.clone()?.join(file) });
    match hook.get("tool_name")?.as_str()? {
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            let file = input.get("file_path").or_else(|| input.get("notebook_path"))?.as_str()?;
            outside(&absolute(file)?, &rules.writes, &[], cwd.as_deref()).then(|| contract_reason(file, "writes", &rules.writes))
        }
        "Read" => {
            let file = input.get("file_path")?.as_str()?;
            if outside(&absolute(file)?, &rules.reads, &rules.writes, cwd.as_deref()) {
                return Some(contract_reason(file, "reads", &rules.reads));
            }
            let ranged = input.get("limit").is_some_and(|l| !l.is_null());
            if ranged { None } else { big(file).map(|n| reason(file, n)) }
        }
        "Bash" | "shell" | "exec_command" => {
            let command = input.get("command").and_then(command_text)?;
            if long_sleep(&command) {
                return Some(format!("{MARK} Waiting with a long sleep keeps this conversation open and costly. To wait for a teammate, end your turn: a message wakes you. To wait for a command, run it in the foreground."));
            }
            whole_file_printed(&command).and_then(|file| big(&file).map(|n| reason(&file, n)))
        }
        _ => None,
    }
}

/// True when `allowed` is a contract (not empty) and `path` is in none of it, nor in `also`, nor in the
/// agent's own `.sculptured/` folder (its notes).
fn outside(path: &Path, allowed: &[String], also: &[String], cwd: Option<&Path>) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let within = |root: &str| {
        let root = root.trim_start_matches("./").trim_end_matches("/**").trim_end_matches("/*").trim_end_matches('/');
        let root = match (Path::new(root).is_absolute(), cwd) {
            (true, _) => PathBuf::from(root),
            (false, Some(c)) => c.join(root),
            (false, None) => return false,
        };
        path.starts_with(root)
    };
    let notes = cwd.is_some_and(|c| path.starts_with(c.join(".sculptured")));
    !(notes || allowed.iter().chain(also).any(|r| within(r)))
}

fn contract_reason(file: &str, what: &str, allowed: &[String]) -> String {
    format!(
        "{MARK} {file} is outside what this agent {what} ({}). Stay within its data contract; for other files, ask the agent whose work they are.",
        allowed.join(", ")
    )
}

/// Longest sleep, in seconds, a shell command may wait for.
const SLEEP_SECONDS: f64 = 30.0;

/// True when one of the command's steps is `sleep` for longer than `SLEEP_SECONDS` (`120`, `5m`).
fn long_sleep(command: &str) -> bool {
    command.split(['\n', ';', '&', '|']).any(|step| {
        let mut words = step.split_whitespace().skip_while(|w| matches!(*w, "do" | "then" | "else"));
        if words.next() != Some("sleep") {
            return false;
        }
        words.any(|w| {
            let (n, unit) = w.find(|c: char| !(c.is_ascii_digit() || c == '.')).map_or((w, ""), |i| w.split_at(i));
            let scale = match unit { "" | "s" => 1.0, "m" => 60.0, "h" => 3600.0, "d" => 86_400.0, _ => return false };
            n.parse::<f64>().is_ok_and(|n| n * scale > SLEEP_SECONDS)
        })
    })
}

/// A shell command as text (Codex sends an argv list).
fn command_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => Some(a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")),
        _ => None,
    }
}

/// The file a command prints whole, when its output is not piped into a filter: `cat f`,
/// `sed -n '1,$p' f`. Anything unclear is None (the call goes through).
fn whole_file_printed(command: &str) -> Option<String> {
    let last = command.rsplit("&&").next()?.rsplit(';').next()?;
    if last.contains('|') || last.contains('>') {
        return None;
    }
    let words: Vec<&str> = last.split_whitespace().map(|w| w.trim_matches(|c| c == '\'' || c == '"')).collect();
    let (cmd, args) = words.split_first()?;
    let files: Vec<&&str> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let printer = PRINTERS.contains(cmd) || (*cmd == "sed" && args.iter().any(|a| *a == "1,$p" || a.is_empty()));
    match (printer, *cmd == "sed", files.as_slice()) {
        (true, false, [file]) => Some((**file).to_string()),
        (true, true, [_, file]) => Some((**file).to_string()),
        _ => None,
    }
}

fn reason(file: &str, bytes: u64) -> String {
    let name = Path::new(file).file_name().and_then(|n| n.to_str()).unwrap_or(file);
    format!(
        "{MARK} {name} is {} KB, too large to read whole. Search it with `grep -n` for what you need, then read only those lines (Read with offset and limit).",
        bytes.div_ceil(1024)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rules() -> GuardRules {
        Protection::default().guard_rules().unwrap()
    }

    fn sizes(p: &Path) -> Option<u64> {
        match p.file_name()?.to_str()? {
            "big.csv" | "page.png" | "manual.pdf" => Some(400_000),
            "small.rs" => Some(2_000),
            _ => None,
        }
    }

    fn call(tool: &str, input: Value) -> Option<String> {
        decide(&rules(), &json!({ "tool_name": tool, "tool_input": input, "cwd": "/work" }), &sizes)
    }

    #[test]
    fn a_large_file_read_whole_is_refused_and_a_ranged_or_small_read_is_not() {
        let r = call("Read", json!({ "file_path": "/work/big.csv" })).expect("refused");
        assert!(r.starts_with(MARK) && r.contains("big.csv is 391 KB") && r.contains("grep -n"), "{r}");
        assert_eq!(call("Read", json!({ "file_path": "/work/big.csv", "offset": 1, "limit": 200 })), None);
        assert_eq!(call("Read", json!({ "file_path": "/work/small.rs" })), None);
        assert_eq!(call("Read", json!({ "file_path": "/work/missing.txt" })), None, "unknown size: let it through");
    }

    #[test]
    fn waiting_with_a_long_sleep_is_refused_with_how_to_wait_instead() {
        let r = call("Bash", json!({ "command": "sleep 120 && cat status.txt" })).expect("refused");
        assert!(r.starts_with(MARK) && r.contains("end your turn"), "{r}");
        assert!(call("Bash", json!({ "command": "sleep 5m" })).is_some(), "minutes too");
        assert!(call("Bash", json!({ "command": "while true; do sleep 60; done" })).is_some());
        assert_eq!(call("Bash", json!({ "command": "sleep 2 && curl localhost:8080" })), None, "a short pause is fine");
        assert_eq!(call("Bash", json!({ "command": "grep -n sleep 600 notes.txt" })), None, "the word in arguments is not a sleep");
    }

    #[test]
    fn images_pdfs_and_notebooks_are_never_refused() {
        assert_eq!(call("Read", json!({ "file_path": "/work/page.png" })), None);
        assert_eq!(call("Read", json!({ "file_path": "/work/manual.pdf" })), None);
    }

    #[test]
    fn printing_a_large_file_whole_in_the_shell_is_refused_but_filtering_it_is_not() {
        assert!(call("Bash", json!({ "command": "cat big.csv" })).is_some(), "relative to cwd");
        assert!(call("Bash", json!({ "command": "cd /work && cat big.csv" })).is_some());
        assert!(call("Bash", json!({ "command": "sed -n '1,$p' big.csv" })).is_some());
        assert!(call("shell", json!({ "command": ["cat", "/work/big.csv"] })).is_some(), "Codex sends an argv list");
        assert_eq!(call("Bash", json!({ "command": "cat big.csv | grep wing" })), None);
        assert_eq!(call("Bash", json!({ "command": "grep -n wing big.csv" })), None);
        assert_eq!(call("Bash", json!({ "command": "cat small.rs" })), None);
        assert_eq!(call("Bash", json!({ "command": "cat a.csv b.csv" })), None, "unclear: let it through");
    }

    fn contract() -> GuardRules {
        GuardRules { reads: vec!["manuals".into()], writes: vec!["/work/cad/wings/".into()], ..rules() }
    }

    fn with(rules: &GuardRules, tool: &str, input: Value) -> Option<String> {
        decide(rules, &json!({ "tool_name": tool, "tool_input": input, "cwd": "/work" }), &sizes)
    }

    #[test]
    fn writes_outside_the_agents_contract_are_refused_and_its_notes_always_pass() {
        let c = contract();
        let r = with(&c, "Write", json!({ "file_path": "/work/cad/fuselage/frame.step" })).expect("refused");
        assert!(r.starts_with(MARK) && r.contains("frame.step") && r.contains("/work/cad/wings"), "{r}");
        assert!(with(&c, "Edit", json!({ "file_path": "cad/fuselage/x.py" })).is_some(), "relative to cwd");
        assert!(with(&c, "NotebookEdit", json!({ "notebook_path": "/work/other.ipynb" })).is_some());
        assert_eq!(with(&c, "Write", json!({ "file_path": "/work/cad/wings/spar.step" })), None);
        assert_eq!(with(&c, "Write", json!({ "file_path": "/work/.sculptured/notes/wings.md" })), None, "its notes");
        assert_eq!(with(&rules(), "Write", json!({ "file_path": "/anywhere/x" })), None, "no contract: no limit");
    }

    #[test]
    fn reads_outside_the_agents_contract_are_refused_but_its_own_writes_can_be_read() {
        let c = contract();
        assert!(with(&c, "Read", json!({ "file_path": "/work/secrets/keys.txt", "limit": 10 })).is_some());
        assert_eq!(with(&c, "Read", json!({ "file_path": "/work/manuals/ch1.txt" })), None);
        assert!(with(&c, "Read", json!({ "file_path": "/work/manualsx/ch1.txt" })).is_some(), "a sibling with the same prefix is outside");
        assert_eq!(with(&c, "Read", json!({ "file_path": "/work/cad/wings/spar.step" })), None);
    }

    #[test]
    fn other_tools_and_odd_input_pass() {
        assert_eq!(call("Write", json!({ "file_path": "/work/big.csv" })), None);
        assert_eq!(decide(&rules(), &json!({}), &sizes), None);
        assert!(!Protection { on: false, ..Protection::default() }.guard_rules().is_some());
    }

    #[test]
    fn a_safety_rule_refuses_first_with_the_protection_mark() {
        let rules = GuardRules { max_read_bytes: 1 << 30, safety: crate::safety::presets(), ..Default::default() };
        let r = decide(&rules, &json!({ "tool_name": "Bash", "tool_input": { "command": "rm -rf target" }, "cwd": "/w" }), &|_| None).expect("refused");
        assert!(r.starts_with(MARK) && r.contains("no-recursive-delete"), "{r}");
        assert!(decide(&rules, &json!({ "tool_name": "Bash", "tool_input": { "command": "cargo test" }, "cwd": "/w" }), &|_| None).is_none());
    }
}
