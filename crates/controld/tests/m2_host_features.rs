//! A hostd started before an upgrade keeps running (it owns the agents), so controld reads what it can
//! do from its Hello and refuses, in plain words, a command it would get wrong. A seat that is still
//! starting when a hostd reconnects is shown as starting, not as running or lost.
mod common;
use common::m2::TOKEN;

use common::m2::{Served, rid};
use controld::store::RigSource;
use futures::{SinkExt, StreamExt};
use host_proto::{
    DriverInfo, FromHost, HostCommand, LaunchParams, PROTOCOL_VERSION, ToHost, decode, features,
};

use model::{
    Actor, Answer, Caps, Desired, EventBody, PermissionPosture, Provenance, QuestionPrompt,
    QuestionSource, SeatLifecycle, Subject, TenureCause,
};
use serde_json::Value;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const OLDER: &str = "Sculptured's agent runner is from an older version. Stop all teams, then end the old agent runner: its process number is in the file hostd.pid, in the host-local folder of Sculptured's data folder. Sculptured starts the new runner within a minute.";

const SPEC: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: stub, cwd: "." }
"#;

/// Says Hello as `hello` (raw JSON, so an M1 shape can be sent) and waits for Welcome.
async fn host(s: &Served, hello: Value) -> Ws {
    let mut req = s.url.clone().into_client_request().unwrap();
    req.headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {TOKEN}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws.send(Message::text(hello.to_string())).await.unwrap();
    loop {
        let Some(Ok(Message::Text(t))) = ws.next().await else {
            panic!("closed before welcome")
        };
        if matches!(decode::<ToHost>(t.as_str()), Ok(ToHost::Welcome { .. })) {
            return ws;
        }
    }
}

/// A Hello as an M1 hostd sends it: no `features`, no `launching_seats`.
fn m1_hello(live: &[&str]) -> Value {
    let hello = FromHost::Hello {
        host_id: "local".into(),
        protocol: PROTOCOL_VERSION,
        drivers: vec![DriverInfo {
            name: "stub".into(),
            caps: Caps::default(),
        }],
        live_seats: live.iter().map(|s| s.to_string()).collect(),
        launching_seats: vec![],
        features: vec![],
        policy: None,
    };
    let mut v: Value = serde_json::from_str(&host_proto::encode(&hello)).unwrap();
    let o = v.as_object_mut().unwrap();
    o.remove("features");
    o.remove("launching_seats");
    v
}

fn hello(live: &[&str], launching: &[&str], features: &[&str]) -> Value {
    let mut v = m1_hello(live);
    v["launching_seats"] = launching.iter().map(|s| Value::from(*s)).collect();
    v["features"] = features.iter().map(|s| Value::from(*s)).collect();
    v
}

/// The commands the fake host received within `wait`.
async fn commands(ws: &mut Ws, wait: Duration) -> Vec<HostCommand> {
    let mut got = Vec::new();
    while let Ok(Some(Ok(m))) = tokio::time::timeout(wait, ws.next()).await {
        if let Message::Text(t) = m
            && let Ok(ToHost::Command { command, .. }) = host_proto::decode::<ToHost>(t.as_str())
        {
            got.push(*command);
        }
    }
    got
}

fn restricted_launch() -> HostCommand {
    let params = LaunchParams {
        cwd: "/tmp".into(),
        posture: PermissionPosture::Restricted,
        ..Default::default()
    };
    HostCommand::Launch {
        seat: "dev.a@r".into(),
        driver: "stub".into(),
        params,
    }
}

fn boot_rig(s: &Served) -> rigspec::CompiledRig {
    let rig = rigspec::compile(SPEC).unwrap();
    s.st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    rig
}

