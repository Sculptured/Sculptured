//! Helpers for the seat-tools tests: a host whose agents have their tools on a real socket, and calls
//! made directly as controld makes them for a host's `SeatTool` frame.
use super::m2::*;
use super::wait_for;
use controld::seat_tools::handle_call;
use drivers::stub::StubDriver;
use host_proto::ToHost;
use hostd::seat_tools::serve_socket;
use hostd::supervisor::{SeatToolsHost, Supervisor};
use model::*;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::JoinHandle;

/// No seat-tool call in these tests may wait longer than this: a hung relay fails the test instead of
/// hanging the suite.
pub const CALL_WAIT: Duration = Duration::from_secs(15);

/// A served controld with a host whose agents have their tools on a real socket.
pub async fn with_tools() -> (Served, Supervisor, tempfile::TempDir) {
    let s = serve().await;
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("seat-tools.sock");
    let host = SeatToolsHost {
        socket: socket.clone(),
        shim: "/unused/sculptured".into(),
    };
    let sup = connect(&s, vec![Arc::new(StubDriver)], Some(host)).await;
    tokio::spawn(serve_socket(sup.clone(), socket.clone()));
    wait_for("the socket", || socket.exists()).await;
    (s, sup, dir)
}

/// Calls a tool directly; the reply channel stays open. Returns (ok, result, error).
pub async fn direct(
    s: &Served,
    seat: &str,
    tool: &str,
    args: Value,
) -> (bool, Value, Option<String>) {
    let (tx, mut rx) = unbounded_channel();
    tokio::time::timeout(
        CALL_WAIT,
        handle_call(
            s.st.clone(),
            tx,
            "c1".into(),
            seat.into(),
            tool.into(),
            args,
        ),
    )
    .await
    .expect("the call ends");
    match rx.recv().await.unwrap() {
        ToHost::SeatToolReply {
            ok, result, error, ..
        } => (ok, result, error),
        other => panic!("{other:?}"),
    }
}

/// Starts a `request_input` call on a connection the test controls; returns the call, the connection's
/// far end and the question it raised.
pub async fn request_call(
    s: &Served,
    seat: &str,
    args: Value,
) -> (JoinHandle<()>, UnboundedReceiver<ToHost>, String) {
    let (tx, rx) = unbounded_channel();
    let call = tokio::spawn(handle_call(
        s.st.clone(),
        tx,
        "c1".into(),
        seat.into(),
        "request_input".into(),
        args,
    ));
    let e = wait_event(&s.st, seat, "a question", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    })
    .await;
    let EventBody::QuestionRaised { question_id, .. } = e.body else {
        unreachable!()
    };
    (call, rx, question_id)
}

/// A `request_input` with one plain question.
pub async fn which(s: &Served, seat: &str) -> (JoinHandle<()>, UnboundedReceiver<ToHost>, String) {
    request_call(s, seat, json!({ "question": "Which?" })).await
}

pub async fn ended(call: JoinHandle<()>) {
    tokio::time::timeout(CALL_WAIT, call)
        .await
        .expect("the call ends")
        .unwrap();
}

/// (tool, reason) of each `report-rejected` on the agent.
pub fn rejected(s: &Served, seat: &str) -> Vec<(String, String)> {
    events(&s.st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::ReportRejected { tool, reason, .. } => Some((tool, reason)),
            _ => None,
        })
        .collect()
}

/// The question as the API shows it.
pub async fn question_view(s: &Served, seat: &str, id: &str) -> Value {
    let (_, qs) = get(&s.router, &format!("/api/seats/{seat}/questions")).await;
    qs.as_array()
        .unwrap()
        .iter()
        .find(|q| q["id"] == id)
        .cloned()
        .unwrap()
}

/// Records a question raised by `seat` (a permission prompt when `tool` is given) that then expired.
pub fn expired_question(
    s: &Served,
    seat: &str,
    id: &str,
    source: QuestionSource,
    tool: Option<&str>,
) {
    let prompt = QuestionPrompt { urgency: None,
        header: None,
        text: "Which?".into(),
        options: vec![],
        allow_other: tool.is_none(),
        tool: tool.map(Into::into),
        tool_input: tool.map(|_| json!({})),
    };
    let raised = EventBody::QuestionRaised {
        question_id: id.into(),
        source,
        prompt,
        turn_id: None,
    };
    s.st.local_event(
        Actor::Seat(seat.into()),
        Subject::Seat(seat.into()),
        Provenance::AgentReported,
        raised,
    )
    .unwrap();
    let expired = EventBody::QuestionExpired {
        question_id: id.into(),
    };
    s.st.local_event(
        Actor::System("timeout".into()),
        Subject::Seat(seat.into()),
        Provenance::ProtocolObserved,
        expired,
    )
    .unwrap();
}
