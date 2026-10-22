//! Claude launch arguments and MCP configuration.
use super::ClaudeStreamDriver;
use crate::LaunchSpec;
use model::PermissionPosture;
use serde_json::{Value, json};

impl ClaudeStreamDriver {
    /// Unrestricted posture by default (spec D7). A restricted agent drops the bypass flag; with seat
    /// tools it routes permission prompts to `approve` (M0: the gate runs through an MCP tool).
    pub fn args(spec: &LaunchSpec, resume: Option<&str>, session_id: &str) -> Vec<String> {
        let mut a = Self::stream_args();
        if spec.posture == PermissionPosture::Unrestricted {
            a.push("--dangerously-skip-permissions".into());
        }
        match resume {
            Some(id) => a.extend(["--resume".to_string(), id.to_string()]),
            None => a.extend(["--session-id".to_string(), session_id.to_string()]),
        }
        if let Some(m) = &spec.model {
            a.extend(["--model".to_string(), m.clone()]);
        }
        Self::seat_tools_args(spec, &mut a);
        Self::integration_args(spec, &mut a);
        a.extend(
            spec.claude_setup
                .iter()
                .flat_map(model::ClaudeSetup::claude_args)
                .chain(Self::protection_args(
                    spec.protection.as_ref(),
                    &spec.tool_hooks,
                )),
        );
        a.extend(spec.extra_args.iter().cloned());
        a
    }

    fn stream_args() -> Vec<String> {
        [
            "-p",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--replay-user-messages",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn seat_tools_args(spec: &LaunchSpec, a: &mut Vec<String>) {
        if let Some(t) = &spec.seat_tools {
            // A file keeps the token off argv (`/proc/<pid>/cmdline` is world-readable); `spawn` writes it.
            let config = t
                .config_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| Self::mcp_config(t));
            a.extend(["--mcp-config".to_string(), config]);
            a.extend(["--disallowedTools".to_string(), "Agent,Task".to_string()]);
            if spec.posture == PermissionPosture::Restricted {
                // Sculptured's own tools never need a permission prompt: without this, every
                // request_input / record_decision / report call of a restricted agent would first
                // raise an `approve` question ("Allow mcp__rig__request_input to run?").
                a.extend([
                    "--allowedTools".to_string(),
                    format!("mcp__{0}__request_input,mcp__{0}__record_decision,mcp__{0}__report,mcp__{0}__dispatch_work,mcp__{0}__complete_work,mcp__{0}__workflow_status,mcp__{0}__spawn_agent,mcp__{0}__send_agent_message,mcp__{0}__team_status,mcp__{0}__read_decisions", crate::MCP_SERVER),
                ]);
            }
            if spec.posture == PermissionPosture::Restricted && t.approve {
                a.extend([
                    "--permission-prompt-tool".to_string(),
                    crate::APPROVE_TOOL.to_string(),
                ]);
            }
        }
    }

    fn integration_args(spec: &LaunchSpec, a: &mut Vec<String>) {
        for dir in &spec.add_dirs {
            a.extend(["--add-dir".to_string(), dir.display().to_string()]);
        }
        for dir in &spec.plugin_dirs {
            a.extend(["--plugin-dir".to_string(), dir.display().to_string()]);
        }
        if !spec.mcp_servers.is_empty() {
            let servers: serde_json::Map<String, Value> = spec.mcp_servers.iter()
                .map(|s| (s.name.clone(), json!({ "type": "stdio", "command": s.program.display().to_string(), "args": s.args })))
                .collect();
            a.extend([
                "--mcp-config".to_string(),
                json!({ "mcpServers": servers }).to_string(),
            ]);
        }
        if !spec.remote_mcp.is_empty() {
            let config = spec
                .remote_mcp_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| Self::remote_mcp_config(&spec.remote_mcp));
            a.extend(["--mcp-config".to_string(), config]);
        }
    }

    /// Protections: a PreToolUse hook running `guard`, the shell output cap, the briefing given back
    /// after a compaction (`rebrief`), and Claude Code's lowest compaction threshold as a backstop within long turns (Sculptured's own context ceiling acts
    /// between turns). Passed with `--settings`, so they apply whatever setting sources load.
    /// More tool hooks (Signet-Eval) run on every call, after the guard.
    fn protection_args(p: Option<&crate::ProtectionLaunch>, more: &[String]) -> Vec<String> {
        let mut pre: Vec<Value> = p.iter().map(|p| json!({ "matcher": p.rules.hook_matcher(), "hooks": [{ "type": "command", "command": format!("\"{}\" guard", p.guard.display()), "timeout": 5 }] })).collect();
        pre.extend(more.iter().map(|c| json!({ "matcher": "", "hooks": [{ "type": "command", "command": c, "timeout": 5 }] })));
        if pre.is_empty() {
            return vec![];
        }
        let mut settings = json!({ "hooks": { "PreToolUse": pre } });
        let mut out = vec![];
        if let Some(p) = p {
            if let Some(file) = &p.briefing_file {
                // A compaction can summarize the briefing away; give it back as context (an idea from Gas Town's `gt prime`).
                let command = format!("\"{}\" rebrief \"{}\"", p.guard.display(), file.display());
                settings["hooks"]["SessionStart"] = json!([{ "matcher": "compact", "hooks": [{ "type": "command", "command": command, "timeout": 5 }] }]);
            }
            settings["bashOutputMaxChars"] = json!(p.rules.shell_output_chars);
            out.extend(["--autocompact".to_string(), "100000".to_string()]);
        }
        [vec!["--settings".to_string(), settings.to_string()], out].concat()
    }

    /// The guard's rules, in the environment the hook inherits from the agent's process.
    #[must_use]
    pub fn protection_env(spec: &LaunchSpec) -> Option<(&'static str, String)> {
        let p = spec.protection.as_ref()?;
        Some((
            "SCULPTURED_PROTECTION",
            serde_json::to_string(&p.rules).expect("rules serialize"),
        ))
    }

    /// `--mcp-config` JSON for servers at a web address, headers included.
    #[must_use]
    pub fn remote_mcp_config(servers: &[crate::RemoteMcp]) -> String {
        let servers: serde_json::Map<String, Value> = servers.iter()
            .map(|s| (s.name.clone(), json!({ "type": "http", "url": s.url, "headers": s.headers.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<serde_json::Map<_, _>>() })))
            .collect();
        json!({ "mcpServers": servers }).to_string()
    }

    /// `--mcp-config` JSON naming one stdio server, `rig`.
    #[must_use]
    pub fn mcp_config(t: &crate::SeatToolsLaunch) -> String {
        let env: serde_json::Map<String, Value> = t
            .env
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        json!({ "mcpServers": { crate::MCP_SERVER: {
            "type": "stdio", "command": t.program.display().to_string(), "args": t.args, "env": env,
        } } })
        .to_string()
    }
}