#[tokio::test]
async fn an_m1_hello_supports_nothing_new_and_a_restricted_launch_is_refused_not_sent() {
    let s = crate::common::m2::serve().await;
    let mut ws = host(&s, m1_hello(&[])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    for f in [
        features::POSTURE,
        features::RESTART,
        features::ANSWER,
        features::SEAT_TOOLS,
        features::USAGE_BASELINE,
    ] {
        assert!(!s.st.hosts.host_supports("local", f), "{f}");
    }
    assert_eq!(
        s.st.hosts.check_features("local", &[features::POSTURE]),
        Err(OLDER.to_string())
    );
    let err =
        s.st.hosts
            .command("local", restricted_launch())
            .await
            .unwrap_err();
    assert_eq!(err, OLDER);
    let answer = HostCommand::Answer {
        seat: "dev.a@r".into(),
        question_id: "q".into(),
        answer: Answer::Allow,
    };
    assert_eq!(
        s.st.hosts.command("local", answer).await.unwrap_err(),
        OLDER
    );
    assert!(
        commands(&mut ws, Duration::from_millis(300))
            .await
            .is_empty(),
        "nothing reaches an older runner"
    );
}

#[tokio::test]
async fn a_host_that_says_it_honours_posture_gets_the_restricted_launch() {
    let s = crate::common::m2::serve().await;
    let mut ws = host(&s, hello(&[], &[], &[features::POSTURE])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    assert!(s.st.hosts.host_supports("local", features::POSTURE));
    assert!(!s.st.hosts.host_supports("local", features::SEAT_TOOLS));
    assert_eq!(
        s.st.hosts.check_features("local", &[features::POSTURE]),
        Ok(())
    );
    let st = s.st.clone();
    let sent = tokio::spawn(async move { st.hosts.command("local", restricted_launch()).await });
    let got = commands(&mut ws, Duration::from_millis(300)).await;
    assert_eq!(got, vec![restricted_launch()]);
    sent.abort();
}

#[tokio::test]
async fn an_unrestricted_agent_still_launches_on_an_m1_runner() {
    let s = crate::common::m2::serve().await;
    let rig = boot_rig(&s);
    let _ws = host(&s, m1_hello(&[])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let seat = rig.seats.iter().find(|x| x.id == "dev.a@r").unwrap();
    let params = controld::launch::launch_params(&s.st, "local", "r", seat, s.home.path())
        .expect("an M1 launch needs nothing new");
    assert_eq!(params.posture, PermissionPosture::Unrestricted);
}

#[tokio::test]
async fn a_reconnect_during_a_launch_keeps_the_seat_launching() {
    let s = crate::common::m2::serve().await;
    boot_rig(&s);
    // Both agents start on the host; then the link drops mid-launch and they go stale.
    let mut first = host(&s, hello(&[], &[], &[])).await;
    for (seq, seat) in [(1, "dev.a@r"), (2, "dev.b@r")] {
        let event = host_proto::HostEvent {
            host_seq: seq,
            at_ms: 0,
            actor: Actor::System("hostd".into()),
            subject: Subject::Seat(seat.into()),
            provenance: Provenance::ProtocolObserved,
            body: EventBody::SeatLifecycleChanged {
                lifecycle: SeatLifecycle::Launching,
                reason: None,
            },
        };
        first
            .send(Message::text(host_proto::encode(&FromHost::Event {
                event,
            })))
            .await
            .unwrap();
    }
    crate::common::wait_for("both launching", || {
        crate::common::lifecycle(&s.st, "dev.b@r") == Some(SeatLifecycle::Launching)
    })
    .await;
    drop(first);
    crate::common::wait_for("a stale", || {
        crate::common::lifecycle(&s.st, "dev.a@r") == Some(SeatLifecycle::Stale)
    })
    .await;
    crate::common::wait_for("b stale", || {
        crate::common::lifecycle(&s.st, "dev.b@r") == Some(SeatLifecycle::Stale)
    })
    .await;
    // The host comes back while a is still starting; b's process is gone.
    let _ws = host(&s, hello(&["dev.a@r"], &["dev.a@r"], &[])).await;
    crate::common::wait_for("b is lost", || {
        crate::common::lifecycle(&s.st, "dev.b@r") == Some(SeatLifecycle::Attention)
    })
    .await;
    assert_eq!(
        crate::common::lifecycle(&s.st, "dev.a@r"),
        Some(SeatLifecycle::Launching),
        "starting, not running and not lost"
    );
}

#[tokio::test]
async fn a_new_connection_ends_the_evicted_one() {
    let s = crate::common::m2::serve().await;
    let mut old = host(&s, hello(&[], &[], &[])).await;
    let _new = host(&s, hello(&[], &[], &[])).await;
    let ended = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match old.next().await {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await;
    assert!(ended.is_ok(), "the evicted session is still open");
    assert!(s.st.hosts.is_connected("local"));
}

#[test]
fn an_action_is_keyed_by_seat_session_and_call() {
    // Call ids repeat across sessions (the stub's `turn-1-call`, fake-claude's `toolu_1`).
    let store = controld::store::Store::open_in_memory().unwrap();
    let insert = "INSERT INTO actions (seat, call_id, rig, tenure, called_seq, at_ms, tool, input_json) VALUES ('dev.a@r', 'toolu_1', 'r', ?1, 1, 0, 'Bash', '{}')";
    store.connection().execute(insert, [1]).unwrap();
    store
        .connection()
        .execute(insert, [2])
        .expect("the same call id in the next session is a different action");
    assert!(
        store.connection().execute(insert, [2]).is_err(),
        "the same call in the same session is one action"
    );
}

fn events_of(s: &Served, seat: &str) -> Vec<EventBody> {
    s.st.store
        .lock()
        .unwrap()
        .events_for_seat(seat, 0, 1000)
        .unwrap()
        .into_iter()
        .map(|e| e.body)
        .collect()
}

#[tokio::test]
async fn an_older_runner_gets_an_unrestricted_agent_without_the_optional_features_and_says_so() {
    let s = crate::common::m2::serve().await;
    boot_rig(&s);
    let _ws = host(&s, m1_hello(&[])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let mut params = LaunchParams {
        cwd: "/tmp".into(),
        seat_tools: Some(host_proto::SeatToolsParams {
            approve: false,
            workflow: true,
        }),
        usage_baseline: Some([1, 2, 3, 4]),
        ..Default::default()
    };
    let dropped = controld::hosts::fit_params(&s.st, "local", "dev.a@r", &mut params).unwrap();
    assert_eq!(
        dropped,
        vec![features::SEAT_TOOLS, features::USAGE_BASELINE]
    );
    assert!(params.seat_tools.is_none() && params.usage_baseline.is_none());
    let noted = events_of(&s, "dev.a@r").into_iter().find_map(|b| match b {
        EventBody::FeaturesDropped { features, note } => Some((features, note)),
        _ => None,
    });
    let (features, note) = noted.expect("the drop is recorded on the agent");
    assert_eq!(
        features,
        vec!["seat-tools".to_string(), "usage-baseline".to_string()]
    );
    assert!(
        note.starts_with(
            "This agent runs without Sculptured's agent tools and without its earlier usage count"
        ),
        "{note}"
    );
    assert!(note.contains("Stop all teams"), "{note}");

    // Posture is never dropped: a restricted agent is refused instead.
    let mut restricted = LaunchParams {
        cwd: "/tmp".into(),
        posture: PermissionPosture::Restricted,
        ..Default::default()
    };
    assert_eq!(
        controld::hosts::fit_params(&s.st, "local", "dev.b@r", &mut restricted),
        Err(OLDER.to_string())
    );
    assert_eq!(restricted.posture, PermissionPosture::Restricted);
    assert!(
        !events_of(&s, "dev.b@r")
            .iter()
            .any(|b| matches!(b, EventBody::FeaturesDropped { .. }))
    );
}

#[tokio::test]
async fn restart_is_refused_end_to_end_on_an_older_runner() {
    let s = crate::common::m2::serve().await;
    boot_rig(&s);
    let mut ws = host(&s, m1_hello(&[])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let err = controld::launch::relaunch(
        &s.st,
        "dev.a@r",
        host_proto::LaunchMode::Fresh,
        TenureCause::Handover,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        (err.status, err.code.as_ref()),
        (axum::http::StatusCode::CONFLICT, "older_runner")
    );
    assert!(
        commands(&mut ws, Duration::from_millis(300))
            .await
            .is_empty(),
        "nothing reaches the older runner"
    );
}

#[tokio::test]
async fn a_reconnect_brings_an_attention_seat_that_is_still_starting_back_to_launching() {
    let s = crate::common::m2::serve().await;
    boot_rig(&s);
    let mut first = host(&s, hello(&[], &[], &[])).await;
    let event = host_proto::HostEvent {
        host_seq: 1,
        at_ms: 0,
        actor: Actor::System("hostd".into()),
        subject: Subject::Seat("dev.a@r".into()),
        provenance: Provenance::ProtocolObserved,
        body: EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Attention,
            reason: Some("test".into()),
        },
    };
    first
        .send(Message::text(host_proto::encode(&FromHost::Event {
            event,
        })))
        .await
        .unwrap();
    crate::common::wait_for("a in attention", || {
        crate::common::lifecycle(&s.st, "dev.a@r") == Some(SeatLifecycle::Attention)
    })
    .await;
    drop(first);
    let _ws = host(&s, hello(&["dev.a@r"], &["dev.a@r"], &[])).await;
    crate::common::wait_for("a is launching", || {
        crate::common::lifecycle(&s.st, "dev.a@r") == Some(SeatLifecycle::Launching)
    })
    .await;
}

/// An older runner has no `Answer` command: answering is refused in its own words (`older_runner`, not
/// `host_unreachable`), nothing is sent, and the question is open again for when the runner is updated.
#[tokio::test]
async fn an_answer_is_refused_plainly_on_an_older_runner_and_the_question_reopens() {
    let s = crate::common::m2::serve().await;
    let rig = rigspec::compile(SPEC).unwrap();
    s.st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            Desired::Stopped,
        )
        .unwrap();
    let mut ws = host(&s, m1_hello(&[])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let seat = "dev.a@r";
    let running = EventBody::SeatLifecycleChanged {
        lifecycle: SeatLifecycle::Running,
        reason: None,
    };
    s.st.local_event(
        Actor::System("hostd".into()),
        Subject::Seat(seat.into()),
        Provenance::ProtocolObserved,
        running,
    )
    .unwrap();
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: "Which colour?".into(),
        options: vec![],
        allow_other: true,
        tool: None,
        tool_input: None,
    };
    let raised = EventBody::QuestionRaised {
        question_id: "q1".into(),
        source: QuestionSource::NativeQuestion,
        prompt,
        turn_id: None,
    };
    s.st.local_event(
        Actor::Seat(seat.into()),
        Subject::Seat(seat.into()),
        Provenance::ProtocolObserved,
        raised,
    )
    .unwrap();
    crate::common::wait_for("question open", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(seat)
            .unwrap()
            .unwrap()
            .open_questions
            == 1
    })
    .await;

    let body =
        serde_json::json!({ "request_id": rid(), "answer": { "kind": "text", "text": "Green" } });
    let (status, err) =
        crate::common::m2::call(&s.router, "POST", "/api/questions/q1/answer", body).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (axum::http::StatusCode::CONFLICT, Some("older_runner")),
        "{err}"
    );
    assert_eq!(err["error"]["message"], OLDER);
    let sent = commands(&mut ws, Duration::from_millis(300)).await;
    assert!(
        !sent.iter().any(|c| matches!(c, HostCommand::Answer { .. })),
        "nothing reaches the older runner: {sent:?}"
    );
    assert!(
        events_of(&s, seat).iter().any(
            |b| matches!(b, EventBody::QuestionAnswerFailed { reason, .. } if reason == OLDER)
        )
    );
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(seat)
            .unwrap()
            .unwrap()
            .open_questions,
        1,
        "open again"
    );
}

/// `launch::launch_params` goes through `fit_params`: an unrestricted Practice agent on an older runner
/// launches without Sculptured's tools and the drop is recorded on it; a runner that serves them keeps them.
#[tokio::test]
async fn launch_params_drop_seat_tools_on_an_older_runner_and_keep_them_on_a_current_one() {
    let s = crate::common::m2::serve().await;
    let rig = boot_rig(&s);
    let seat = rig.seats.iter().find(|x| x.id == "dev.a@r").unwrap();
    let ws = host(&s, m1_hello(&[])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let params = controld::launch::launch_params(&s.st, "local", "r", seat, s.home.path()).unwrap();
    assert_eq!(params.posture, PermissionPosture::Unrestricted);
    assert_eq!(
        params.seat_tools, None,
        "an older runner cannot serve the tools"
    );
    let dropped: Vec<Vec<String>> = events_of(&s, "dev.a@r")
        .into_iter()
        .filter_map(|b| match b {
            EventBody::FeaturesDropped { features, .. } => Some(features),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, vec![vec!["seat-tools".to_string()]]);
    drop(ws);
    crate::common::wait_for("host gone", || !s.st.hosts.is_connected("local")).await;

    let _ws = host(&s, hello(&[], &[], &[features::SEAT_TOOLS])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let params = controld::launch::launch_params(&s.st, "local", "r", seat, s.home.path()).unwrap();
    assert_eq!(
        params.seat_tools,
        Some(host_proto::SeatToolsParams {
            approve: false,
            workflow: true
        })
    );
    let drops = events_of(&s, "dev.a@r")
        .into_iter()
        .filter(|b| matches!(b, EventBody::FeaturesDropped { .. }))
        .count();
    assert_eq!(drops, 1, "nothing more is dropped on a current runner");
}

/// Concern 1: a runner that can restart but cannot seed a usage baseline gets the resume without it,
/// and the drop is recorded on the agent (never a refusal: `relaunch` fits the baseline with the rest).
#[tokio::test]
async fn relaunch_drops_the_usage_baseline_on_a_runner_that_cannot_seed_it() {
    let s = crate::common::m2::serve().await;
    boot_rig(&s);
    let mut ws = host(
        &s,
        hello(
            &[],
            &[],
            &[
                features::RESTART,
                features::POSTURE,
                features::SEAT_TOOLS,
                features::ANSWER,
            ],
        ),
    )
    .await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let st = s.st.clone();
    let mode = host_proto::LaunchMode::Resume {
        native_session_id: "stub-dev.a@r".into(),
    };
    let relaunch = tokio::spawn(async move {
        controld::launch::relaunch(
            &st,
            "dev.a@r",
            mode,
            TenureCause::Resume,
            Some([1, 2, 3, 4]),
        )
        .await
    });
    let sent = commands(&mut ws, Duration::from_millis(500)).await;
    relaunch.abort();
    let [HostCommand::Restart { params, .. }] = sent.as_slice() else {
        panic!("one restart: {sent:?}")
    };
    assert_eq!(
        params.usage_baseline, None,
        "dropped, not sent to a runner that cannot use it"
    );
    assert!(params.seat_tools.is_some());
    let dropped: Vec<Vec<String>> = events_of(&s, "dev.a@r")
        .into_iter()
        .filter_map(|b| match b {
            EventBody::FeaturesDropped { features, .. } => Some(features),
            _ => None,
        })
        .collect();
    assert_eq!(dropped, vec![vec!["usage-baseline".to_string()]]);
}

/// `relaunch`'s third `older_runner` mapping maps exactly this refusal: a command the connected runner
/// cannot do is refused before it is sent (`CommandFailure::OlderRunner`), and `command` gives its words.
/// (In `relaunch` it can only happen when the runner reconnects older between the fit and the command.)
#[tokio::test]
async fn a_command_the_runner_cannot_do_is_refused_as_older_runner() {
    let s = crate::common::m2::serve().await;
    let mut ws = host(&s, hello(&[], &[], &[features::RESTART])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let restart = || HostCommand::Restart {
        seat: "dev.a@r".into(),
        driver: "stub".into(),
        params: LaunchParams {
            cwd: "/tmp".into(),
            usage_baseline: Some([1, 2, 3, 4]),
            ..Default::default()
        },
        mode: host_proto::LaunchMode::Fresh,
        cause: TenureCause::Fresh,
    };
    let failure =
        s.st.hosts
            .command_checked("local", restart())
            .await
            .unwrap_err();
    assert_eq!(
        failure,
        controld::hosts::CommandFailure::OlderRunner(OLDER.to_string())
    );
    assert_eq!(
        s.st.hosts.command("local", restart()).await.unwrap_err(),
        OLDER,
        "the words relaunch maps to 409 older_runner"
    );
    assert!(
        commands(&mut ws, Duration::from_millis(300))
            .await
            .is_empty(),
        "nothing reaches the older runner"
    );
}

const CLAUDE_SPEC: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: claude-code, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: codex, cwd: "." }
"#;

#[tokio::test]
async fn claude_agents_leave_out_the_persons_own_setup_and_an_older_runner_says_it_cannot() {
    let s = crate::common::m2::serve().await;
    let rig = rigspec::compile(CLAUDE_SPEC).unwrap();
    s.st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    let (claude, codex) = (
        rig.seats.iter().find(|x| x.id == "dev.a@r").unwrap(),
        rig.seats.iter().find(|x| x.id == "dev.b@r").unwrap(),
    );
    let _ws = host(&s, hello(&[], &[], &[features::CLAUDE_SETUP])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let params =
        controld::launch::launch_params(&s.st, "local", "r", claude, s.home.path()).unwrap();
    assert_eq!(
        params.claude_setup,
        Some(model::ClaudeSetup::default()),
        "global setup off, project on"
    );
    let params =
        controld::launch::launch_params(&s.st, "local", "r", codex, s.home.path()).unwrap();
    assert_eq!(params.claude_setup, None, "Codex agents are not affected");
    drop(_ws);
    crate::common::wait_for("host gone", || !s.st.hosts.is_connected("local")).await;

    let _ws = host(&s, hello(&[], &[], &[])).await;
    crate::common::wait_for("host connected", || s.st.hosts.is_connected("local")).await;
    let params =
        controld::launch::launch_params(&s.st, "local", "r", claude, s.home.path()).unwrap();
    assert_eq!(
        params.claude_setup, None,
        "an older runner gets the launch without it"
    );
    let note = events_of(&s, "dev.a@r").into_iter().find_map(|b| match b {
        EventBody::FeaturesDropped { features, note }
            if features.iter().any(|f| f == "claude-setup") =>
        {
            Some(note)
        }
        _ => None,
    });
    assert!(
        note.is_some_and(|n| n.contains("leaves out your own Claude Code plugins")),
        "the agent says so"
    );
}
