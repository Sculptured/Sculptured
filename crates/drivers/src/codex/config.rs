//! Codex launch arguments and thread configuration.
use super::CodexAppServerDriver;
use crate::{LaunchSpec, SeatToolsLaunch};
use model::PermissionPosture;
use serde_json::{Map, Value, json};

impl CodexAppServerDriver {
    #[must_use]
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["app-server".to_string()];
        if self.native_questions {
            a.extend([
                "--enable".to_string(),
                "default_mode_request_user_input".to_string(),
            ]);
        }
        a
    }

    /// `thread/start|resume|fork` parameters. D7 (spec §6.2): unrestricted means approvals off and
    /// no sandbox; restricted means `untrusted`, the policy M0 saw ask before a file write.
    #[must_use]
    pub fn thread_params(spec: &LaunchSpec) -> Value {
        let approval = match spec.posture {
            PermissionPosture::Unrestricted => "never",
            PermissionPosture::Restricted => "untrusted",
        };
        // A sandboxed computer (its owner's choice) lets Codex write only inside the folder.
        let sandbox = if spec.sandboxed {
            "workspace-write"
        } else {
            "danger-full-access"
        };
        let mut p = json!({ "cwd": spec.cwd.display().to_string(), "approvalPolicy": approval, "sandbox": sandbox });
        if let Some(m) = &spec.model {
            p["model"] = json!(m);
        }
        let config = Self::thread_config(spec);
        if !config.is_empty() {
            p["config"] = Value::Object(config);
        }
        p
    }

    fn thread_config(spec: &LaunchSpec) -> Map<String, Value> {
        let mut config = Map::new();
        if let Some(tools) = &spec.seat_tools {
            if let Value::Object(tools_config) = Self::mcp_config(tools) {
                config = tools_config;
            }
            // All delegation uses the managed MCP tool and therefore appears on the live board.
            config.insert("features.multi_agent".into(), json!(false));
            config.insert("features.multi_agent_v2".into(), json!(false));
        }
        if let Some(limit) = spec.compact_tokens {
            config.insert("model_auto_compact_token_limit".into(), json!(limit));
        }
        for s in &spec.mcp_servers {
            config.insert(
                format!("mcp_servers.{}.command", s.name),
                json!(s.program.display().to_string()),
            );
            config.insert(format!("mcp_servers.{}.args", s.name), json!(s.args));
        }
        Self::remote_config(spec, &mut config);
        config
    }

    fn remote_config(spec: &LaunchSpec, config: &mut Map<String, Value>) {
        let indexed_remote_mcp = spec.remote_mcp.iter().enumerate();
        for (i, s) in indexed_remote_mcp {
            config.insert(format!("mcp_servers.{}.url", s.name), json!(s.url));
            if !s.headers.is_empty() {
                let from_env: Map<String, Value> = s
                    .headers
                    .iter()
                    .enumerate()
                    .map(|(j, (k, _))| (k.clone(), json!(remote_header_env(i, j))))
                    .collect();
                config.insert(
                    format!("mcp_servers.{}.env_http_headers", s.name),
                    Value::Object(from_env),
                );
            }
        }
    }

    /// The header values of the remote servers, as the environment variables `env_http_headers` names,
    /// so they are in neither the command line nor the thread's config.
    #[must_use]
    pub fn remote_env(spec: &LaunchSpec) -> Vec<(String, String)> {
        spec.remote_mcp
            .iter()
            .enumerate()
            .flat_map(|(i, s)| {
                s.headers
                    .iter()
                    .enumerate()
                    .map(move |(j, (_, v))| (remote_header_env(i, j), v.clone()))
            })
            .collect()
    }

    /// Config overrides that start the `rig` MCP server (`sculptured seat-tools`) for this thread,
    /// as dotted keys (codex-m2-seat-config-dotted: started, listed and called).
    #[must_use]
    pub fn mcp_config(t: &SeatToolsLaunch) -> Value {
        let env: Map<String, Value> = t
            .env
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        let key = |k: &str| format!("mcp_servers.{}.{k}", crate::MCP_SERVER);
        let timeout_sec = crate::MCP_TOOL_TIMEOUT_MS
            .parse::<u64>()
            .unwrap_or(86_700_000)
            / 1000;
        let mut m = Map::new();
        m.insert(key("command"), json!(t.program.display().to_string()));
        m.insert(key("args"), json!(t.args));
        m.insert(key("env"), Value::Object(env));
        m.insert(key("tool_timeout_sec"), json!(timeout_sec));
        Value::Object(m)
    }
}

/// The environment variable holding header `j` of remote server `i`.
fn remote_header_env(i: usize, j: usize) -> String {
    format!("SCULPTURED_MCP_HEADER_{i}_{j}")
}
