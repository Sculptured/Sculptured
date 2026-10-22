use drivers::stub::StubDriver;
use drivers::*;
use futures::StreamExt;
use std::time::Duration;

async fn next(s: &mut futures::stream::BoxStream<'static, DriverEvent>) -> DriverEvent {
    tokio::time::timeout(Duration::from_secs(2), s.next()).await.expect("event in time").expect("stream open")
}

#[tokio::test]
async fn fork_starts_a_new_session() {
    let dir = tempfile::tempdir().unwrap();
    assert!(StubDriver.capabilities().fork);
    let spec = LaunchSpec::new("dev.a@r".parse().unwrap(), dir.path().into());
    let mut s = StubDriver.fork(spec, "stub-dev.a@r").await.unwrap();
    let mut ev = s.take_events().unwrap();
    match next(&mut ev).await {
        DriverEvent::SessionReady { native_session_id } => {
            assert!(native_session_id.starts_with("stub-fork-"), "{native_session_id}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_session_without_questions_refuses_an_answer() {
    let dir = tempfile::tempdir().unwrap();
    let spec = LaunchSpec::new("dev.a@r".parse().unwrap(), dir.path().into());
    let mut s = StubDriver.launch(spec).await.unwrap();
    let err = s.answer("q-none", model::Answer::Allow).await.unwrap_err();
    assert!(err.to_string().contains("answer"), "{err}");
}

use model::{Answer, PermissionPosture, QuestionSource, TurnStatus};

fn msg(id: &str, text: &str) -> OutboundMessage {
    OutboundMessage { message_id: id.into(), intent: model::MessageIntent::Inform, text: text.into() }
}

async fn session(posture: PermissionPosture) -> (Box<dyn Session>, futures::stream::BoxStream<'static, DriverEvent>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut spec = LaunchSpec::new("dev.a@r".parse().unwrap(), dir.path().into());
    spec.posture = posture;
    let mut s = StubDriver.launch(spec).await.unwrap();
    let mut ev = s.take_events().unwrap();
    assert!(matches!(next(&mut ev).await, DriverEvent::SessionReady { .. }));
    (s, ev, dir)
}

/// Events until (and including) the turn's end.
async fn rest_of_turn(ev: &mut futures::stream::BoxStream<'static, DriverEvent>) -> Vec<DriverEvent> {
    let mut out = Vec::new();
    loop {
        let e = next(ev).await;
        let end = matches!(e, DriverEvent::TurnEnded { .. });
        out.push(e);
        if end {
            return out;
        }
    }
}

async fn question_of(ev: &mut futures::stream::BoxStream<'static, DriverEvent>) -> (String, QuestionSource, model::QuestionPrompt) {
    loop {
        if let DriverEvent::QuestionRaised { question_id, source, prompt, .. } = next(ev).await {
            return (question_id, source, prompt);
        }
    }
}

#[tokio::test]
async fn ask_waits_for_an_answer_and_replies_with_it() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    assert!(StubDriver.capabilities().questions);
    s.send(msg("m1", "ask:Which colour?|Blue|Green")).await.unwrap();
    let (id, source, prompt) = question_of(&mut ev).await;
    assert!(id.starts_with("q-") && id.len() == 34, "{id}");
    assert_eq!(source, QuestionSource::NativeQuestion);
    assert_eq!(prompt.text, "Which colour?");
    assert_eq!(prompt.options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["Blue", "Green"]);
    s.send(msg("m2", "queued meanwhile")).await.unwrap();
    s.answer(&id, Answer::Text { text: "Green".into() }).await.unwrap();
    let turn = rest_of_turn(&mut ev).await;
    assert_eq!(turn[0], DriverEvent::QuestionResolved { question_id: id.clone() });
    assert!(turn.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub got answer: Green")));
    assert!(matches!(turn.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Completed, .. })));
    let next_turn = rest_of_turn(&mut ev).await;
    assert!(next_turn.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub received: queued meanwhile")), "the queue drains after the answer");
    assert!(s.answer(&id, Answer::Allow).await.is_err(), "a question is answered once");
}

#[tokio::test]
async fn question_ids_differ_between_sessions() {
    let (mut a, mut ea, _da) = session(PermissionPosture::Unrestricted).await;
    let (mut b, mut eb, _db) = session(PermissionPosture::Unrestricted).await;
    a.send(msg("m1", "ask:One?")).await.unwrap();
    b.send(msg("m1", "ask:One?")).await.unwrap();
    assert_ne!(question_of(&mut ea).await.0, question_of(&mut eb).await.0);
}

#[tokio::test]
async fn an_expired_ask_decides_or_stays_silent() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    s.send(msg("m1", "ask:Pick one|Red|Blue")).await.unwrap();
    let (id, ..) = question_of(&mut ev).await;
    s.answer(&id, Answer::Expired).await.unwrap();
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "No answer came, so I decided: Red")));
    s.send(msg("m2", "ask-silent:Anything?")).await.unwrap();
    let (id, ..) = question_of(&mut ev).await;
    s.answer(&id, Answer::Expired).await.unwrap();
    let turn = rest_of_turn(&mut ev).await;
    let texts: Vec<&str> = turn.iter().filter_map(|e| match e { DriverEvent::Text { text, .. } => Some(text.as_str()), _ => None }).collect();
    assert!(texts.is_empty(), "silent: no decision text, got {texts:?}");
}

