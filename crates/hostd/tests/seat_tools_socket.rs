//! The seat-tools socket: live tokens only, plain refusals, limits, and `approve` enforced by hostd.
mod common;

use host_proto::{HostCommand, ToHost};
use hostd::seat_tools::{
    NO_APPROVE, NOT_TEXT, SocketLimits, TOO_LARGE, TOO_MANY, TOO_MANY_FOR_AGENT, TOO_SLOW,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test(flavor = "multi_thread")]
async fn the_socket_relays_for_a_live_token_only() {
    let dir = tempfile::tempdir().unwrap();
    let (_sup, mut ws, socket, token) = crate::common::tools_host(dir.path()).await;
    assert_eq!(
        std::fs::metadata(&socket)
            .map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o777)
            .unwrap(),
        0o600
    );

    let call = crate::common::blocking_call(
        &socket,
        &token,
        "report",
        json!({ "summary": "two problems" }),
    );
    let (call_id, seat, tool, _) = crate::common::next_call(&mut ws).await;
    assert_eq!((seat.as_str(), tool.as_str()), ("dev.a@r", "report"));
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id,
            ok: true,
            result: json!({ "text": "Recorded your report." }),
            error: None,
        },
    )
    .await;
    assert_eq!(
        crate::common::ended(call).await.unwrap()["text"],
        "Recorded your report."
    );

    let err = crate::common::ended(crate::common::blocking_call(
        &socket,
        "not-a-token",
        "report",
        json!({}),
    ))
    .await
    .unwrap_err();
    assert!(err.contains("not valid"), "{err}");
}

/// Tokens live only as long as the agent's process: a token saved by a CLI (Codex keeps its config in
/// `~/.codex`) is refused once that process has stopped, and nothing reaches controld.
#[tokio::test(flavor = "multi_thread")]
async fn a_stopped_agents_token_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (sup, mut ws, socket, token) = crate::common::tools_host(dir.path()).await;
    assert_eq!(sup.seat_for_token(&token).as_deref(), Some("dev.a@r"));
    sup.handle(HostCommand::Stop {
        seat: "dev.a@r".into(),
        grace_ms: 100,
        hard: false,
    })
    .await
    .unwrap();
    assert_eq!(sup.seat_for_token(&token), None);
    let err = crate::common::ended(crate::common::blocking_call(
        &socket,
        &token,
        "report",
        json!({ "summary": "late" }),
    ))
    .await
    .unwrap_err();
    assert!(err.contains("not valid"), "{err}");
    crate::common::no_call_within(&mut ws, Duration::from_millis(300)).await;
}

/// An unreadable line on the socket gets a plain tool error, not a hang or a crash.
#[tokio::test(flavor = "multi_thread")]
async fn a_garbled_socket_request_gets_a_plain_error() {
    let dir = tempfile::tempdir().unwrap();
    let (_sup, _ws, socket, _token) = crate::common::tools_host(dir.path()).await;
    let reply =
        crate::common::ended(crate::common::raw_call(&socket, b"not json\n".to_vec())).await;
    assert_eq!(reply["ok"], false);
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .starts_with("Sculptured could not read this tool call"),
        "{reply}"
    );
}

