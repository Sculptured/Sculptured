//! Components: programs on the agent's computer that an agent is also given, described by a
//! manifest rather than written into Sculptured. Kindex and Signet-Eval ship as built-in examples;
//! anyone can write one, share it through Community, and add it to a team. The runner finds the
//! program on its own computer, and a connected computer runs a manifest that is not built in only
//! after its owner approved that exact manifest there (`sculptured approve-component <digest>`).
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

/// Replaced in a tool hook's arguments by the path of the team's safety rules (Signet-Eval's format).
pub const SAFETY_RULES: &str = "{safety_rules}";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ComponentKind {
    /// An MCP server over stdio: its tools sit next to Sculptured's.
    Mcp,
    /// A Claude Code PreToolUse hook run on every tool call, after Sculptured's own guard.
    ToolHook,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[ts(export)]
pub struct Component {
    /// Lowercase letters, digits and hyphens.
    pub id: String,
    pub name: String,
    /// What it gives an agent, in a sentence or two.
    pub description: String,
    pub kind: ComponentKind,
    /// A program name (no path) the runner looks for on the agent's computer.
    pub program: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub args: Vec<String>,
    /// What agents given it are told about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub briefing: Option<String>,
    /// How to install the program on a computer, shown when it is missing.
    pub install: String,
    /// Claude Code agents only (every tool hook is).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub claude_only: bool,
    /// Other components it needs, by id; they come with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub homepage: Option<String>,
}

fn kebab(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !s.starts_with('-') && !s.ends_with('-')
}

impl Component {
    /// What is wrong with it, in plain words; empty when it can be used.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let who = if self.name.trim().is_empty() { self.id.clone() } else { self.name.clone() };
        if !kebab(&self.id, 40) {
            out.push(format!("Component id {:?} must be 1–40 lowercase letters, digits and hyphens.", self.id));
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 60 {
            out.push(format!("Component {:?} needs a name of at most 60 characters.", self.id));
        }
        if self.description.chars().count() > 500 || self.install.trim().is_empty() || self.install.chars().count() > 500 {
            out.push(format!("{who}: the description and the install steps are at most 500 characters, and install steps are required."));
        }
        let bare = !self.program.is_empty() && self.program.len() <= 80 && self.program.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)) && !self.program.starts_with('.');
        if !bare {
            out.push(format!("{who}: the program must be a plain program name such as kin-mcp, with no folder: the runner finds it on its own computer."));
        }
        if self.args.len() > 32 || self.args.iter().any(|a| a.len() > 500 || a.contains('\0')) {
            out.push(format!("{who}: at most 32 arguments of at most 500 characters each."));
        }
        if self.kind == ComponentKind::ToolHook && !self.claude_only {
            out.push(format!("{who}: a tool hook runs only for Claude Code agents; set claude_only."));
        }
        if self.kind == ComponentKind::Mcp && self.args.iter().any(|a| a.contains(SAFETY_RULES)) {
            out.push(format!("{who}: {SAFETY_RULES} is only for tool hooks."));
        }
        if self.briefing.as_ref().is_some_and(|b| b.chars().count() > 1000) {
            out.push(format!("{who}: the briefing is at most 1000 characters."));
        }
        if self.homepage.as_ref().is_some_and(|h| !(h.starts_with("https://") || h.starts_with("http://"))) {
            out.push(format!("{who}: the homepage must be a web address."));
        }
        if self.requires.iter().any(|r| !kebab(r, 40) || r == &self.id) {
            out.push(format!("{who}: requires names other components by id."));
        }
        out
    }

    /// The SHA-256 of the manifest as Sculptured writes it: what an owner approves on a computer.
    pub fn digest(&self) -> String {
        let canon = serde_json::to_string(self).expect("a component serializes");
        Sha256::digest(canon.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The command line it runs, for people to read before they approve it.
    pub fn command_line(&self) -> String {
        std::iter::once(self.program.as_str()).chain(self.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
    }

    /// True when it is exactly one of Sculptured's built-in components.
    pub fn is_builtin(&self) -> bool {
        builtins().iter().any(|b| b == self)
    }
}

/// The components that ship with Sculptured, as examples of the format (parsed once).
pub fn builtins() -> Vec<Component> {
    static BUILTINS: std::sync::LazyLock<Vec<Component>> = std::sync::LazyLock::new(|| {
        [include_str!("components/kindex.json"), include_str!("components/signet.json")]
            .iter()
            .map(|j| serde_json::from_str(j).expect("a built-in component is valid"))
            .collect()
    });
    BUILTINS.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtins_are_valid_and_a_bad_manifest_is_named() {
        for c in builtins() {
            assert!(c.problems().is_empty(), "{}: {:?}", c.id, c.problems());
            assert!(c.is_builtin());
        }
        let mut c = builtins()[0].clone();
        c.program = "/usr/bin/kin-mcp".into();
        assert!(c.problems()[0].contains("plain program name"), "{:?}", c.problems());
        assert!(!c.is_builtin(), "a changed manifest is not the built-in one");
        assert_ne!(c.digest(), builtins()[0].digest());
        assert_eq!(builtins()[1].command_line(), "signet-eval --rules-path {safety_rules}");
    }

    #[test]
    fn the_web_apps_copies_of_the_builtins_are_the_same() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/structure/components");
        for c in builtins() {
            let Ok(text) = std::fs::read_to_string(dir.join(format!("{}.json", c.id))) else { return }; // a build without the web app
            assert_eq!(serde_json::from_str::<Component>(&text).unwrap(), c, "copy crates/model/src/components/{}.json to the web app", c.id);
        }
    }
}
