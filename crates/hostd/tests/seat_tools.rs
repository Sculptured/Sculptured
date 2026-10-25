//! The shim as a pure function, and the relay between an agent's tool call and controld over the link.
mod common;

use common::CALL_WAIT;

use host_proto::{FromHost, ToHost};
use hostd::seat_tools::Offer;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::net::TcpListener;

// ---- the shim, as a pure function ----

#[tokio::test]
async fn pending_calls_keep_their_frame_until_the_matching_reply_arrives() {
    let supervisor = crate::common::stub_sup();
    let mut outgoing = supervisor.subscribe_tool_calls();
    let caller = supervisor.clone();
    let task = tokio::spawn(async move {
        caller
            .tool_call("dev.a@team", "report", json!({"result": "ready"}))
            .await
    });
    let frame = tokio::time::timeout(CALL_WAIT, outgoing.recv())
        .await
        .unwrap()
        .unwrap();
    let FromHost::SeatTool { call_id, .. } = &frame else {
        panic!("a tool frame")
    };
    assert_eq!(supervisor.pending_tool_calls(), vec![frame.clone()]);
    supervisor.resolve_tool_call("unknown", Ok(json!({})));
    assert!(supervisor.tool_call_pending(call_id));
    supervisor.resolve_tool_call(call_id, Ok(json!({"accepted": true})));
    assert_eq!(
        crate::common::ended(task).await.unwrap(),
        json!({"accepted": true})
    );
    assert!(supervisor.pending_tool_calls().is_empty());
}

fn no_call(_: &str, _: Value) -> Result<Value, String> {
    panic!("no tool call expected")
}

fn rpc(line: Value, approve: bool, call: &dyn Fn(&str, Value) -> Result<Value, String>) -> Value {
    serde_json::from_str(
        &hostd::seat_tools::handle_rpc(&line.to_string(), approve, call).expect("a reply"),
    )
    .unwrap()
}

#[test]
fn the_shim_initializes_and_lists_its_tools() {
    let init = rpc(
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
        false,
        &no_call,
    );
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "rig");
    assert!(init["result"]["capabilities"]["tools"].is_object());
    assert!(
        hostd::seat_tools::handle_rpc(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            false,
            &no_call
        )
        .is_none(),
        "notifications get no reply"
    );
    assert_eq!(
        rpc(
            json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
            false,
            &no_call
        )["result"],
        json!({})
    );
    let names = |approve| -> Vec<String> {
        hostd::seat_tools::tool_list(approve)
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        names(false),
        [
            "request_input",
            "record_decision",
            "report",
            "dispatch_work",
            "complete_work",
            "workflow_status",
            "read_decisions",
            "search_history",
            "record_lesson",
            "spawn_agent",
            "send_agent_message",
            "team_status",
            "write_status",
            "read_pdf"
        ]
    );
    assert_eq!(
        names(true),
        [
            "request_input",
            "record_decision",
            "report",
            "dispatch_work",
            "complete_work",
            "workflow_status",
            "read_decisions",
            "search_history",
            "record_lesson",
            "spawn_agent",
            "send_agent_message",
            "team_status",
            "write_status",
            "read_pdf",
            "approve"
        ]
    );
    let listed = rpc(
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }),
        true,
        &no_call,
    );
    assert_eq!(
        listed["result"]["tools"][0]["inputSchema"]["required"],
        json!(["question"])
    );
}

#[test]
fn the_shim_relays_a_call_and_shows_the_text() {
    let call = |tool: &str, args: Value| -> Result<Value, String> {
        assert_eq!(tool, "report");
        assert_eq!(args["summary"], "ok");
        Ok(json!({ "text": "Recorded your report." }))
    };
    let r = rpc(
        json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "report", "arguments": { "summary": "ok" } } }),
        false,
        &call,
    );
    assert_eq!(r["id"], 7);
    assert_eq!(
        r["result"]["content"][0],
        json!({ "type": "text", "text": "Recorded your report." })
    );
    assert_eq!(r["result"]["isError"], false);
    let failing =
        |_: &str, _: Value| -> Result<Value, String> { Err("the question was withdrawn".into()) };
    let r = rpc(
        json!({ "jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": { "name": "request_input", "arguments": { "question": "?" } } }),
        false,
        &failing,
    );
    assert_eq!(r["result"]["isError"], true);
    assert_eq!(
        r["result"]["content"][0]["text"],
        "the question was withdrawn"
    );
}

#[test]
fn the_shim_refuses_approve_unless_restricted_and_unknown_methods() {
    let r = rpc(
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "approve", "arguments": {} } }),
        false,
        &no_call,
    );
    assert_eq!(r["result"]["isError"], true);
    let r = rpc(
        json!({ "jsonrpc": "2.0", "id": 2, "method": "resources/list" }),
        false,
        &no_call,
    );
    assert_eq!(r["error"]["code"], -32601);
    let r: Value =
        serde_json::from_str(&hostd::seat_tools::handle_rpc("not json", false, &no_call).unwrap())
            .unwrap();
    assert_eq!(r["error"]["code"], -32700);
}

// ---- the relay, against a fake controld ----

/// No link yet (the computer is offline): the call waits, and goes as soon as a link comes up.
#[tokio::test]
async fn without_controld_a_tool_call_waits_for_the_link() {
    let sup = crate::common::stub_sup();
    let s = sup.clone();
    let call = tokio::spawn(async move {
        s.tool_call("dev.a@r", "report", json!({ "summary": "later" }))
            .await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!call.is_finished(), "the call waits instead of failing");
    let mut ws = crate::common::linked(&sup).await;
    let (call_id, _, _, args) = crate::common::next_call(&mut ws).await;
    assert_eq!(args["summary"], "later");
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id,
            ok: true,
            result: json!({ "text": "ok" }),
            error: None,
        },
    )
    .await;
    assert_eq!(crate::common::ended(call).await.unwrap()["text"], "ok");
}