#[tokio::test]
async fn perm_asks_only_when_restricted() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    s.send(msg("m1", "perm:ls -la")).await.unwrap();
    let turn = rest_of_turn(&mut ev).await;
    assert!(!turn.iter().any(|e| matches!(e, DriverEvent::QuestionRaised { .. })));
    assert!(turn.iter().any(|e| matches!(e, DriverEvent::ToolCall { name, .. } if name == "Bash")));

    let (mut s, mut ev, _d) = session(PermissionPosture::Restricted).await;
    s.send(msg("m1", "perm:rm -rf build")).await.unwrap();
    let (id, source, prompt) = question_of(&mut ev).await;
    assert_eq!(source, QuestionSource::NativePermission);
    assert_eq!(prompt.tool.as_deref(), Some("Bash"));
    assert_eq!(prompt.tool_input.unwrap()["command"], "rm -rf build");
    s.answer(&id, Answer::Deny { reason: None }).await.unwrap();
    let turn = rest_of_turn(&mut ev).await;
    assert!(!turn.iter().any(|e| matches!(e, DriverEvent::ToolCall { .. })), "denied: nothing ran");
    assert!(turn.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub was not allowed to run rm -rf build")));
    s.send(msg("m2", "perm:make")).await.unwrap();
    let (id, ..) = question_of(&mut ev).await;
    s.answer(&id, Answer::Expired).await.unwrap();
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::ToolCall { input, .. } if input["command"] == "make")), "D8: allowed on timeout");
}

#[tokio::test]
async fn interrupt_drops_the_pending_question() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    s.send(msg("m1", "ask:Wait?")).await.unwrap();
    let (id, ..) = question_of(&mut ev).await;
    s.interrupt().await.unwrap();
    assert!(matches!(rest_of_turn(&mut ev).await.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Interrupted, .. })));
    assert!(s.answer(&id, Answer::Text { text: "late".into() }).await.is_err());
}

#[tokio::test]
async fn fail_ends_the_turn_failed() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    s.send(msg("m1", "fail")).await.unwrap();
    let turn = rest_of_turn(&mut ev).await;
    assert!(matches!(turn.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Failed, .. })), "{turn:?}");
    s.send(msg("m2", "still here")).await.unwrap();
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub received: still here")));
}

#[tokio::test]
async fn limit_ends_the_turn_on_a_usage_limit_with_the_clis_words() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    s.send(msg("m1", "limit")).await.unwrap();
    let turn = rest_of_turn(&mut ev).await;
    assert!(turn.iter().any(|e| matches!(e, DriverEvent::LimitReached { kind: model::LimitKind::Usage, resets: Some(r), .. } if r == "4:30am")), "{turn:?}");
    assert!(matches!(turn.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Failed, .. })), "{turn:?}");
}

#[tokio::test]
async fn ask_only_accepts_just_its_options() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    s.send(msg("m1", "ask:Open?|A|B")).await.unwrap();
    assert!(question_of(&mut ev).await.2.allow_other, "ask: takes other answers too");
    s.interrupt().await.unwrap();
    s.send(msg("m2", "ask-only:Pick|A|B")).await.unwrap();
    let (id, source, prompt) = question_of(&mut ev).await;
    assert_eq!(source, QuestionSource::NativeQuestion);
    assert!(!prompt.allow_other, "ask-only: only its options");
    assert_eq!(prompt.options.len(), 2);
    s.answer(&id, Answer::Expired).await.unwrap();
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "No answer came, so I decided: A")));
}

#[tokio::test]
async fn a_practice_agent_without_tools_says_it_cannot_call_them() {
    let (mut s, mut ev, _d) = session(PermissionPosture::Unrestricted).await;
    s.send(msg("m1", "request:Which?|A")).await.unwrap();
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub has no Sculptured tools, so it could not call request_input")));
    s.send(msg("m2", "report:All good")).await.unwrap();
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub has no Sculptured tools, so it could not call report")));
}

/// A fake hostd socket: hands each request to the test and replies with what the test sends back.
struct FakeSocket {
    path: std::path::PathBuf,
    requests: std::sync::mpsc::Receiver<host_proto::seat_tools::ToolRequest>,
    replies: std::sync::mpsc::Sender<host_proto::seat_tools::ToolReply>,
}

