mod common;
use common::m2::TOKEN;

use controld::handoffs::{CONTINUE_PREFIX, OwedMode};
use controld::launch;
use controld::store::{RigSource, Store};
use controld::views::{Settings, TeamSettings};
use futures::{SinkExt, StreamExt};
use host_proto::{FromHost, LaunchMode, PROTOCOL_VERSION, ToHost};

use model::{
    Actor, Caps, Desired, EventBody, LimitKind, Provenance, SeatLifecycle, Subject, TenureCause,
};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;

const SPEC: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
"#;

fn tables(st: &controld::AppState) -> Vec<String> {
    let store = st.store.lock().unwrap();
    let mut q = store
        .connection()
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .unwrap();
    q.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[tokio::test]
async fn the_m2_schema_and_seat_fields_exist() {
    let s = crate::common::m2::serve().await;
    for t in ["questions", "actions", "settings"] {
        assert!(tables(&s.st).contains(&t.to_string()), "missing {t}");
    }
    let rig = rigspec::compile(SPEC).unwrap();
    s.st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &RigSource::File {
                path: "/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    let seat = s.st.store.lock().unwrap().seat("dev.a@r").unwrap().unwrap();
    assert_eq!((seat.open_questions, seat.risk_count), (0, 0));
    assert!(seat.restore_offer.is_none() && seat.cwd.is_none());
}

#[tokio::test]
async fn settings_and_timeouts_resolve_in_order() {
    let s = crate::common::m2::serve().await;
    let mut store = s.st.store.lock().unwrap();
    assert_eq!(store.settings().unwrap(), Settings::default());
    assert_eq!(
        store.question_timeout_ms("t").unwrap(),
        600_000,
        "D8: 10 minutes by default"
    );
    store
        .save_settings(&Settings {
            question_timeout_min: 5,
            ..Settings::default()
        })
        .unwrap();
    assert_eq!(store.question_timeout_ms("t").unwrap(), 300_000);
    store
        .save_team_settings(
            "t",
            &TeamSettings {
                posture: None,
                question_timeout_min: Some(3),
                own_copies: false,
                host: None,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(store.question_timeout_ms("t").unwrap(), 180_000);
    assert_eq!(store.question_timeout_ms("other").unwrap(), 300_000);
    store.set_question_timeout_override(Some(4000)).unwrap();
    assert_eq!(
        store.question_timeout_ms("t").unwrap(),
        4000,
        "the test override wins"
    );
    store.set_question_timeout_override(None).unwrap();
    assert_eq!(store.question_timeout_ms("t").unwrap(), 180_000);
}

#[tokio::test]
async fn restore_offer_is_projected_and_cleared_by_a_new_tenure() {
    let s = crate::common::m2::serve().await;
    let rig = rigspec::compile(SPEC).unwrap();
    s.st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &RigSource::File {
                path: "/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    let seat = Subject::Seat("dev.a@r".into());
    let sys = Actor::System("restore".into());
    s.st.local_event(
        sys.clone(),
        seat.clone(),
        Provenance::Heuristic,
        EventBody::RestoreNeeded {
            reason: "resume failed".into(),
            can_resume: true,
        },
    )
    .unwrap();
    let offer =
        s.st.store
            .lock()
            .unwrap()
            .seat("dev.a@r")
            .unwrap()
            .unwrap()
            .restore_offer
            .unwrap();
    assert_eq!(
        (offer.reason.as_str(), offer.can_resume),
        ("resume failed", true)
    );
    let started = EventBody::TenureStarted {
        tenure: 2,
        cause: TenureCause::Fresh,
        driver: "stub".into(),
        native_session_id: None,
        caps: Caps::default(),
    };
    s.st.local_event(sys, seat, Provenance::ProtocolObserved, started)
        .unwrap();
    assert!(
        s.st.store
            .lock()
            .unwrap()
            .seat("dev.a@r")
            .unwrap()
            .unwrap()
            .restore_offer
            .is_none()
    );
}

#[tokio::test]
async fn a_limit_is_projected_and_cleared_by_the_next_turn() {
    let s = crate::common::m2::serve().await;
    let rig = rigspec::compile(SPEC).unwrap();
    s.st.store
        .lock()
        .unwrap()
        .upsert_rig(
            &rig,
            &RigSource::File {
                path: "/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    let seat = Subject::Seat("dev.a@r".into());
    let text = "You've hit your session limit · resets 4:30am (America/New_York)";
    let hit = EventBody::LimitReached {
        turn_id: Some("t1".into()),
        kind: LimitKind::Usage,
        resets: Some("4:30am".into()),
        text: text.into(),
    };
    s.st.local_event(
        Actor::System("test".into()),
        seat.clone(),
        Provenance::ProtocolObserved,
        hit,
    )
    .unwrap();
    let limit =
        s.st.store
            .lock()
            .unwrap()
            .seat("dev.a@r")
            .unwrap()
            .unwrap()
            .limit_hit
            .unwrap();
    assert_eq!(
        (limit.kind, limit.resets.as_deref(), limit.text.as_str()),
        (LimitKind::Usage, Some("4:30am"), text)
    );
    // The operator's "continue" starts the next turn: the limit is no longer the agent's state.
    s.st.local_event(
        Actor::Seat("dev.a@r".into()),
        seat,
        Provenance::ProtocolObserved,
        EventBody::TurnStarted {
            turn_id: "t2".into(),
        },
    )
    .unwrap();
    assert!(
        s.st.store
            .lock()
            .unwrap()
            .seat("dev.a@r")
            .unwrap()
            .unwrap()
            .limit_hit
            .is_none()
    );
}

#[tokio::test]
async fn relaunch_starts_a_new_tenure_with_the_given_cause_and_records_the_folder() {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    let cwd =
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .cwd
            .unwrap();
    assert_eq!(
        std::fs::canonicalize(&cwd).unwrap(),
        std::fs::canonicalize(s.home.path()).unwrap(),
        "a Practice agent without a folder runs in the home folder ({cwd})"
    );
    controld::reconciler::stop_seat(&s.st, &agent)
        .await
        .unwrap();
    crate::common::wait_for("stopped", || {
        crate::common::lifecycle(&s.st, &agent) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let l = launch::relaunch(
        &s.st,
        &agent,
        LaunchMode::Fresh,
        TenureCause::Handover,
        None,
    )
    .await
    .unwrap();
    assert_eq!(l, SeatLifecycle::Running);
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .tenure,
        2
    );
    assert!(
        crate::common::m2::events(&s.st, &agent)
            .iter()
            .any(|e| matches!(
                e.body,
                EventBody::TenureStarted {
                    tenure: 2,
                    cause: TenureCause::Handover,
                    ..
                }
            ))
    );
}

#[test]
fn continue_text_depends_on_whether_the_session_was_resumed() {
    assert_eq!(
        controld::handoffs::continue_text(OwedMode::Fresh, "Build it"),
        format!("{CONTINUE_PREFIX}\n\nBuild it")
    );
    assert_eq!(
        controld::handoffs::continue_text(OwedMode::Resumed, "Build it"),
        CONTINUE_PREFIX
    );
}

#[tokio::test]
async fn an_unknown_seat_tool_gets_a_plain_refusal() {
    let s = crate::common::m2::serve().await;
    let mut req = s.url.clone().into_client_request().unwrap();
    req.headers_mut()
        .insert(AUTHORIZATION, format!("Bearer {TOKEN}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let hello = FromHost::Hello {
        host_id: "box".into(),
        protocol: PROTOCOL_VERSION,
        drivers: vec![],
        live_seats: vec![],
        launching_seats: vec![],
        features: vec![],
        policy: None,
    };
    ws.send(Message::text(host_proto::encode(&hello)))
        .await
        .unwrap();
    let call = FromHost::SeatTool {
        call_id: "c1".into(),
        seat: "dev.a@r".into(),
        tool: "launch_missiles".into(),
        args: serde_json::json!({}),
    };
    ws.send(Message::text(host_proto::encode(&call)))
        .await
        .unwrap();
    loop {
        let Some(Ok(Message::Text(t))) = ws.next().await else {
            panic!("closed")
        };
        if let Ok(ToHost::SeatToolReply {
            call_id, ok, error, ..
        }) = host_proto::decode::<ToHost>(t.as_str())
        {
            assert_eq!(call_id, "c1");
            assert!(!ok);
            assert!(error.unwrap().contains("unknown tool"));
            break;
        }
    }
}

#[test]
fn a_store_keeps_the_new_columns_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    let rig = rigspec::compile(SPEC).unwrap();
    {
        let mut store = Store::open(&path).unwrap();
        store
            .upsert_rig(
                &rig,
                &RigSource::File {
                    path: "/r.yaml".into(),
                },
                Desired::Running,
            )
            .unwrap();
        store.set_seat_cwd("dev.a@r", "/home/x/project").unwrap();
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.seat("dev.a@r").unwrap().unwrap().cwd.as_deref(),
        Some("/home/x/project")
    );
}
