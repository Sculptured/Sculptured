//! What an agent is given beyond its CLI and Sculptured's tools: its skills, as a plugin folder
//! Claude Code loads for that session only, and components (programs and Claude plugins on this
//! computer, such as Kindex, found here by name; MCP servers at a web address, whose secrets are
//! filled in here). The server never names a path or holds a secret.
use super::Supervisor;
use drivers::{McpServer, RemoteMcp};
use model::{Component, ComponentKind, EventBody, SafetyRule, Skill};
use std::path::{Path, PathBuf};

/// The plugin that carries one agent's skills; Claude names them `sculptured:<skill>`.
const PLUGIN: &str = "sculptured";

impl Supervisor {
    /// Writes the agent's skills as a plugin folder in this runner's own data, replacing an earlier one.
    /// Nothing is written into the person's project.
    pub(super) fn skills_plugin(&self, seat: &str, skills: &[Skill]) -> Result<Option<PathBuf>, String> {
        let Some(host) = &self.inner.seat_tools else { return Ok(None) };
        let root = host.socket.parent().unwrap_or_else(|| Path::new(".")).join("skills");
        let dir = root.join(seat.replace(|c: char| !(c.is_ascii_alphanumeric() || "@.-_".contains(c)), "_"));
        write_plugin(&dir, skills).map_err(|e| format!("cannot prepare the agent's skills: {e}"))?;
        Ok(Some(dir))
    }

    /// The agent's components found on this computer, as MCP servers, tool hooks and plugin folders.
    /// Each one that cannot run here (not approved on a connected computer, its program, plugin or a
    /// secret missing) is told on the agent, and the agent starts without it. `names`: built-in
    /// components named by an older server. `safety`: the team's rules, for a tool hook's `{safety_rules}`.
    pub(super) fn components(&self, seat: &str, manifests: &[Component], names: &[String], safety: &[SafetyRule]) -> Given {
        let builtins = model::component::builtins();
        let mut all: Vec<Component> = manifests.to_vec();
        let named: Vec<Component> = names.iter().filter(|n| !manifests.iter().any(|c| &c.id == *n)).filter_map(|n| builtins.iter().find(|b| &b.id == n).cloned()).collect();
        all.extend(named);
        let mut given = Given::default();
        for c in &all {
            match self.resolve(seat, c, safety) {
                Ok(Resolved::Server(s)) => given.servers.push(s),
                Ok(Resolved::Remote(r)) => given.remote.push(r),
                Ok(Resolved::Hook(h)) => given.hooks.push(h),
                Ok(Resolved::Plugin(p)) => given.plugins.push(p),
                Ok(Resolved::Nothing) => {}
                Err(note) => self.emit_system(seat, EventBody::FeaturesDropped { features: vec![c.id.clone()], note }),
            }
        }
        given
    }

    fn resolve(&self, seat: &str, c: &Component, safety: &[SafetyRule]) -> Result<Resolved, String> {
        if let Some(why) = c.problems().into_iter().next() {
            return Err(format!("This agent runs without {}: {why}", c.name));
        }
        // On a connected computer, only its owner decides what runs: built-in components, or a manifest
        // approved there by its digest.
        if !may_run(self.inner.policy.is_some(), c, &|d| self.approved(d)) {
            return Err(format!(
                "This agent runs without {}: it is not approved on this computer. It would run `{}`. To allow it, run on that computer: sculptured approve-component {}",
                c.name, c.command_line(), c.digest()
            ));
        }
        match c.kind {
            ComponentKind::Skills => return Ok(Resolved::Nothing), // the server adds them to the agent's skills
            ComponentKind::McpRemote => {
                let secrets = self.secrets();
                let lookup = |n: &str| std::env::var(n).ok().filter(|v| !v.is_empty()).or_else(|| secrets.get(n).cloned());
                let missing = |n: String| format!("This agent runs without {}: the secret {n} is not set on its computer. Set it there with: sculptured set-secret {n} (then restart the agent). {}", c.name, c.install);
                let url = model::component::fill(c.url.as_deref().unwrap_or_default(), &lookup).map_err(missing)?;
                let headers = c.headers.iter().map(|(k, v)| model::component::fill(v, &lookup).map(|v| (k.clone(), v))).collect::<Result<Vec<_>, _>>().map_err(missing)?;
                return Ok(Resolved::Remote(RemoteMcp { name: c.id.clone(), url, headers }));
            }
            ComponentKind::ClaudePlugin => {
                let plugin = c.plugin.as_deref().unwrap_or_default();
                return installed_plugin(&claude_plugins_root(), plugin).map(Resolved::Plugin).ok_or_else(|| {
                    format!("This agent runs without {}: the Claude plugin {plugin} is not installed on its computer. Install it there with {}, then restart the agent.", c.name, c.install)
                });
            }
            ComponentKind::Mcp | ComponentKind::ToolHook => {}
        }
        let program = find_program(&c.program).ok_or_else(|| format!("This agent runs without {}: `{}` is not installed on its computer. Install it there with {}, then restart the agent.", c.name, c.program, c.install))?;
        let mut args = c.args.clone();
        if args.iter().any(|a| a.contains(model::component::SAFETY_RULES)) {
            let file = self.safety_rules_file(seat, &c.id, safety).ok_or_else(|| format!("This agent runs without {}: its rules file could not be written.", c.name))?;
            for a in &mut args {
                *a = a.replace(model::component::SAFETY_RULES, &file.display().to_string());
            }
        }
        Ok(match c.kind {
            ComponentKind::Mcp => Resolved::Server(McpServer { name: c.id.clone(), program, args }),
            ComponentKind::ToolHook => Resolved::Hook(std::iter::once(&program.display().to_string()).chain(args.iter()).map(|a| format!("\"{}\"", a.replace('"', "\\\""))).collect::<Vec<_>>().join(" ")),
            ComponentKind::McpRemote | ComponentKind::Skills | ComponentKind::ClaudePlugin => unreachable!("handled above"),
        })
    }

