//! Agents for the question and seat-tool tests. Contract tests use a recorded agent: a team's rows in
//! the store and its lifecycle as events, with no process and no computer connected; questions come
//! from Sculptured's own `request_input` handler. Live tests start a real Claude agent on the cheapest
//! model whose Sculptured tools run through the real `sculptured seat-tools` shim.
//! Not part of `common/mod.rs`: each file that uses it says `#[path = "common/asking.rs"] mod asking;`.
#![allow(dead_code)]
use crate::common::m2::{Served, call, rid};
use axum::http::StatusCode;
use controld::store::{RigSource, Store};
use host_proto::ToHost;
use hostd::supervisor::Supervisor;
use model::{Actor, Desired, EventBody, Provenance, SeatLifecycle, Subject};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

/// Team `o` with two Claude agents, `dev.a@o` and `dev.b@o`.
pub const RIG: &str = r#"
version: "0.2"
name: o
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: claude-code, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: claude-code, cwd: "." }
"#;

pub const A: &str = "dev.a@o";
pub const B: &str = "dev.b@o";

/// A store holding team `o`, with the agents in `running` recorded as running.
pub fn store_with_agents(running: &[&str]) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let rig = rigspec::compile(RIG).unwrap();
    store
        .upsert_rig(
            &rig,
            &RigSource::File {
                path: "/specs/o.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    for seat in running {
        store
            .append_local(
                Actor::System("test".into()),
                Subject::Seat((*seat).into()),
                Provenance::ProtocolObserved,
                EventBody::SeatLifecycleChanged {
                    lifecycle: SeatLifecycle::Running,
                    reason: None,
                },
            )
            .unwrap();
    }
    store
}

/// A served controld whose agents A and B are recorded as running.
pub async fn recorded_agents() -> Served {
    crate::common::m2::serve_with_store(store_with_agents(&[A, B])).await
}

/// Records `body` on `seat` as the agent's program reported it (protocol-observed).
pub fn observed(s: &Served, seat: &str, body: EventBody) {
    s.st.local_event(
        Actor::Seat(seat.into()),
        Subject::Seat(seat.into()),
        Provenance::ProtocolObserved,
        body,
    )
    .unwrap();
}

/// A `request_input` call in flight, made directly as controld handles a host's `SeatTool` frame.
pub struct Asked {
    pub call: JoinHandle<()>,
    /// The connection's far end: the call's reply arrives here.
    pub link: UnboundedReceiver<ToHost>,
    pub id: String,
}

/// Starts a `request_input` call with `args` on `seat` and waits for the question it raises (the first
/// one after the log's current end, so a seat can ask more than once).
pub async fn ask(s: &Served, seat: &str, args: Value) -> Asked {
    asking_call(s, seat, "request_input", args).await
}

/// Team `o` runs restricted from now on: its Claude agents may use `approve`, Claude's permission-prompt tool.
pub fn restrict(s: &Served) {
    let settings = controld::views::TeamSettings {
        posture: Some(model::PermissionPosture::Restricted),
        ..Default::default()
    };
    s.st.store
        .lock()
        .unwrap()
        .save_team_settings("o", &settings)
        .unwrap();
}

/// Starts an `approve` call (a permission prompt for `ls`) on `seat` of a restricted team (`restrict`).
pub async fn ask_permission(s: &Served, seat: &str) -> Asked {
    let args = json!({ "tool_name": "Bash", "input": { "command": "ls" } });
    asking_call(s, seat, "approve", args).await
}

async fn asking_call(s: &Served, seat: &str, tool: &str, args: Value) -> Asked {
    let after = s.st.store.lock().unwrap().last_seq().unwrap();
    let (tx, link) = tokio::sync::mpsc::unbounded_channel();
    let call = tokio::spawn(controld::seat_tools::handle_call(
        s.st.clone(),
        tx,
        "c1".into(),
        seat.into(),
        tool.into(),
        args,
    ));
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let id = loop {
        let found = crate::common::m2::events(&s.st, seat)
            .into_iter()
            .filter(|e| e.seq > after)
            .find_map(|e| match e.body {
                EventBody::QuestionRaised { question_id, .. } => Some(question_id),
                _ => None,
            });
        if let Some(id) = found {
            break id;
        }
        assert!(tokio::time::Instant::now() < deadline, "no question raised");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    Asked { call, link, id }
}

impl Asked {
    /// Waits for the call to end and returns its one reply: (ok, result, error). Panics on a second reply.
    pub async fn reply(mut self) -> (bool, Value, Option<String>) {
        crate::common::seat_tools::ended(self.call).await;
        let first = self.link.recv().await.expect("the call's reply");
        assert!(self.link.try_recv().is_err(), "one reply per call");
        match first {
            ToHost::SeatToolReply {
                ok, result, error, ..
            } => (ok, result, error),
            other => panic!("{other:?}"),
        }
    }
}

/// A live host (the real CLIs) whose agents reach Sculptured's tools through the real shim and socket.
pub async fn live_host() -> (Served, Supervisor, tempfile::TempDir) {
    crate::common::live::with_tools().await
}

/// Starts a one-agent team: Claude on the cheapest model in `live::work_folder`, restricted when
/// `restricted`, whose Job is `job`. Returns the agent's seat once it runs.
pub async fn live_agent(s: &Served, job: &str, restricted: bool) -> String {
    let (status, rec) = call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Asking" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    let permission = if restricted {
        "restricted"
    } else {
        "unrestricted"
    };
    draft["agents"] = json!([{
        "id": "a", "group_id": null, "name": "A", "kind": "claude", "job": job, "folder": null,
        "x": 40, "y": 40, "advanced": { "model": null, "permission": permission },
    }]);
    crate::common::live::make_live(s, &mut draft);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::m2::start_team(s, &id).await;
    crate::common::m2::seat(&id, "a")
}

/// The first question `seat` raised, waiting up to `live::WAIT`: (id, source).
pub async fn live_question(s: &Served, seat: &str) -> (String, model::QuestionSource) {
    let e = crate::common::live::wait_event(&s.st, seat, "a question", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    })
    .await;
    let EventBody::QuestionRaised {
        question_id,
        source,
        ..
    } = e.body
    else {
        unreachable!()
    };
    (question_id, source)
}

/// The id of the first call to `tool` (a name ending in it, e.g. `mcp__rig__request_input`) on `seat`.
pub async fn live_call_of(s: &Served, seat: &str, tool: &str) -> String {
    let e = crate::common::live::wait_event(
        &s.st,
        seat,
        tool,
        |b| matches!(b, EventBody::ToolCalled { name, .. } if name.ends_with(tool)),
    )
    .await;
    let EventBody::ToolCalled { call_id, .. } = e.body else {
        unreachable!()
    };
    call_id
}

/// The result of call `call_id` on `seat`, waiting up to `live::WAIT`: (seq, is_error).
pub async fn live_result_of(s: &Served, seat: &str, call_id: &str) -> (i64, bool) {
    let e = crate::common::live::wait_event(
        &s.st,
        seat,
        "the call's result",
        |b| matches!(b, EventBody::ToolResulted { call_id: c, .. } if c == call_id),
    )
    .await;
    let EventBody::ToolResulted { is_error, .. } = e.body else {
        unreachable!()
    };
    (e.seq, is_error)
}
