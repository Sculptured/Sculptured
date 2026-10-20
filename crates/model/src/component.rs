//! Components: what an agent is also given beyond Sculptured's tools, described by a manifest rather
//! than written into Sculptured: an MCP server on its computer or at a web address, a Claude Code tool
//! hook or plugin, or a pack of skills. Kindex and Signet-Eval ship as built-in examples; anyone can
//! write one, share it through Community, and add it to a team. The runner finds programs and plugins
//! on its own computer and fills in a remote server's secrets there, so the server never holds them.
//! A connected computer runs a manifest that is not built in only after its owner approved that exact
//! manifest there (`sculptured approve-component <digest>`).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    /// An MCP server at a web address (`url`); `${NAME}` in it and its headers is a secret the runner
    /// fills in on the agent's computer.
    McpRemote,
    /// Skills (`skills`), added to the agent's own; nothing runs.
    Skills,
    /// A Claude Code plugin installed on the agent's computer (`plugin`, `name@marketplace`), loaded
    /// for that agent only.
    ClaudePlugin,
}

impl ComponentKind {
    /// Kinds a runner understands only from `features::COMPONENT_KINDS` on.
    pub fn is_newer(self) -> bool {
        matches!(self, ComponentKind::McpRemote | ComponentKind::ClaudePlugin)
    }
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
    /// `mcp` and `tool-hook`: a program name (no path) the runner looks for on the agent's computer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub program: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub args: Vec<String>,
    /// What agents given it are told about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub briefing: Option<String>,
    /// How to set it up on a computer (install the program or plugin, get the secret), shown when it
    /// is missing. Not needed for skills.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
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
    /// `mcp-remote`: the server's address, `https://` (or `http://` on this computer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub url: Option<String>,
    /// `mcp-remote`: request headers; each value names its secret as `${NAME}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(as = "Option<BTreeMap<String, String>>", optional)]
    pub headers: BTreeMap<String, String>,
    /// `skills`: the skills it adds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<crate::Skill>>", optional)]
    pub skills: Vec<crate::Skill>,
    /// `claude-plugin`: the installed plugin, as `name@marketplace`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plugin: Option<String>,
}

/// The secrets named in `text` as `${NAME}` (uppercase letters, digits and underscores), in order;
/// `Err` names a malformed one.
pub fn placeholders(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("${") {
        let after = &rest[i + 2..];
        let end = after.find('}').ok_or_else(|| format!("{text:?} opens ${{ without closing it."))?;
        let name = &after[..end];
        if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') || name.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(format!("${{{name}}} must name a secret in capitals, such as ${{OPENFOAM_TOKEN}}."));
        }
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &after[end + 1..];
    }
    Ok(out)
}