    /// Manifests this computer's owner approved (`sculptured approve-component`), read at each launch so
    /// an approval needs no restart.
    fn approved(&self, digest: &str) -> bool {
        let Some(host) = &self.inner.seat_tools else { return false };
        let file = host.socket.parent().unwrap_or_else(|| Path::new(".")).join(APPROVED_FILE);
        std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).is_some_and(|l| l.iter().any(|d| d == digest))
    }

    /// Secrets this computer's owner set for components (`sculptured set-secret`), read at each launch.
    fn secrets(&self) -> std::collections::BTreeMap<String, String> {
        let Some(host) = &self.inner.seat_tools else { return Default::default() };
        let file = host.socket.parent().unwrap_or_else(|| Path::new(".")).join(SECRETS_FILE);
        std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    /// The agent's briefing in this runner's data, readable by this user only, for `sculptured rebrief`.
    pub(super) fn briefing_file(&self, seat: &str, text: &str) -> Option<PathBuf> {
        let host = self.inner.seat_tools.as_ref()?;
        let dir = host.socket.parent().unwrap_or_else(|| Path::new(".")).join("briefings");
        std::fs::create_dir_all(&dir).ok()?;
        let file = dir.join(format!("{}.md", seat.replace(|c: char| !(c.is_ascii_alphanumeric() || "@.-_".contains(c)), "_")));
        crate::private::write_file(&file, text.as_bytes()).ok()?;
        Some(file)
    }

    /// Where a Claude agent reads its remote servers (with their secrets), readable by this user only.
    pub(super) fn remote_mcp_file(&self, seat: &str) -> Option<PathBuf> {
        let host = self.inner.seat_tools.as_ref()?;
        let dir = host.socket.parent().unwrap_or_else(|| Path::new(".")).join("components").join("remote");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir.join(format!("{}.json", seat.replace(|c: char| !(c.is_ascii_alphanumeric() || "@.-_".contains(c)), "_"))))
    }

    /// The team's safety rules in Signet-Eval's format (YAML reads JSON), for a tool hook.
    fn safety_rules_file(&self, seat: &str, component: &str, safety: &[SafetyRule]) -> Option<PathBuf> {
        let host = self.inner.seat_tools.as_ref()?;
        let dir = host.socket.parent().unwrap_or_else(|| Path::new(".")).join("components").join(component).join(seat.replace(|c: char| !(c.is_ascii_alphanumeric() || "@.-_".contains(c)), "_"));
        let rules: Vec<serde_json::Value> = safety.iter().map(|r| serde_json::json!({
            "name": r.name, "tool_pattern": r.tool_pattern, "conditions": r.conditions, "action": "DENY",
            "reason": r.reason.clone().unwrap_or_else(|| "Not allowed in this team.".into()),
        })).collect();
        let file = dir.join("rules.yaml");
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, serde_json::to_string_pretty(&rules).unwrap_or_default())).ok()?;
        Some(file)
    }
}

/// A component may run when it is built in, on this computer's own local server, or approved here
/// by its owner (a connected computer, where the server is someone else's).
fn may_run(connected: bool, c: &Component, approved: &dyn Fn(&str) -> bool) -> bool {
    !connected || c.is_builtin() || approved(&c.digest())
}

