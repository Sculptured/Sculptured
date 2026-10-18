//! Sculptured's tools for agents. `sculptured seat-tools` is a stdio MCP server (the "shim") that a
//! CLI starts for its agent; it forwards each call over hostd's private socket (`socket`), which relays
//! it to controld over the host link. One JSON line each way on the socket (`host_proto::seat_tools`).
mod request;
mod socket;
mod workflow;
mod collaboration;
mod pdf;

pub use socket::{
    serve_socket, serve_socket_with, SocketLimits, NOT_TEXT, NO_APPROVE, TOO_LARGE, TOO_MANY, TOO_MANY_FOR_AGENT, TOO_SLOW,
};

use host_proto::seat_tools::{call, APPROVE_ENV, SOCKET_ENV, TOKEN_ENV};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The socket's file name inside hostd's data dir (`host-local/`, private to the user).
pub const SOCKET_FILE: &str = "seat-tools.sock";
/// Used when the client does not say which MCP protocol version it speaks.
const DEFAULT_PROTOCOL: &str = "2025-06-18";

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
pub fn tool_list(approve: bool) -> Value {
    tool_list_for(Offer { approve, workflow: true, skills: false })
}

pub fn tool_list_for(offer: Offer) -> Value {
    let approve = offer.approve;
    let mut tools = vec![request_input_tool(), record_decision_tool(), report_tool()];
    let workflow_only = |t: &Value| matches!(t["name"].as_str(), Some("dispatch_work" | "complete_work" | "workflow_status"));
    tools.extend(workflow::tools().into_iter().filter(|t| offer.workflow || !workflow_only(t)));
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

fn read_skill_tool() -> Value {
    json!({
        "name": "read_skill",
        "description": "Load one of your skills (listed in your briefing with when to use each) and follow its instructions. Load a skill only when its description applies to what you are doing.",
        "inputSchema": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }
    })
}

fn request_input_tool() -> Value {
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

fn record_decision_tool() -> Value {
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

fn report_tool() -> Value {
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

fn approve_tool() -> Value {
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

fn known(tool: &str, offer: Offer) -> bool {
    let workflow = matches!(tool, "dispatch_work" | "complete_work" | "workflow_status");
    matches!(tool, "request_input" | "record_decision" | "report" | "spawn_agent" | "send_agent_message" | "team_status" | "read_decisions" | "read_pdf" | "search_history" | "record_lesson")
        || (offer.workflow && workflow)
        || (offer.approve && tool == "approve")
        || (offer.skills && tool == "read_skill")
}

fn reply(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

/// One JSON-RPC line in, at most one line out (notifications get none). `call` makes a tool call.
pub fn handle_rpc(line: &str, approve: bool, call: &dyn Fn(&str, Value) -> Result<Value, String>) -> Option<String> {
    handle_rpc_with(line, Offer { approve, workflow: true, skills: false }, call)
}

pub fn handle_rpc_with(line: &str, offer: Offer, call: &dyn Fn(&str, Value) -> Result<Value, String>) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let id = msg.get("id").cloned()?;
    Some(match msg["method"].as_str().unwrap_or_default() {
        "initialize" => {
            let version = msg["params"]["protocolVersion"].as_str().unwrap_or(DEFAULT_PROTOCOL);
            reply(id, json!({ "protocolVersion": version, "capabilities": { "tools": {} }, "serverInfo": { "name": "rig", "version": env!("CARGO_PKG_VERSION") } }))
        }
        "ping" => reply(id, json!({})),
        "tools/list" => reply(id, json!({ "tools": tool_list_for(offer) })),
        "tools/call" => reply(id, tool_call(&msg["params"], offer, call)),
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

/// A `tools/call`'s result: the reply's `text` as the content the agent reads.
fn tool_call(params: &Value, offer: Offer, call: &dyn Fn(&str, Value) -> Result<Value, String>) -> Value {
    let name = params["name"].as_str().unwrap_or_default();
    if !known(name, offer) {
        return tool_result(format!("the tool {name:?} is not available to this agent"), true);
    }
    let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
    // Read on the agent's own computer: the PDF never leaves it.
    let result = if name == "read_pdf" { pdf::read(&args) } else { call(name, args) };
    match result {
        Ok(v) => tool_result(v["text"].as_str().map(str::to_string).unwrap_or_else(|| v.to_string()), false),
        Err(e) => tool_result(e, true),
    }
}

/// `sculptured seat-tools`: serves MCP on stdin/stdout until stdin closes. Each request runs on its
/// own thread, so a `ping` is answered while a question waits.
pub fn run_shim() -> Result<(), String> {
    let socket = std::env::var_os(SOCKET_ENV).map(PathBuf::from).ok_or(format!("{SOCKET_ENV} is not set"))?;
    let token = std::env::var(TOKEN_ENV).map_err(|_| format!("{TOKEN_ENV} is not set"))?;
    let approve = std::env::var(APPROVE_ENV).is_ok_and(|v| v == "1");
    // Teams without a workflow are told so by the launch; when it says nothing, the tools are offered.
    let offer = Offer {
        approve,
        workflow: std::env::var(host_proto::seat_tools::WORKFLOW_ENV).map_or(true, |v| v != "0"),
        skills: std::env::var(host_proto::seat_tools::SKILLS_ENV).is_ok_and(|v| v == "1"),
    };
    let out = Arc::new(Mutex::new(std::io::stdout()));
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| format!("cannot read from the agent: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let (socket, token, out) = (socket.clone(), token.clone(), out.clone());
        std::thread::spawn(move || {
            if let Some(r) = handle_rpc_with(&line, offer, &|tool, args| call(&socket, &token, tool, args)) {
                let mut o = out.lock().unwrap();
                let _ = writeln!(o, "{r}");
                let _ = o.flush();
            }
        });
    }
    Ok(())
}
