//! What OpenCode is started with: its config (`OPENCODE_CONFIG_CONTENT`, merged over the person's own
//! OpenCode setup) and the MCP servers named in `session/new`.
use crate::{LaunchSpec, MCP_SERVER, MCP_TOOL_TIMEOUT_MS};
use model::PermissionPosture;
use serde_json::{Value, json};

/// The config Sculptured adds for every OpenCode agent.
///
/// - `small_model` is the agent's own model: otherwise OpenCode names each session with another model
///   (opencode-basic: a call to a model nobody chose, on the person's key).
/// - `experimental.mcp_timeout` lets a team tool wait for the operator's answer: without it MCP calls
///   end after the MCP SDK's 60 seconds.
/// - No automatic updates and no session sharing.
#[must_use]
pub fn config(spec: &LaunchSpec) -> Value {
    let timeout: u64 = MCP_TOOL_TIMEOUT_MS.parse().expect("a number");
    let mut c = json!({
        "autoupdate": false,
        "share": "disabled",
        "permission": permission(spec.posture, spec.sandboxed),
        "experimental": { "mcp_timeout": timeout },
    });
    if let Some(m) = &spec.model {
        c["small_model"] = json!(m);
    }
    c
}

/// Unrestricted: everything allowed. Restricted: changing files, running commands and the web ask first
/// (a question to the operator). A sandboxed computer keeps the agent inside its folder.
fn permission(posture: PermissionPosture, sandboxed: bool) -> Value {
    let mut p = match posture {
        PermissionPosture::Unrestricted => json!({ "*": "allow" }),
        PermissionPosture::Restricted => json!({
            "edit": "ask", "bash": "ask", "webfetch": "ask", "websearch": "ask", "external_directory": "ask",
        }),
    };
    if sandboxed {
        p["external_directory"] = json!("deny");
    }
    p
}

/// The environment OpenCode runs with, besides the agent's own (a model connection's key).
#[must_use]
pub fn env(spec: &LaunchSpec) -> Vec<(String, String)> {
    vec![
        ("OPENCODE_CONFIG_CONTENT".into(), config(spec).to_string()),
        ("OPENCODE_DISABLE_AUTOUPDATE".into(), "1".into()),
    ]
}

/// `session/new` and `session/resume`'s `mcpServers`: the team tools, the agent's integrations and its
/// MCP servers at a web address. They travel over stdin, so the seat token and headers never reach argv.
#[must_use]
pub fn mcp_servers(spec: &LaunchSpec) -> Vec<Value> {
    let pairs = |env: &[(String, String)]| -> Vec<Value> {
        env.iter()
            .map(|(k, v)| json!({ "name": k, "value": v }))
            .collect()
    };
    let seat = spec.seat_tools.iter().map(|t| {
        json!({ "name": MCP_SERVER, "command": t.program, "args": t.args, "env": pairs(&t.env) })
    });
    let local = spec
        .mcp_servers
        .iter()
        .map(|m| json!({ "name": m.name, "command": m.program, "args": m.args, "env": [] }));
    let remote = spec.remote_mcp.iter().map(
        |r| json!({ "type": "http", "name": r.name, "url": r.url, "headers": pairs(&r.headers) }),
    );
    seat.chain(local).chain(remote).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> LaunchSpec {
        LaunchSpec::new("dev.a@r".parse().unwrap(), "/w".into())
    }

    #[test]
    fn an_unrestricted_agent_is_allowed_everything_and_titles_use_its_own_model() {
        let mut s = spec();
        s.model = Some("openrouter/deepseek/deepseek-chat-v3.1".into());
        let c = config(&s);
        assert_eq!(c["permission"], json!({ "*": "allow" }));
        assert_eq!(c["small_model"], "openrouter/deepseek/deepseek-chat-v3.1");
        assert_eq!(c["autoupdate"], false);
        assert_eq!(c["share"], "disabled");
        assert_eq!(c["experimental"]["mcp_timeout"], 86_700_000);
    }

    #[test]
    fn a_restricted_agent_asks_before_changing_files_or_running_commands() {
        let mut s = spec();
        s.posture = PermissionPosture::Restricted;
        let p = &config(&s)["permission"];
        for tool in ["edit", "bash", "webfetch"] {
            assert_eq!(p[tool], "ask", "{tool}");
        }
        assert!(
            config(&spec()).get("small_model").is_none(),
            "no model: OpenCode's own default"
        );
    }

    #[test]
    fn a_sandboxed_agent_stays_in_its_folder() {
        let mut s = spec();
        s.sandboxed = true;
        assert_eq!(config(&s)["permission"]["external_directory"], "deny");
    }

    #[test]
    fn team_tools_integrations_and_web_servers_are_named_in_the_session() {
        let mut s = spec();
        s.seat_tools = Some(crate::SeatToolsLaunch {
            program: "/bin/sculptured".into(),
            args: vec!["seat-tools".into()],
            env: vec![("SCULPTURED_SEAT_TOKEN".to_string(), "t0k".to_string())].into(),
            approve: false,
            config_file: None,
        });
        s.mcp_servers = vec![crate::McpServer {
            name: "kindex".into(),
            program: "/bin/kin".into(),
            args: vec![],
        }];
        s.remote_mcp = vec![crate::RemoteMcp {
            name: "docs".into(),
            url: "https://mcp.example.com/mcp".into(),
            headers: vec![("Authorization".into(), "Bearer s3cret".into())],
        }];
        let m = mcp_servers(&s);
        assert_eq!(m[0]["name"], "rig");
        assert_eq!(
            m[0]["env"],
            json!([{ "name": "SCULPTURED_SEAT_TOKEN", "value": "t0k" }])
        );
        assert_eq!(m[1]["command"], "/bin/kin");
        assert_eq!(m[2]["type"], "http");
        assert_eq!(m[2]["headers"][0]["value"], "Bearer s3cret");
    }
}