/// Fills each `${NAME}` in `text` from `lookup`; `Err` is the first secret it lacks.
pub fn fill(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = text.to_string();
    for name in placeholders(text)? {
        let value = lookup(&name).ok_or(name.clone())?;
        out = out.replace(&format!("${{{name}}}"), &value);
    }
    Ok(out)
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
        if self.description.chars().count() > 500 || self.install.chars().count() > 500 {
            out.push(format!("{who}: the description and the install steps are at most 500 characters."));
        }
        let runs_program = matches!(self.kind, ComponentKind::Mcp | ComponentKind::ToolHook);
        let bare = !self.program.is_empty() && self.program.len() <= 80 && self.program.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)) && !self.program.starts_with('.');
        if runs_program && !bare {
            out.push(format!("{who}: the program must be a plain program name such as kin-mcp, with no folder: the runner finds it on its own computer."));
        }
        if !runs_program && (!self.program.is_empty() || !self.args.is_empty()) {
            out.push(format!("{who}: only an MCP server on the computer or a tool hook runs a program."));
        }
        if self.kind != ComponentKind::Skills && self.install.trim().is_empty() {
            out.push(format!("{who}: say how to set it up on a computer (the install steps)."));
        }
        if self.args.len() > 32 || self.args.iter().any(|a| a.len() > 500 || a.contains('\0')) {
            out.push(format!("{who}: at most 32 arguments of at most 500 characters each."));
        }
        if matches!(self.kind, ComponentKind::ToolHook | ComponentKind::ClaudePlugin) && !self.claude_only {
            out.push(format!("{who}: tool hooks and Claude plugins work only for Claude Code agents; set claude_only."));
        }
        out.extend(self.remote_problems(&who));
        if (self.kind == ComponentKind::Skills) != !self.skills.is_empty() {
            out.push(format!("{who}: a skills component carries one or more skills, and only it does."));
        }
        if self.skills.len() > 20 {
            out.push(format!("{who}: at most 20 skills in one component."));
        }
        out.extend(crate::skill::skills_problems(&self.skills).into_iter().map(|p| format!("{who}: {p}")));
        let plugin_ok = self.plugin.as_ref().is_some_and(|p| {
            p.len() <= 120 && p.split_once('@').is_some_and(|(n, m)| [n, m].iter().all(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) && !s.starts_with('.')))
        });
        if (self.kind == ComponentKind::ClaudePlugin) != self.plugin.is_some() || (self.plugin.is_some() && !plugin_ok) {
            out.push(format!("{who}: a Claude plugin component names its plugin as name@marketplace, as `claude plugin install` takes it, and only it does."));
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

    fn remote_problems(&self, who: &str) -> Vec<String> {
        let mut out = Vec::new();
        if (self.kind == ComponentKind::McpRemote) != self.url.is_some() || (self.kind != ComponentKind::McpRemote && !self.headers.is_empty()) {
            out.push(format!("{who}: a remote MCP server has a url (and headers), and only it does."));
        }
        let Some(url) = &self.url else { return out };
        let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"].iter().any(|p| url.strip_prefix(p).is_some_and(|r| r.is_empty() || r.starts_with([':', '/'])));
        if url.len() > 500 || !(url.starts_with("https://") || local) || url.chars().any(char::is_whitespace) {
            out.push(format!("{who}: the url must start with https:// (http:// only for a server on the agent's own computer)."));
        }
        if let Err(e) = placeholders(url) {
            out.push(format!("{who}: {e}"));
        }
        if self.headers.len() > 16 {
            out.push(format!("{who}: at most 16 headers."));
        }
        for (name, value) in &self.headers {
            if name.is_empty() || name.len() > 64 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                out.push(format!("{who}: header {name:?} must be letters, digits and hyphens."));
            }
            match placeholders(value) {
                Err(e) => out.push(format!("{who}: {e}")),
                Ok(names) if names.is_empty() => out.push(format!(
                    "{who}: header {name} must name its secret as ${{NAME}} (for example \"Bearer ${{API_TOKEN}}\"), so the secret stays on the agent's computer and out of the manifest."
                )),
                Ok(_) if value.len() > 500 || value.contains(['\r', '\n']) => out.push(format!("{who}: header {name} is too long or spans lines.")),
                Ok(_) => {}
            }
        }
        out
    }

    /// The secrets the runner fills in on the agent's computer, by name.
    pub fn secrets(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for text in self.url.iter().chain(self.headers.values()) {
            for n in placeholders(text).unwrap_or_default() {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// The SHA-256 of the manifest as Sculptured writes it: what an owner approves on a computer.
    pub fn digest(&self) -> String {
        let canon = serde_json::to_string(self).expect("a component serializes");
        Sha256::digest(canon.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// What it runs or connects to, for people to read before they approve it.
    pub fn command_line(&self) -> String {
        match self.kind {
            ComponentKind::Mcp | ComponentKind::ToolHook => std::iter::once(self.program.as_str()).chain(self.args.iter().map(String::as_str)).collect::<Vec<_>>().join(" "),
            ComponentKind::McpRemote => {
                let headers: Vec<String> = self.headers.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                let url = self.url.as_deref().unwrap_or_default();
                if headers.is_empty() { format!("connects to {url}") } else { format!("connects to {url} with {}", headers.join(", ")) }
            }
            ComponentKind::Skills => format!("adds skills {}", self.skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")),
            ComponentKind::ClaudePlugin => format!("loads the Claude plugin {}", self.plugin.as_deref().unwrap_or_default()),
        }
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

    fn base(kind: ComponentKind) -> Component {
        let mut c = builtins()[0].clone();
        (c.id, c.name, c.kind, c.program, c.args) = ("x".into(), "X".into(), kind, String::new(), vec![]);
        c
    }

    #[test]
    fn a_remote_server_keeps_its_secrets_as_names_and_fills_them_on_the_computer() {
        let mut c = base(ComponentKind::McpRemote);
        c.url = Some("https://mcp.example.com/${TEAM}/mcp".into());
        c.headers.insert("Authorization".into(), "Bearer ${OPENFOAM_TOKEN}".into());
        assert!(c.problems().is_empty(), "{:?}", c.problems());
        assert_eq!(c.secrets(), ["TEAM", "OPENFOAM_TOKEN"]);
        assert_eq!(c.command_line(), "connects to https://mcp.example.com/${TEAM}/mcp with Authorization: Bearer ${OPENFOAM_TOKEN}");
        let lookup = |n: &str| (n == "OPENFOAM_TOKEN").then(|| "s3cret".to_string());
        assert_eq!(fill("Bearer ${OPENFOAM_TOKEN}", &lookup).unwrap(), "Bearer s3cret");
        assert_eq!(fill("${TEAM}", &lookup).unwrap_err(), "TEAM", "a missing secret is named");

        c.headers.insert("X-Key".into(), "literal-secret".into());
        assert!(c.problems().iter().any(|p| p.contains("must name its secret")), "a written-in secret is refused: {:?}", c.problems());
        c.headers.remove("X-Key");
        c.url = Some("http://mcp.example.com/mcp".into());
        assert!(c.problems().iter().any(|p| p.contains("https://")), "plain http to another computer is refused");
        c.url = Some("http://localhost:8080/mcp".into());
        assert!(c.problems().is_empty(), "{:?}", c.problems());
        c.url = Some("http://localhost.evil.example/mcp".into());
        assert!(!c.problems().is_empty(), "a look-alike host is not this computer");
        c.url = Some("https://x/${lower}".into());
        assert!(!c.problems().is_empty());
    }

    #[test]
    fn skills_and_plugins_are_checked_by_kind_and_existing_digests_do_not_change() {
        let mut s = base(ComponentKind::Skills);
        s.install = String::new();
        assert!(!s.problems().is_empty(), "a skills component needs skills");
        s.skills = vec![crate::Skill { name: "mesh-check".into(), description: "Use before a run.".into(), body: "Check the mesh.\n".into(), ..Default::default() }];
        assert!(s.problems().is_empty(), "{:?}", s.problems());
        assert_eq!(s.command_line(), "adds skills mesh-check");
        s.program = "kin-mcp".into();
        assert!(!s.problems().is_empty(), "a skills component runs nothing");

        let mut p = base(ComponentKind::ClaudePlugin);
        p.plugin = Some("openfoam@hpc-tools".into());
        assert!(!p.problems().is_empty(), "a plugin is Claude-only");
        p.claude_only = true;
        assert!(p.problems().is_empty(), "{:?}", p.problems());
        p.plugin = Some("../x@y".into());
        assert!(!p.problems().is_empty());

        // Approvals already given stay valid: the new fields are left out when unused.
        for (c, file) in builtins().iter().zip([include_str!("components/kindex.json"), include_str!("components/signet.json")]) {
            let keys = |v: serde_json::Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
            assert_eq!(keys(serde_json::to_value(c).unwrap()), keys(serde_json::from_str(file).unwrap()), "{}", c.id);
        }
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
