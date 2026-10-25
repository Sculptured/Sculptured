use super::{
    APPROVE_ENV, Arc, BufRead, Mutex, PathBuf, SOCKET_ENV, TOKEN_ENV, Value, Write, collaboration,
    json, pdf, workflow,
};

/// The socket's file name inside hostd's data dir (`host-local/`, private to the user).
pub const SOCKET_FILE: &str = "seat-tools.sock";
/// The longest socket path that binds everywhere (`sun_path` is 108 bytes on Linux, 104 on macOS).
pub const MAX_SOCKET_PATH: usize = 100;

/// Where the agent tool socket goes for hostd data folder `data_dir`: inside it, unless that path is
/// too long for a Unix socket; then in a private `sculptured` folder under `short_root`, named for the
/// data folder.
#[must_use]
pub fn socket_path_in(data_dir: &std::path::Path, short_root: &std::path::Path) -> PathBuf {
    let inside = data_dir.join(SOCKET_FILE);
    if inside.as_os_str().len() < MAX_SOCKET_PATH {
        return inside;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data_dir.hash(&mut h);
    short_root
        .join("sculptured")
        .join(format!("{:016x}.sock", h.finish()))
}

/// `socket_path_in` with the user's runtime folder (or the temp folder) as the short root, creating
/// the private folder when the socket goes there.
pub fn socket_path(data_dir: &std::path::Path) -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    let path = socket_path_in(data_dir, &root);
    if let Some(dir) = path.parent().filter(|_| !path.starts_with(data_dir)) {
        std::fs::create_dir_all(dir)
            .and_then(|_| crate::private::narrow_dir(dir))
            .map_err(|e| format!("cannot make the agent tool folder {}: {e}", dir.display()))?;
    }
    Ok(path)
}

/// Used when the client does not say which MCP protocol version it speaks.
pub(super) const DEFAULT_PROTOCOL: &str = "2025-06-18";

/// Which optional tools an agent is offered: `approve` for a restricted Claude agent, the workflow tools
/// for a team with a dynamic workflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    pub approve: bool,
    pub workflow: bool,
    /// `read_skill`, for an agent whose CLI cannot load its skills natively.
    pub skills: bool,
}

/// The tools an agent sees; `approve` only when launched with it (a restricted Claude agent).
#[must_use]
pub fn tool_list(approve: bool) -> Value {
    tool_list_for(Offer {
        approve,
        workflow: true,
        skills: false,
    })
}

#[must_use]
pub fn tool_list_for(offer: Offer) -> Value {
    let approve = offer.approve;
    let mut tools = vec![request_input_tool(), record_decision_tool(), report_tool()];
    let workflow_only = |t: &Value| {
        matches!(
            t["name"].as_str(),
            Some("dispatch_work" | "complete_work" | "workflow_status")
        )
    };
    tools.extend(
        workflow::tools()
            .into_iter()
            .filter(|t| offer.workflow || !workflow_only(t)),
    );
    tools.extend(collaboration::tools());
    tools.push(pdf::tool());
    if offer.skills {
        tools.push(read_skill_tool());
    }
    if approve {
        tools.push(approve_tool());
    }
    Value::Array(tools)
}

pub(super) fn read_skill_tool() -> Value {
    json!({
        "name": "read_skill",
        "description": "Load one of your skills (listed in your briefing with when to use each) and follow its instructions. Load a skill only when its description applies to what you are doing.",
        "inputSchema": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }
    })
}

pub(super) fn request_input_tool() -> Value {
    json!({
        "name": "request_input",
        "description": "Ask the person running this team a question and wait for the answer. If nobody answers in time you are told so: decide yourself, then call record_decision.",
        "inputSchema": { "type": "object", "properties": {
            "question": { "type": "string" },
            "options": { "type": "array", "items": { "type": "string" } },
            "header": { "type": "string" },
            "urgency": { "type": "string", "enum": ["low", "normal", "high"], "description": "high: your work cannot go on, or others wait on the answer; low: a preference you could decide yourself. Urgent questions are shown first." }
        }, "required": ["question"] }
    })
}

pub(super) fn record_decision_tool() -> Value {
    json!({
        "name": "record_decision",
        "description": "After a question timed out, say what you decided and why.",
        "inputSchema": { "type": "object", "properties": {
            "question_id": { "type": "string" },
            "choice": { "type": "string" },
            "rationale": { "type": "string" }
        }, "required": ["question_id", "choice"] }
    })
}

pub(super) fn report_tool() -> Value {
    let finding = json!({ "anyOf": [
        { "type": "string" },
        { "type": "object", "properties": { "text": { "type": "string" }, "location": { "type": "string" } }, "required": ["text"] }
    ] });
    json!({
        "name": "report",
        "description": "Report findings about the work you were handed (for example problems a check found). Shown to the person as reported by you.",
        "inputSchema": { "type": "object", "properties": {
            "summary": { "type": "string" },
            "findings": { "type": "array", "items": finding },
            "verdict": { "type": "string", "enum": ["no-problems", "problems-found", "unsure"] }
        }, "required": ["summary"] }
    })
}

pub(super) fn approve_tool() -> Value {
    json!({
        "name": "approve",
        "description": "Asks the person whether a tool may run.",
        "inputSchema": { "type": "object", "properties": {
            "tool_name": { "type": "string" },
            "input": { "type": "object" },
            "tool_use_id": { "type": "string" }
        }, "required": ["tool_name", "input"] }
    })
}