fn fake_socket(dir: &std::path::Path) -> FakeSocket {
    use std::io::{BufRead, Write};
    let path = dir.join("seat-tools.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let (req_tx, requests) = std::sync::mpsc::channel();
    let (replies, reply_rx) = std::sync::mpsc::channel::<host_proto::seat_tools::ToolReply>();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { return };
            let mut line = String::new();
            std::io::BufReader::new(conn.try_clone().unwrap()).read_line(&mut line).unwrap();
            if req_tx.send(serde_json::from_str(line.trim()).unwrap()).is_err() {
                return;
            }
            let Ok(reply) = reply_rx.recv() else { return };
            let _ = writeln!(conn, "{}", serde_json::to_string(&reply).unwrap());
        }
    });
    FakeSocket { path, requests, replies }
}

async fn tool_session(socket: &std::path::Path, dir: &std::path::Path) -> (Box<dyn Session>, futures::stream::BoxStream<'static, DriverEvent>) {
    let mut spec = LaunchSpec::new("dev.a@r".parse().unwrap(), dir.into());
    spec.seat_tools = Some(SeatToolsLaunch {
        program: "/opt/sculptured".into(),
        args: vec!["seat-tools".into()],
        env: host_proto::seat_tools::shim_env(socket, "tok", false).into(),
        approve: false,
        config_file: None,
    });
    let mut s = StubDriver.launch(spec).await.unwrap();
    let mut ev = s.take_events().unwrap();
    assert!(matches!(next(&mut ev).await, DriverEvent::SessionReady { .. }));
    (s, ev)
}

fn reply(result: serde_json::Value) -> host_proto::seat_tools::ToolReply {
    host_proto::seat_tools::ToolReply { ok: true, result, error: None }
}

async fn request(f: &FakeSocket) -> host_proto::seat_tools::ToolRequest {
    for _ in 0..500 {
        if let Ok(r) = f.requests.try_recv() {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no tool call reached the socket");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_timed_out_request_records_the_first_option_as_the_decision() {
    let dir = tempfile::tempdir().unwrap();
    let f = fake_socket(dir.path());
    let (mut s, mut ev) = tool_session(&f.path, dir.path()).await;
    s.send(msg("m1", "request:Pick one|Red|Blue")).await.unwrap();
    let r = request(&f).await;
    assert_eq!((r.token.as_str(), r.tool.as_str()), ("tok", "request_input"));
    assert_eq!(r.args, serde_json::json!({ "question": "Pick one", "options": ["Red", "Blue"] }));
    f.replies.send(reply(serde_json::json!({ "text": "No operator answer within 10m.", "question_id": "q-1", "timed_out": true }))).unwrap();
    let decision = request(&f).await;
    assert_eq!(decision.tool, "record_decision");
    assert_eq!((decision.args["question_id"].as_str(), decision.args["choice"].as_str()), (Some("q-1"), Some("Red")));
    f.replies.send(reply(serde_json::json!({ "text": "Recorded your decision." }))).unwrap();
    let turn = rest_of_turn(&mut ev).await;
    assert!(turn.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "I decided: Red")), "{turn:?}");
    assert!(matches!(turn.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Completed, .. })));
}

/// The turn waits on the tool call; an interrupt ends it at once, and the call's late reply adds nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupt_ends_a_turn_that_waits_on_a_tool_call() {
    let dir = tempfile::tempdir().unwrap();
    let f = fake_socket(dir.path());
    let (mut s, mut ev) = tool_session(&f.path, dir.path()).await;
    s.send(msg("m1", "request:Wait?")).await.unwrap();
    request(&f).await;
    s.send(msg("m2", "queued meanwhile")).await.unwrap();
    s.interrupt().await.unwrap();
    let turn = rest_of_turn(&mut ev).await;
    assert!(matches!(turn.last(), Some(DriverEvent::TurnEnded { status: TurnStatus::Interrupted, .. })), "{turn:?}");
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub received: queued meanwhile")));
    f.replies.send(reply(serde_json::json!({ "text": "too late", "timed_out": false }))).unwrap();
    let late = tokio::time::timeout(Duration::from_millis(300), ev.next()).await;
    assert!(late.is_err(), "nothing after the interrupt: {late:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_report_goes_through_the_socket_and_the_turn_ends_by_itself() {
    let dir = tempfile::tempdir().unwrap();
    let f = fake_socket(dir.path());
    let (mut s, mut ev) = tool_session(&f.path, dir.path()).await;
    s.send(msg("m1", "report:Two problems|missing test|typo")).await.unwrap();
    let r = request(&f).await;
    assert_eq!(r.tool, "report");
    assert_eq!(r.args, serde_json::json!({ "summary": "Two problems", "findings": ["missing test", "typo"], "verdict": "problems-found" }));
    s.send(msg("m2", "after the report")).await.unwrap();
    f.replies.send(reply(serde_json::json!({ "text": "Recorded your report." }))).unwrap();
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub reported: Two problems")));
    assert!(rest_of_turn(&mut ev).await.iter().any(|e| matches!(e, DriverEvent::Text { text, .. } if text == "stub received: after the report")), "the queue drains");
}