#[tokio::test]
async fn a_tool_call_goes_to_controld_and_back() {
    let sup = crate::common::stub_sup();
    let mut ws = crate::common::linked(&sup).await;
    let s = sup.clone();
    let call = tokio::spawn(async move {
        s.tool_call("dev.a@r", "report", json!({ "summary": "ok" }))
            .await
    });
    let (call_id, seat, tool, args) = crate::common::next_call(&mut ws).await;
    assert_eq!(
        (seat.as_str(), tool.as_str(), args["summary"].as_str()),
        ("dev.a@r", "report", Some("ok"))
    );
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id,
            ok: true,
            result: json!({ "text": "done" }),
            error: None,
        },
    )
    .await;
    assert_eq!(
        crate::common::ended(call).await.unwrap(),
        json!({ "text": "done" })
    );
}

#[tokio::test]
async fn a_refused_call_returns_controlds_words() {
    let sup = crate::common::stub_sup();
    let mut ws = crate::common::linked(&sup).await;
    let s = sup.clone();
    let call =
        tokio::spawn(async move { s.tool_call("dev.a@r", "launch_missiles", json!({})).await });
    let (call_id, ..) = crate::common::next_call(&mut ws).await;
    assert!(sup.tool_call_pending(&call_id));
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id: call_id.clone(),
            ok: false,
            result: Value::Null,
            error: Some("unknown tool \"launch_missiles\"".into()),
        },
    )
    .await;
    assert_eq!(
        crate::common::ended(call).await.unwrap_err(),
        "unknown tool \"launch_missiles\""
    );
    assert!(
        !sup.tool_call_pending(&call_id),
        "answered calls are forgotten"
    );
}

/// A link that drops mid-call keeps the agent's call waiting; the next link sends it again (same id)
/// and controld's reply reaches the agent.
#[tokio::test]
async fn a_dropped_link_resends_pending_tool_calls_on_reconnect() {
    let sup = crate::common::stub_sup();
    let mut ws = crate::common::linked(&sup).await;
    let s = sup.clone();
    let call = tokio::spawn(async move {
        s.tool_call("dev.a@r", "request_input", json!({ "question": "?" }))
            .await
    });
    let (first_id, ..) = crate::common::next_call(&mut ws).await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!call.is_finished(), "the call survives the drop");
    let mut ws = crate::common::linked(&sup).await;
    let (again, seat, tool, _) = crate::common::next_call(&mut ws).await;
    assert_eq!(
        (again.as_str(), seat.as_str(), tool.as_str()),
        (first_id.as_str(), "dev.a@r", "request_input")
    );
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id: again,
            ok: true,
            result: json!({ "text": "Blue" }),
            error: None,
        },
    )
    .await;
    assert_eq!(crate::common::ended(call).await.unwrap()["text"], "Blue");
}

/// The lag path through a real link: more calls than the link's queue holds pile up before controld
/// welcomes it. None is lost: every one reaches controld, and each answer reaches its call.
#[tokio::test]
async fn a_link_that_lags_still_passes_every_call_on() {
    let sup = crate::common::stub_sup();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/host", listener.local_addr().unwrap());
    let s = sup.clone();
    tokio::spawn(async move { hostd::run_link(&url, "secret", &s).await });
    let mut ws = crate::common::accept(&listener).await;
    assert!(matches!(
        crate::common::recv(&mut ws).await,
        FromHost::Hello { .. }
    ));
    // The link has subscribed but reads no call until it is welcomed.
    let calls: Vec<_> = (0..1100)
        .map(|i| {
            let s = sup.clone();
            tokio::spawn(async move {
                s.tool_call(&format!("dev.a{i}@r"), "report", json!({}))
                    .await
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(100)).await;
    crate::common::send(&mut ws, &ToHost::Welcome { resume_after: 0 }).await;
    let mut seen = std::collections::HashSet::new();
    while seen.len() < 1100 {
        let (call_id, ..) = crate::common::next_call(&mut ws).await;
        if seen.insert(call_id.clone()) {
            crate::common::send(
                &mut ws,
                &ToHost::SeatToolReply {
                    call_id,
                    ok: true,
                    result: json!({ "text": "ok" }),
                    error: None,
                },
            )
            .await;
        }
    }
    for c in calls {
        assert_eq!(crate::common::ended(c).await.unwrap()["text"], "ok");
    }
}

#[test]
fn a_team_without_a_workflow_is_not_offered_the_workflow_tools() {
    let offer = Offer {
        approve: false,
        workflow: false,
        skills: false,
    };
    let names: Vec<String> = hostd::seat_tools::tool_list_for(offer)
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "request_input",
            "record_decision",
            "report",
            "read_decisions",
            "search_history",
            "record_lesson",
            "spawn_agent",
            "send_agent_message",
            "team_status",
            "write_status",
            "read_pdf"
        ]
    );
    let call = json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": "complete_work", "arguments": {} } });
    let r: Value = serde_json::from_str(
        &hostd::seat_tools::handle_rpc_with(&call.to_string(), offer, &no_call).unwrap(),
    )
    .unwrap();
    assert_eq!(r["result"]["isError"], true);
    assert!(
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not available")
    );
}