pub(super) fn known(tool: &str, offer: Offer) -> bool {
    let workflow = matches!(tool, "dispatch_work" | "complete_work" | "workflow_status");
    matches!(
        tool,
        "request_input"
            | "record_decision"
            | "report"
            | "spawn_agent"
            | "send_agent_message"
            | "team_status"
            | "read_decisions"
            | "read_pdf"
            | "search_history"
            | "record_lesson"
            | "write_status"
    ) || (offer.workflow && workflow)
        || (offer.approve && tool == "approve")
        || (offer.skills && tool == "read_skill")
}

pub(super) fn reply(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

pub(super) fn error(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

pub(super) fn tool_result(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// One JSON-RPC line in, at most one line out (notifications get none). `call` makes a tool call.
pub fn handle_rpc(
    line: &str,
    approve: bool,
    call: &dyn Fn(&str, Value) -> Result<Value, String>,
) -> Option<String> {
    handle_rpc_with(
        line,
        Offer {
            approve,
            workflow: true,
            skills: false,
        },
        call,
    )
}

pub fn handle_rpc_with(
    line: &str,
    offer: Offer,
    call: &dyn Fn(&str, Value) -> Result<Value, String>,
) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let id = msg.get("id").cloned()?;
    Some(match msg["method"].as_str().unwrap_or_default() {
        "initialize" => {
            let version = msg["params"]["protocolVersion"]
                .as_str()
                .unwrap_or(DEFAULT_PROTOCOL);
            reply(
                id,
                json!({ "protocolVersion": version, "capabilities": { "tools": {} }, "serverInfo": { "name": "rig", "version": env!("CARGO_PKG_VERSION") } }),
            )
        }
        "ping" => reply(id, json!({})),
        "tools/list" => reply(id, json!({ "tools": tool_list_for(offer) })),
        "tools/call" => reply(id, tool_call(&msg["params"], offer, call)),
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

/// A `tools/call`'s result: the reply's `text` as the content the agent reads.
pub(super) fn tool_call(
    params: &Value,
    offer: Offer,
    call: &dyn Fn(&str, Value) -> Result<Value, String>,
) -> Value {
    let name = params["name"].as_str().unwrap_or_default();
    if !known(name, offer) {
        return tool_result(
            format!("the tool {name:?} is not available to this agent"),
            true,
        );
    }
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    // Read on the agent's own computer: the PDF never leaves it.
    let result = if name == "read_pdf" {
        pdf::read(&args)
    } else {
        call(name, args)
    };
    match result {
        Ok(v) => tool_result(
            v["text"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| v.to_string()),
            false,
        ),
        Err(e) => tool_result(e, true),
    }
}

/// `sculptured seat-tools`: serves MCP on stdin/stdout until stdin closes. Each request runs on its
/// own thread, so a `ping` is answered while a question waits.
pub fn run_shim() -> Result<(), String> {
    let socket = std::env::var_os(SOCKET_ENV)
        .map(PathBuf::from)
        .ok_or(format!("{SOCKET_ENV} is not set"))?;
    let token = std::env::var(TOKEN_ENV).map_err(|_| format!("{TOKEN_ENV} is not set"))?;
    let approve = std::env::var(APPROVE_ENV).is_ok_and(|v| v == "1");
    // Teams without a workflow are told so by the launch; when it says nothing, the tools are offered.
    let offer = Offer {
        approve,
        workflow: std::env::var(host_proto::seat_tools::WORKFLOW_ENV).map_or(true, |v| v != "0"),
        skills: std::env::var(host_proto::seat_tools::SKILLS_ENV).is_ok_and(|v| v == "1"),
    };
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let matching_items = std::io::stdin().lock().lines();
    for line in matching_items {
        let line = line.map_err(|e| format!("cannot read from the agent: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let (socket, token, out) = (socket.clone(), token.clone(), out.clone());
        std::thread::spawn(move || {
            if let Some(r) = handle_rpc_with(&line, offer, &|tool, args| {
                crate::seat_tools::call(&socket, &token, tool, args)
            }) {
                let mut o = out.lock().expect("out mutex must not be poisoned");
                let _ = writeln!(o, "{r}");
                let _ = o.flush();
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod socket_path_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_socket_stays_in_a_short_data_folder() {
        assert_eq!(
            socket_path_in(
                Path::new("/home/me/.sculptured/host-local"),
                Path::new("/run/user/1000")
            ),
            PathBuf::from("/home/me/.sculptured/host-local/seat-tools.sock")
        );
    }

    #[test]
    fn a_long_data_folder_gets_a_short_socket_path_of_its_own() {
        let long = format!("/home/me/{}/data", "a-long-folder-name/".repeat(8));
        let other = format!("{long}2");
        let p = socket_path_in(Path::new(&long), Path::new("/run/user/1000"));
        assert!(
            p.starts_with("/run/user/1000/sculptured") && p.as_os_str().len() < MAX_SOCKET_PATH,
            "{}",
            p.display()
        );
        assert_ne!(
            p,
            socket_path_in(Path::new(&other), Path::new("/run/user/1000")),
            "each data folder has its own socket"
        );
    }
}