/// The owner's approved component digests, in the runner's data folder.
pub const APPROVED_FILE: &str = "approved-components.json";
/// The owner's component secrets by name, in the runner's data folder (this user only).
pub const SECRETS_FILE: &str = "component-secrets.json";

/// What an agent's components give it on this computer.
#[derive(Default)]
pub(super) struct Given {
    pub servers: Vec<McpServer>,
    pub remote: Vec<RemoteMcp>,
    pub hooks: Vec<String>,
    pub plugins: Vec<PathBuf>,
}

enum Resolved {
    Server(McpServer),
    Remote(RemoteMcp),
    Hook(String),
    Plugin(PathBuf),
    Nothing,
}

/// Claude Code's plugins folder for this user.
fn claude_plugins_root() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".claude")).join("plugins")
}

/// The folder of plugin `name@marketplace`, as `claude plugin install` recorded it in
/// `installed_plugins.json` (its latest install), when that folder exists.
fn installed_plugin(root: &Path, plugin: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join("installed_plugins.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let installs = v.get("plugins")?.get(plugin)?.as_array()?;
    installs.iter().rev().filter_map(|i| i.get("installPath")?.as_str()).map(PathBuf::from).find(|p| p.is_dir())
}

pub(crate) fn write_plugin(dir: &Path, skills: &[Skill]) -> std::io::Result<()> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir.join(".claude-plugin"))?;
    let manifest = serde_json::json!({ "name": PLUGIN, "version": "1.0.0", "description": "This agent's skills, from its Sculptured team" });
    std::fs::write(dir.join(".claude-plugin/plugin.json"), manifest.to_string())?;
    for s in skills {
        // Names were checked by the server; a name that could leave the folder is skipped anyway.
        if s.name.is_empty() || !s.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            continue;
        }
        let skill = dir.join("skills").join(&s.name);
        std::fs::create_dir_all(&skill)?;
        std::fs::write(skill.join("SKILL.md"), s.to_skill_md())?;
    }
    Ok(())
}

/// A program on PATH, or in the usual per-user install folders (login services start with a short PATH).
pub fn find_program(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(h) = home {
        dirs.extend([h.join(".local/bin"), h.join(".cargo/bin")]);
    }
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_connected_computer_runs_only_built_in_or_approved_components() {
        let builtin = model::component::builtins()[0].clone();
        let mut other = builtin.clone();
        other.id = "notes".into();
        let none = |_: &str| false;
        assert!(may_run(true, &builtin, &none), "built-in components need no approval");
        assert!(!may_run(true, &other, &none), "another manifest needs its owner's approval");
        let digest = other.digest();
        assert!(may_run(true, &other, &|d: &str| d == digest));
        other.args.push("--changed".into());
        assert!(!may_run(true, &other, &|d: &str| d == digest), "a changed manifest needs approving again");
        assert!(may_run(false, &other, &none), "on its own local server the person's own team decides");
    }

    #[test]
    fn an_installed_plugin_is_found_by_name_and_a_missing_one_is_not() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("cache/hpc/openfoam/1.2.0");
        std::fs::create_dir_all(&folder).unwrap();
        let list = serde_json::json!({ "version": 2, "plugins": {
            "openfoam@hpc": [{ "scope": "user", "installPath": folder }],
            "gone@hpc": [{ "scope": "user", "installPath": root.path().join("cache/hpc/gone/1.0.0") }],
        } });
        std::fs::write(root.path().join("installed_plugins.json"), list.to_string()).unwrap();
        assert_eq!(installed_plugin(root.path(), "openfoam@hpc"), Some(folder));
        assert_eq!(installed_plugin(root.path(), "gone@hpc"), None, "its folder was removed");
        assert_eq!(installed_plugin(root.path(), "other@hpc"), None);
    }

    #[test]
    fn an_agents_skills_become_a_plugin_folder_and_an_unsafe_name_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("p");
        let good = Skill { name: "money-rules".into(), description: "Use for money.".into(), body: "Decimal only.\n".into() };
        let bad = Skill { name: "../escape".into(), description: "x".into(), body: "x".into() };
        write_plugin(&plugin, &[good.clone(), bad]).unwrap();
        let md = std::fs::read_to_string(plugin.join("skills/money-rules/SKILL.md")).unwrap();
        assert_eq!(Skill::from_skill_md(&md).unwrap(), good);
        assert!(plugin.join(".claude-plugin/plugin.json").is_file());
        assert_eq!(std::fs::read_dir(plugin.join("skills")).unwrap().count(), 1, "the unsafe name is not written");
        write_plugin(&plugin, &[]).unwrap();
        assert!(!plugin.join("skills/money-rules").exists(), "a later launch replaces the folder");
    }
}