/// Important 1: a call over 1 MiB would exceed the host link's frame limit and drop the link for every
/// agent. It is refused in plain words, recorded on the agent, and the link carries the next call.
#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_call_is_refused_and_the_link_stays_up() {
    let dir = tempfile::tempdir().unwrap();
    let (_sup, mut ws, socket, token) = crate::common::tools_host(dir.path()).await;
    let huge = json!({ "summary": "x".repeat(2 * 1024 * 1024) });
    let err = crate::common::ended(crate::common::blocking_call(
        &socket, &token, "report", huge,
    ))
    .await
    .unwrap_err();
    assert_eq!(err, TOO_LARGE);
    let (seat, tool, reason) = crate::common::rejected_event(&mut ws).await;
    assert_eq!(
        (seat.as_str(), tool.as_str(), reason.as_str()),
        ("dev.a@r", "report", TOO_LARGE)
    );
    crate::common::no_call_within(&mut ws, Duration::from_millis(200)).await;

    let call =
        crate::common::blocking_call(&socket, &token, "report", json!({ "summary": "small" }));
    let (call_id, _, tool, args) = crate::common::next_call(&mut ws).await;
    assert_eq!(
        (tool.as_str(), args["summary"].as_str()),
        ("report", Some("small"))
    );
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id,
            ok: true,
            result: json!({ "text": "Recorded your report." }),
            error: None,
        },
    )
    .await;
    assert_eq!(
        crate::common::ended(call).await.unwrap()["text"],
        "Recorded your report."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_that_is_not_utf8_gets_a_plain_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let (_sup, _ws, socket, _token) = crate::common::tools_host(dir.path()).await;
    let reply = crate::common::ended(crate::common::raw_call(
        &socket,
        vec![0xff, 0xfe, b'{', b'\n'],
    ))
    .await;
    assert_eq!(
        (reply["ok"].as_bool(), reply["error"].as_str()),
        (Some(false), Some(NOT_TEXT))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connection_that_sends_nothing_is_told_and_closed() {
    let dir = tempfile::tempdir().unwrap();
    let limits = SocketLimits {
        read_timeout: Duration::from_millis(200),
        ..SocketLimits::default()
    };
    let (_sup, _ws, socket, _token) =
        crate::common::tools_host_with(dir.path(), false, limits).await;
    let reply = crate::common::ended(crate::common::raw_call(&socket, vec![])).await;
    assert_eq!(reply["error"], TOO_SLOW);
}

#[tokio::test(flavor = "multi_thread")]
async fn connections_beyond_the_limit_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let limits = SocketLimits {
        max_connections: 1,
        read_timeout: Duration::from_secs(3),
        ..SocketLimits::default()
    };
    let (_sup, _ws, socket, token) =
        crate::common::tools_host_with(dir.path(), false, limits).await;
    // The only place is taken by a connection that has not sent its call yet.
    let _idle = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let err = crate::common::ended(crate::common::blocking_call(
        &socket,
        &token,
        "report",
        json!({ "summary": "x" }),
    ))
    .await
    .unwrap_err();
    assert_eq!(err, TOO_MANY);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_agent_cannot_hold_more_than_its_share_of_calls() {
    let dir = tempfile::tempdir().unwrap();
    let limits = SocketLimits {
        max_per_agent: 1,
        ..SocketLimits::default()
    };
    let (_sup, mut ws, socket, token) =
        crate::common::tools_host_with(dir.path(), false, limits).await;
    let first =
        crate::common::blocking_call(&socket, &token, "request_input", json!({ "question": "?" }));
    let (call_id, ..) = crate::common::next_call(&mut ws).await;
    let err = crate::common::ended(crate::common::blocking_call(
        &socket,
        &token,
        "report",
        json!({ "summary": "x" }),
    ))
    .await
    .unwrap_err();
    assert_eq!(err, TOO_MANY_FOR_AGENT);
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id,
            ok: true,
            result: json!({ "text": "Blue" }),
            error: None,
        },
    )
    .await;
    assert_eq!(crate::common::ended(first).await.unwrap()["text"], "Blue");
    let again = crate::common::blocking_call(&socket, &token, "report", json!({ "summary": "x" }));
    let (call_id, ..) = crate::common::next_call(&mut ws).await;
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
    assert!(
        crate::common::ended(again).await.is_ok(),
        "the place is free again once the first call ended"
    );
}

/// Important 3: `approve` is enforced by hostd, whatever the agent's shim offers.
#[tokio::test(flavor = "multi_thread")]
async fn approve_is_refused_in_hostd_for_an_agent_launched_without_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_sup, mut ws, socket, token) = crate::common::tools_host(dir.path()).await;
    let args = json!({ "tool_name": "Bash", "input": { "command": "rm -rf /" } });
    let err = crate::common::ended(crate::common::blocking_call(
        &socket, &token, "approve", args,
    ))
    .await
    .unwrap_err();
    assert_eq!(err, NO_APPROVE);
    let (seat, tool, reason) = crate::common::rejected_event(&mut ws).await;
    assert_eq!(
        (seat.as_str(), tool.as_str(), reason.as_str()),
        ("dev.a@r", "approve", NO_APPROVE)
    );
    crate::common::no_call_within(&mut ws, Duration::from_millis(200)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn approve_is_relayed_for_an_agent_launched_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_sup, mut ws, socket, token) =
        crate::common::tools_host_with(dir.path(), true, SocketLimits::default()).await;
    let call = crate::common::blocking_call(
        &socket,
        &token,
        "approve",
        json!({ "tool_name": "Bash", "input": {} }),
    );
    let (call_id, _, tool, _) = crate::common::next_call(&mut ws).await;
    assert_eq!(tool, "approve");
    crate::common::send(
        &mut ws,
        &ToHost::SeatToolReply {
            call_id,
            ok: true,
            result: json!({ "text": "{}" }),
            error: None,
        },
    )
    .await;
    assert!(crate::common::ended(call).await.is_ok());
}

#[test]
fn hostd_creates_its_folder_private_and_refuses_one_others_can_open() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let fresh = dir.path().join("a/host-local");
    hostd::run::private_data_dir(&fresh).unwrap();
    assert_eq!(
        std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let open = dir.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = hostd::run::private_data_dir(&open).unwrap_err();
    assert!(err.contains("chmod 700"), "{err}");
    assert_eq!(
        std::fs::metadata(&open).unwrap().permissions().mode() & 0o777,
        0o755,
        "an existing folder is never changed"
    );
}
