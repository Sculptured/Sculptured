mod common;
#[path = "common/knobbed.rs"]
mod knobbed;

use axum::http::StatusCode;

use common::m2::{Served, rid};
use controld::store::Store;

use knobbed::{AnswerPlan, Knobbed, REFUSAL};

use model::{
    Actor, Answer, Desired, EventBody, EventRecord, MessageIntent, Provenance, QuestionPrompt,
    QuestionSource, SeatActivity, SeatLifecycle, Subject, TurnStatus,
};
use serde_json::{Value, json};

async fn ready() -> (Served, String) {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    (s, agent)
}

/// The id of the first question the agent raised after event `after`.
async fn raised(s: &Served, seat: &str, after: i64) -> String {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let found = crate::common::m2::events(&s.st, seat)
            .into_iter()
            .filter(|e| e.seq > after)
            .find_map(|e| match e.body {
                EventBody::QuestionRaised { question_id, .. } => Some(question_id),
                _ => None,
            });
        if let Some(id) = found {
            return id;
        }
        assert!(tokio::time::Instant::now() < deadline, "no question raised");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

fn answer_body(text: &str) -> Value {
    json!({ "request_id": rid(), "answer": { "kind": "text", "text": text } })
}

async fn questions_of(s: &Served, seat: &str) -> Value {
    crate::common::m2::get(&s.router, &format!("/api/seats/{seat}/questions"))
        .await
        .1
}

#[tokio::test]
async fn an_answered_question_reaches_the_agent_once() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Which colour?|Blue|Green").await;
    let q = raised(&s, &agent, 0).await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["id"], format!("q:{q}"));
    assert_eq!(inbox[0]["kind"], "question");
    assert_eq!(
        inbox[0]["question"]["prompt"]["options"][1]["label"],
        "Green"
    );
    let open = inbox[0]["question"]["opened_at_ms"].as_i64().unwrap();
    assert_eq!(
        inbox[0]["question"]["deadline_ms"].as_i64().unwrap() - open,
        600_000,
        "D8 default"
    );
    crate::common::wait_for("waiting for input", || {
        let v = s.st.store.lock().unwrap().seat(&agent).unwrap().unwrap();
        (v.open_questions, v.activity) == (1, SeatActivity::WaitingInput)
    })
    .await;

    let body = answer_body("Green");
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "answered");
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: Green"),
    )
    .await;
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/questions/{q}/answer"),
            body
        )
        .await
        .0,
        StatusCode::OK,
        "same request id replays"
    );
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("Blue"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "question_closed");
    assert_eq!(
        crate::common::m2::agent_texts(&s.st, &agent)
            .iter()
            .filter(|t| t.starts_with("stub got answer"))
            .count(),
        1
    );
    assert!(
        crate::common::m2::get(&s.router, "/api/inbox")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions,
        0
    );
}

#[tokio::test]
async fn an_answer_must_fit_the_question() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Pick|A|B").await;
    let q = raised(&s, &agent, 0).await;
    for bad in [
        json!({ "kind": "allow" }),
        json!({ "kind": "text", "text": "  " }),
        json!({ "kind": "expired" }),
    ] {
        let (status, err) = crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/questions/{q}/answer"),
            json!({ "request_id": rid(), "answer": bad }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
        assert_eq!(err["error"]["code"], "bad_answer");
    }
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/acknowledge"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("question_open"))
    );
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/questions/q-nope/answer",
        answer_body("A"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_expired_question_records_the_agents_own_decision() {
    let (s, agent) = ready().await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(300))
        .unwrap();
    crate::common::m2::say(&s, &agent, "ask:Pick one|Red|Blue").await;
    let q = raised(&s, &agent, 0).await;
    crate::common::m2::wait_event(&s.st, &agent, "the decision", |b| {
        matches!(b, EventBody::QuestionDecided { .. })
    })
    .await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(v["state"], "expired");
    assert_eq!(v["decision"]["text"], "No answer came, so I decided: Red");
    assert_eq!(v["decision"]["source"], "next-message");
    assert_eq!(v["decision"]["provenance"], "agent-reported");
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["kind"], "expired");
    let expiry = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .find(|e| matches!(e.body, EventBody::QuestionExpired { .. }))
        .unwrap();
    assert_eq!(expiry.actor, Actor::System("timeout".into()));
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/acknowledge"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        crate::common::m2::get(&s.router, "/api/inbox")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_silent_agent_leaves_its_decision_unknown() {
    let (s, agent) = ready().await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(300))
        .unwrap();
    crate::common::m2::say(&s, &agent, "ask-silent:Anything?").await;
    raised(&s, &agent, 0).await;
    crate::common::m2::wait_event(&s.st, &agent, "the decision", |b| {
        matches!(b, EventBody::QuestionDecided { .. })
    })
    .await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(
        (
            v["decision"]["text"].as_str(),
            v["decision"]["source"].as_str()
        ),
        (Some("unknown"), Some("unknown"))
    );
    assert_eq!(v["decision"]["provenance"], "heuristic");
}

#[tokio::test]
async fn a_late_answer_is_an_overriding_instruction() {
    let (s, agent) = ready().await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(300))
        .unwrap();
    crate::common::m2::say(&s, &agent, "ask:Pick one|Red|Blue").await;
    let q = raised(&s, &agent, 0).await;
    crate::common::m2::wait_event(&s.st, &agent, "the decision", |b| {
        matches!(b, EventBody::QuestionDecided { .. })
    })
    .await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("Blue"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "expired");
    assert_eq!(v["late_answer"], json!({ "kind": "text", "text": "Blue" }));
    assert_eq!(
        v["acknowledged"], true,
        "acting on an expired question is also seeing it"
    );
    let late = crate::common::m2::wait_event(&s.st, &agent, "the override", |b| matches!(b, EventBody::MessageQueued { intent: MessageIntent::Instruct, text, .. } if text.starts_with("Late answer from the operator"))).await;
    assert_eq!(late.actor, Actor::Human("local".into()));
    // One override only: a further late answer is refused and sends nothing.
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("Red"),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("question_closed")),
        "{err}"
    );
    let overrides = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .filter(|e| matches!(&e.body, EventBody::MessageQueued { text, .. } if text.starts_with("Late answer from the operator")))
        .count();
    assert_eq!(overrides, 1);
    assert_eq!(
        questions_of(&s, &agent).await[0]["late_answer"]["text"],
        "Blue",
        "the first override stands"
    );
}

/// Review Focus #2: an answer and the timeout firing at once — exactly one resolution reaches the agent.
/// On a multi-thread runtime, with a driver that counts the answer calls reaching the agent's program.
/// Rounds alternate which side holds the question first:
/// - expiry first: a test hook pauses `expire` inside its claim, between its "still open?" check and
///   its record (the window the claim guards), and the answer arrives meanwhile;
/// - answer first: the driver slows the answer's delivery, and the expiry fires meanwhile.
/// Without the claim in `expire`, an expiry-first round delivers twice and records both an answer and an
/// expiry for one question.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn answer_racing_the_timeout_has_one_winner() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let knobs = Knobbed {
        then: AnswerPlan::Delay(std::time::Duration::from_millis(150)),
        ..Knobbed::default()
    };
    let calls = knobs.calls.clone();
    let (s, agent) = crate::knobbed::ready_with(knobs).await;
    let paused: Arc<Mutex<Vec<String>>> = Arc::default();
    let inside = Arc::new(AtomicBool::new(false));
    let (hook_ids, hook_inside) = (paused.clone(), inside.clone());
    *controld::seams::IN_EXPIRE_BEFORE_RECORD.lock().unwrap() = Some(Arc::new(move |id: &str| {
        if hook_ids.lock().unwrap().iter().any(|p| p == id) {
            hook_inside.store(true, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }));
    let mut states = Vec::new();
    for round in 0..8 {
        let expiry_first = round % 2 == 0;
        let after = s.st.store.lock().unwrap().last_seq().unwrap();
        crate::common::m2::say(&s, &agent, &format!("ask:Round {round}?|Red|Green")).await;
        let q = raised(&s, &agent, after).await;
        let before = calls.load(Ordering::SeqCst);
        let uri = format!("/api/questions/{q}/answer");
        let (status, v) = if expiry_first {
            paused.lock().unwrap().push(q.clone());
            inside.store(false, Ordering::SeqCst);
            let (st, id) = (s.st.clone(), q.clone());
            let expiry = tokio::spawn(async move { controld::questions::expire(&st, &id).await });
            crate::common::wait_for("the expiry to hold the question", || {
                inside.load(Ordering::SeqCst)
            })
            .await;
            let answered =
                crate::common::m2::call(&s.router, "POST", &uri, answer_body("Green")).await;
            expiry.await.unwrap();
            answered
        } else {
            let (router, uri) = (s.router.clone(), uri.clone());
            let answer = tokio::spawn(async move {
                crate::common::m2::call(&router, "POST", &uri, answer_body("Green")).await
            });
            crate::common::m2::wait_event(&s.st, &agent, "the answer, delivering", |b| {
                matches!(b, EventBody::QuestionAnswered { question_id, late: false, .. } if *question_id == q)
            })
            .await;
            controld::questions::expire(&s.st, &q).await;
            answer.await.unwrap()
        };
        assert_eq!(status, StatusCode::OK, "{v}");
        crate::common::wait_for("the turn to end", || {
            let seat = s.st.store.lock().unwrap().seat(&agent).unwrap().unwrap();
            seat.current_turn.is_none()
                && seat.activity == SeatActivity::Idle
                && seat.open_questions == 0
        })
        .await;
        assert_eq!(
            calls.load(Ordering::SeqCst) - before,
            1,
            "round {round}: exactly one answer reached the agent's program"
        );
        let replies = crate::common::m2::events(&s.st, &agent)
            .into_iter()
            .filter(|e| e.seq > after)
            .filter(|e| matches!(&e.body, EventBody::AgentText { text, .. } if text.starts_with("stub got answer") || text.starts_with("No answer came")))
            .count();
        assert_eq!(
            replies, 1,
            "round {round}: the agent got exactly one resolution"
        );
        let v = questions_of(&s, &agent).await[0].clone();
        match (v["state"].as_str(), expiry_first) {
            (Some("answered"), false) => assert!(v["expired_seq"].is_null()),
            (Some("expired"), true) => assert_eq!(
                v["late_answer"]["text"], "Green",
                "the losing answer became a late override"
            ),
            other => panic!("round {round}: {other:?}"),
        }
        states.push(v["state"].as_str().unwrap().to_string());
    }
    *controld::seams::IN_EXPIRE_BEFORE_RECORD.lock().unwrap() = None;
    // Over the whole log: no question was both answered (on time) and expired.
    let all = crate::common::m2::events(&s.st, &agent);
    let answered: Vec<&str> = all
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::QuestionAnswered {
                question_id,
                late: false,
                ..
            } => Some(question_id.as_str()),
            _ => None,
        })
        .collect();
    let expired: Vec<&str> = all
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::QuestionExpired { question_id } => Some(question_id.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !answered.iter().any(|a| expired.contains(a)),
        "answered {answered:?}, expired {expired:?}"
    );
    assert_eq!(
        states.iter().filter(|x| *x == "expired").count(),
        4,
        "{states:?}"
    );
}

#[tokio::test]
async fn a_process_end_withdraws_its_open_question() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Still there?").await;
    let q = raised(&s, &agent, 0).await;
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{agent}/stop"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    crate::common::m2::wait_event(&s.st, &agent, "stopped", |b| {
        matches!(b, EventBody::TenureEnded { .. })
    })
    .await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some(q.as_str()), Some("withdrawn"))
    );
    // Stop first ends the hanging turn as Interrupted (the stub does), which withdraws the question
    // before TenureEnded arrives; either honest reason is fine, as long as it is withdrawn.
    let reason = v["withdrawn_reason"].as_str().unwrap();
    assert!(
        reason.contains("process ended") || reason.contains("interrupted"),
        "{reason}"
    );
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions,
        0
    );
}

#[tokio::test]
async fn an_interrupted_turn_withdraws_its_question() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Wait?").await;
    raised(&s, &agent, 0).await;
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{agent}/interrupt"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    crate::common::m2::wait_event(&s.st, &agent, "withdrawn", |b| {
        matches!(
            b,
            EventBody::TurnEnded {
                status: TurnStatus::Interrupted,
                ..
            }
        )
    })
    .await;
    crate::common::wait_for("withdrawn", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions
            == 0
    })
    .await;
    assert_eq!(questions_of(&s, &agent).await[0]["state"], "withdrawn");
}

/// One stub agent `dev.a@o`, for a store prepared before controld starts.
const ONE_STUB: &str = r#"
version: "0.2"
name: o
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
"#;

/// Review Focus #4 (controld half): a question nobody can answer any more is withdrawn on start.
#[tokio::test]
async fn orphaned_questions_are_withdrawn_on_start() {
    let mut store = Store::open_in_memory().unwrap();
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: "Which?".into(),
        options: vec![],
        allow_other: true,
        tool: None,
        tool_input: None,
    };
    let orphan = EventBody::QuestionRaised {
        question_id: "q-orphan".into(),
        source: QuestionSource::RequestInput,
        prompt,
        turn_id: None,
    };
    store
        .append_local(
            Actor::Seat("team.a@t".into()),
            Subject::Seat("team.a@t".into()),
            Provenance::AgentReported,
            orphan,
        )
        .unwrap();
    // An `approve` question (Claude's permission-prompt seat tool: stored NativePermission, recorded
    // agent-reported) of an agent that still runs: its waiter died with the old controld as well.
    let rig = rigspec::compile(ONE_STUB).unwrap();
    store
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/o.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    let running = EventBody::SeatLifecycleChanged {
        lifecycle: SeatLifecycle::Running,
        reason: None,
    };
    store
        .append_local(
            Actor::System("test".into()),
            Subject::Seat("dev.a@o".into()),
            Provenance::ProtocolObserved,
            running,
        )
        .unwrap();
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: "Allow Bash to run?".into(),
        options: vec![],
        allow_other: false,
        tool: Some("Bash".into()),
        tool_input: Some(json!({ "command": "ls" })),
    };
    let approve = EventBody::QuestionRaised {
        question_id: "q-approve".into(),
        source: QuestionSource::NativePermission,
        prompt,
        turn_id: None,
    };
    store
        .append_local(
            Actor::Seat("dev.a@o".into()),
            Subject::Seat("dev.a@o".into()),
            Provenance::AgentReported,
            approve,
        )
        .unwrap();

    let s = crate::common::m2::serve_with_store(store).await;
    let e = crate::common::m2::wait_event(&s.st, "team.a@t", "withdrawn", |b| matches!(b, EventBody::QuestionWithdrawn { question_id, .. } if question_id == "q-orphan")).await;
    let EventBody::QuestionWithdrawn { reason, .. } = e.body else {
        unreachable!()
    };
    assert!(reason.contains("restarted"), "{reason}");
    let e = crate::common::m2::wait_event(&s.st, "dev.a@o", "approve withdrawn", |b| matches!(b, EventBody::QuestionWithdrawn { question_id, .. } if question_id == "q-approve")).await;
    let EventBody::QuestionWithdrawn { reason, .. } = e.body else {
        unreachable!()
    };
    assert!(
        reason.contains("restarted"),
        "a seat-tool question of a running agent has no waiter either: {reason}"
    );
    let a = s.st.store.lock().unwrap().seat("dev.a@o").unwrap().unwrap();
    assert_eq!((a.lifecycle, a.open_questions), (SeatLifecycle::Running, 0));
    assert!(
        crate::common::m2::get(&s.router, "/api/inbox")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// A process lost without a TenureEnded (hostd restarted, the machine slept) leaves only an
/// `attention` lifecycle; its open question is withdrawn, not left open with buttons that fail.
#[tokio::test]
async fn a_lost_process_withdraws_its_open_question() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Still there?").await;
    let q = raised(&s, &agent, 0).await;
    let lost = EventBody::SeatLifecycleChanged {
        lifecycle: SeatLifecycle::Attention,
        reason: Some("lost with its host".into()),
    };
    s.st.local_event(
        Actor::System("test".into()),
        Subject::Seat(agent.clone()),
        Provenance::Heuristic,
        lost,
    )
    .unwrap();
    crate::common::wait_for("withdrawn", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions
            == 0
    })
    .await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some(q.as_str()), Some("withdrawn"))
    );
    assert!(
        v["withdrawn_reason"]
            .as_str()
            .unwrap()
            .contains("process ended")
    );
}

#[tokio::test]
async fn agents_that_need_attention_or_are_stuck_are_in_the_inbox() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "crash").await;
    crate::common::wait_for("attention", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .lifecycle
            == SeatLifecycle::Attention
    })
    .await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["id"], format!("seat:{agent}"));
    assert_eq!(inbox[0]["kind"], "attention");
    assert_eq!(inbox[0]["seat_view"]["id"], agent.as_str());

    let s2 = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s2).await;
    let id = crate::common::m2::practice_chain(
        &s2,
        "Stuck chain",
        &[
            ("planner", "Planner", "hang"),
            ("builder", "Builder", "Build it"),
        ],
        &[("planner", "builder")],
    )
    .await;
    crate::common::m2::start_team(&s2, &id).await;
    let planner = crate::common::m2::seat(&id, "planner");
    crate::common::m2::wait_event(&s2.st, &planner, "the Job's turn", |b| {
        matches!(b, EventBody::MessageConsumed { .. })
    })
    .await;
    let (status, _) = crate::common::m2::call(
        &s2.router,
        "POST",
        &format!("/api/seats/{planner}/interrupt"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    crate::common::wait_for("stuck", || {
        s2.st
            .store
            .lock()
            .unwrap()
            .seat(&planner)
            .unwrap()
            .unwrap()
            .handoff_stuck
            .is_some()
    })
    .await;
    let (_, inbox) = crate::common::m2::get(&s2.router, "/api/inbox").await;
    assert!(
        inbox
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "stuck" && i["seat"] == planner.as_str()),
        "{inbox}"
    );
}

/// The user's real run: a turn ended on the CLI's usage limit. It waits in Needs you with the CLI's words;
/// its work stays unfinished (no handoff), nothing retries it, and the operator's "continue" goes on.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_usage_limit_waits_in_needs_you_and_is_never_retried() {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_chain(
        &s,
        "Limit chain",
        &[
            ("planner", "Planner", "limit"),
            ("builder", "Builder", "Build it"),
        ],
        &[("planner", "builder")],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let (planner, builder) = (
        crate::common::m2::seat(&id, "planner"),
        crate::common::m2::seat(&id, "builder"),
    );
    crate::common::wait_for("the limit", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&planner)
            .unwrap()
            .unwrap()
            .limit_hit
            .is_some()
    })
    .await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    let item = inbox
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["seat"] == planner.as_str())
        .expect("in Needs you")
        .clone();
    assert_eq!(item["kind"], "limit");
    assert_eq!(item["seat_view"]["limit_hit"]["resets"], "4:30am");
    assert_eq!(
        item["seat_view"]["limit_hit"]["text"],
        "You've hit your session limit · resets 4:30am (America/New_York)"
    );
    // No automatic retry and no handoff: the failed turn is not a work turn. Watched for 3 s (twelve
    // sweeper ticks), failing at the first retry, message or handoff that appears.
    let retry = |b: &EventBody| {
        matches!(
            b,
            EventBody::TurnStarted { .. } | EventBody::MessageQueued { .. }
        )
    };
    let limit_seq = crate::common::m2::events(&s.st, &planner)
        .into_iter()
        .find(|e| matches!(e.body, EventBody::LimitReached { .. }))
        .unwrap()
        .seq;
    let watch_until = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while tokio::time::Instant::now() < watch_until {
        let after: Vec<_> = crate::common::m2::events(&s.st, &planner)
            .into_iter()
            .filter(|e| e.seq > limit_seq && retry(&e.body))
            .collect();
        assert!(
            after.is_empty(),
            "nothing may retry the limited turn: {after:?}"
        );
        assert!(
            !crate::common::m2::events(&s.st, &builder)
                .into_iter()
                .any(|e| retry(&e.body)),
            "no handoff for unfinished work"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    // The operator sends "continue" after the reset: the limit clears and the work goes on.
    crate::common::m2::say(&s, &planner, "continue").await;
    crate::common::wait_for("the limit cleared", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&planner)
            .unwrap()
            .unwrap()
            .limit_hit
            .is_none()
    })
    .await;
    crate::common::m2::wait_event(&s.st, &builder, "the handoff", |b| {
        matches!(b, EventBody::MessageQueued { .. })
    })
    .await;
    // Up to the next turn, the only new thing is the operator's message: that turn reads it, not a retry.
    let after: Vec<_> = crate::common::m2::events(&s.st, &planner)
        .into_iter()
        .filter(|e| e.seq > limit_seq)
        .collect();
    let operator = after
        .iter()
        .find_map(|e| match (&e.actor, &e.body) {
            (
                Actor::Human(_),
                EventBody::MessageQueued {
                    message_id, text, ..
                },
            ) if text == "continue" => Some(message_id.clone()),
            _ => None,
        })
        .expect("the operator's continue");
    let first_new = after.iter().find(|e| retry(&e.body)).unwrap();
    assert!(
        matches!(&first_new.body, EventBody::MessageQueued { message_id, .. } if *message_id == operator),
        "{first_new:?}"
    );
    let consumed = after.iter().find_map(|e| match &e.body {
        EventBody::MessageConsumed { message_id, .. } => Some(message_id.clone()),
        _ => None,
    });
    assert_eq!(
        consumed,
        Some(operator),
        "the next turn read the operator's message"
    );
    assert_eq!(
        after
            .iter()
            .filter(|e| matches!(e.body, EventBody::TurnStarted { .. }))
            .count(),
        1
    );
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert!(
        !inbox
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "limit"),
        "{inbox}"
    );
}

// ---- Fix round 1 -------------------------------------------------------------------------------

fn seq_of(s: &Served, seat: &str, what: &str, pred: impl Fn(&EventBody) -> bool) -> i64 {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .find(|e| pred(&e.body))
        .unwrap_or_else(|| panic!("no {what}"))
        .seq
}

/// Ruling Q1: the answer is recorded before it is delivered, so the log reads raised → answered → reply,
/// and the program's own "resolved" notice that follows withdraws nothing.
#[tokio::test]
async fn an_answer_is_recorded_before_the_agent_hears_it() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Which colour?|Blue|Green").await;
    let q = raised(&s, &agent, 0).await;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("Green"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: Green"),
    )
    .await;
    let raised_at = seq_of(&s, &agent, "raised", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    });
    let answered = seq_of(&s, &agent, "answered", |b| {
        matches!(b, EventBody::QuestionAnswered { .. })
    });
    let resolved = seq_of(&s, &agent, "resolved", |b| {
        matches!(b, EventBody::QuestionResolved { .. })
    });
    let reply = seq_of(
        &s,
        &agent,
        "reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text.starts_with("stub got answer")),
    );
    assert!(
        raised_at < answered && answered < resolved && resolved < reply,
        "{raised_at} {answered} {resolved} {reply}"
    );
    assert_eq!(questions_of(&s, &agent).await[0]["state"], "answered");
}

/// Ruling (Important 1): a second answer waits while the first one is being delivered; when the agent
/// refuses the first, the question is open again and the second one is delivered, never `question_closed`.
#[tokio::test]
async fn a_second_answer_waits_while_the_first_one_fails() {
    let knobs = Knobbed {
        plan: vec![AnswerPlan::Refuse(std::time::Duration::from_millis(600))],
        ..Knobbed::default()
    };
    let (s, agent) = crate::knobbed::ready_with(knobs).await;
    crate::common::m2::say(&s, &agent, "ask:Pick|A|B").await;
    let q = raised(&s, &agent, 0).await;
    let uri = format!("/api/questions/{q}/answer");
    let first = crate::common::m2::call(&s.router, "POST", &uri, answer_body("A"));
    let second = async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        crate::common::m2::call(&s.router, "POST", &uri, answer_body("B")).await
    };
    let ((s1, v1), (s2, v2)) = tokio::join!(first, second);
    assert_eq!(
        (s1, v1["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("answer_refused")),
        "{v1}"
    );
    assert!(
        v1["error"]["message"].as_str().unwrap().contains(REFUSAL),
        "the agent's words: {v1}"
    );
    assert_eq!(s2, StatusCode::OK, "{v2}");
    assert_eq!(
        (v2["state"].as_str(), v2["answer"]["text"].as_str()),
        (Some("answered"), Some("B"))
    );
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: B"),
    )
    .await;
    assert_eq!(
        crate::common::m2::agent_texts(&s.st, &agent)
            .iter()
            .filter(|t| t.starts_with("stub got answer"))
            .count(),
        1
    );
    let trail: Vec<String> = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::QuestionAnswered {
                answer: Answer::Text { text },
                ..
            } => Some(format!("answered {text}")),
            EventBody::QuestionAnswerFailed { .. } => Some("failed".into()),
            _ => None,
        })
        .collect();
    assert_eq!(trail, ["answered A", "failed", "answered B"]);
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions,
        0
    );
}

#[tokio::test]
async fn a_failed_turn_withdraws_its_question() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Wait?").await;
    let q = raised(&s, &agent, 0).await;
    let turn = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::QuestionRaised { turn_id, .. } => turn_id,
            _ => None,
        })
        .unwrap();
    let failed = EventBody::TurnEnded {
        turn_id: turn,
        status: TurnStatus::Failed,
    };
    s.st.local_event(
        Actor::Seat(agent.clone()),
        Subject::Seat(agent.clone()),
        Provenance::ProtocolObserved,
        failed,
    )
    .unwrap();
    crate::common::wait_for("withdrawn", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions
            == 0
    })
    .await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some(q.as_str()), Some("withdrawn"))
    );
    assert!(
        v["withdrawn_reason"]
            .as_str()
            .unwrap()
            .contains("turn failed"),
        "{v}"
    );
}

/// The program settled the question itself (Codex can): it is withdrawn, and an answer is refused.
#[tokio::test]
async fn a_question_the_program_settled_itself_is_withdrawn() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Wait?").await;
    let q = raised(&s, &agent, 0).await;
    let settled = EventBody::QuestionResolved {
        question_id: q.clone(),
    };
    s.st.local_event(
        Actor::Seat(agent.clone()),
        Subject::Seat(agent.clone()),
        Provenance::ProtocolObserved,
        settled,
    )
    .unwrap();
    crate::common::wait_for("withdrawn", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions
            == 0
    })
    .await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(v["state"], "withdrawn");
    assert!(
        v["withdrawn_reason"].as_str().unwrap().contains("settled"),
        "{v}"
    );
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("now"),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("question_closed"))
    );
}

/// D8 at the server: an unanswered permission prompt is allowed, and that is recorded as observed.
#[tokio::test]
async fn an_unanswered_permission_prompt_is_allowed_and_recorded() {
    let (s, agent) = crate::knobbed::ready_with(Knobbed {
        restricted: true,
        ..Knobbed::default()
    })
    .await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(1500))
        .unwrap();
    crate::common::m2::say(&s, &agent, "perm:make").await;
    let q = raised(&s, &agent, 0).await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(
        (inbox[0]["id"].as_str(), inbox[0]["kind"].as_str()),
        (Some(format!("q:{q}").as_str()), Some("permission"))
    );
    let decided = crate::common::m2::wait_event(&s.st, &agent, "the decision", |b| {
        matches!(b, EventBody::QuestionDecided { .. })
    })
    .await;
    assert_eq!(
        (decided.actor, decided.provenance),
        (
            Actor::System("timeout".into()),
            Provenance::ProtocolObserved
        )
    );
    let expired = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .find(|e| matches!(e.body, EventBody::QuestionExpired { .. }))
        .unwrap();
    assert_eq!(expired.actor, Actor::System("timeout".into()));
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(v["state"], "expired");
    assert_eq!(v["decision"]["source"], "allowed-on-timeout");
    assert_eq!(v["decision"]["provenance"], "protocol-observed");
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the command ran",
        |b| matches!(b, EventBody::ToolCalled { input, .. } if input["command"] == "make"),
    )
    .await;
}

#[tokio::test]
async fn permission_prompts_and_closed_choices_take_only_fitting_answers() {
    let (s, agent) = crate::knobbed::ready_with(Knobbed {
        restricted: true,
        ..Knobbed::default()
    })
    .await;
    crate::common::m2::say(&s, &agent, "perm:ls").await;
    let q = raised(&s, &agent, 0).await;
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("yes"),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("bad_answer")),
        "text to a permission prompt"
    );
    let allow = json!({ "request_id": rid(), "answer": { "kind": "allow" } });
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/questions/{q}/answer"),
            allow
        )
        .await
        .0,
        StatusCode::OK
    );
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "ls ran",
        |b| matches!(b, EventBody::ToolCalled { input, .. } if input["command"] == "ls"),
    )
    .await;

    let after = s.st.store.lock().unwrap().last_seq().unwrap();
    crate::common::m2::say(&s, &agent, "ask-only:Pick|A|B").await;
    let q = raised(&s, &agent, after).await;
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("C"),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("bad_answer")),
        "not one of the choices"
    );
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("B"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: B"),
    )
    .await;
}

/// A link blip (`stale`) withdraws nothing, and the timeout waits until the agent runs again.
#[tokio::test]
async fn a_stale_link_withdraws_nothing_and_holds_the_timeout() {
    let (s, agent) = ready().await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(800))
        .unwrap();
    crate::common::m2::say(&s, &agent, "ask:Wait?|Stay").await;
    let q = raised(&s, &agent, 0).await;
    let lifecycle = |l: SeatLifecycle| EventBody::SeatLifecycleChanged {
        lifecycle: l,
        reason: None,
    };
    s.st.local_event(
        Actor::System("test".into()),
        Subject::Seat(agent.clone()),
        Provenance::Heuristic,
        lifecycle(SeatLifecycle::Stale),
    )
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some(q.as_str()), Some("open")),
        "past its deadline, but the agent is away"
    );
    assert!(
        !crate::common::m2::events(&s.st, &agent)
            .into_iter()
            .any(|e| matches!(e.body, EventBody::QuestionExpired { .. }))
    );
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .open_questions,
        1
    );
    s.st.local_event(
        Actor::System("test".into()),
        Subject::Seat(agent.clone()),
        Provenance::Heuristic,
        lifecycle(SeatLifecycle::Running),
    )
    .unwrap();
    crate::common::m2::wait_event(&s.st, &agent, "the expiry", |b| {
        matches!(b, EventBody::QuestionExpired { .. })
    })
    .await;
    crate::common::m2::wait_event(&s.st, &agent, "the decision", |b| matches!(b, EventBody::AgentText { text, .. } if text == "No answer came, so I decided: Stay")).await;
}

/// Two stub agents: `dev.a@o` (set running below) and `dev.b@o` (never started).
const TWO_STUBS: &str = r#"
version: "0.2"
name: o
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: stub, cwd: "." }
"#;

/// Review Focus #4: a native question of a running agent survives a controld restart (hostd still holds
/// it and can take the answer); a native question of an agent that no longer runs is withdrawn.
#[tokio::test]
async fn a_restart_keeps_native_questions_of_running_agents() {
    let mut store = Store::open_in_memory().unwrap();
    let rig = rigspec::compile(TWO_STUBS).unwrap();
    store
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/o.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    let running = EventBody::SeatLifecycleChanged {
        lifecycle: SeatLifecycle::Running,
        reason: None,
    };
    store
        .append_local(
            Actor::System("test".into()),
            Subject::Seat("dev.a@o".into()),
            Provenance::ProtocolObserved,
            running,
        )
        .unwrap();
    let native = |id: &str| {
        let prompt = QuestionPrompt {
            urgency: None,
            header: None,
            text: "Which?".into(),
            options: vec![],
            allow_other: true,
            tool: None,
            tool_input: None,
        };
        EventBody::QuestionRaised {
            question_id: id.into(),
            source: QuestionSource::NativeQuestion,
            prompt,
            turn_id: None,
        }
    };
    // Raised through hostd's mapping: protocol-observed. The running agent's comes first.
    store
        .append_local(
            Actor::Seat("dev.a@o".into()),
            Subject::Seat("dev.a@o".into()),
            Provenance::ProtocolObserved,
            native("q-native"),
        )
        .unwrap();
    store
        .append_local(
            Actor::Seat("dev.b@o".into()),
            Subject::Seat("dev.b@o".into()),
            Provenance::ProtocolObserved,
            native("q-gone"),
        )
        .unwrap();

    let s = crate::common::m2::serve_with_store(store).await;
    let e = crate::common::m2::wait_event(&s.st, "dev.b@o", "withdrawn", |b| matches!(b, EventBody::QuestionWithdrawn { question_id, .. } if question_id == "q-gone")).await;
    let EventBody::QuestionWithdrawn { reason, .. } = e.body else {
        unreachable!()
    };
    assert!(reason.contains("process ended"), "{reason}");
    let v = &questions_of(&s, "dev.a@o").await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some("q-native"), Some("open")),
        "{v}"
    );
    assert!(
        !crate::common::m2::events(&s.st, "dev.a@o")
            .into_iter()
            .any(|e| matches!(e.body, EventBody::QuestionWithdrawn { .. }))
    );
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    let ids: Vec<&str> = inbox
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .filter(|i| i.starts_with("q:"))
        .collect();
    assert_eq!(ids, ["q:q-native"]);
}

/// Needs you lists agents by when they started needing the operator, not by their latest event, so a
/// newer event on an older item never reorders the list.
#[tokio::test]
async fn needs_you_lists_agents_by_when_they_started_needing_you() {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_chain(
        &s,
        "Two heads",
        &[("x", "X", "hello"), ("y", "Y", "hello")],
        &[],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let (x, y) = (
        crate::common::m2::seat(&id, "x"),
        crate::common::m2::seat(&id, "y"),
    );
    let attention = |a: &str| {
        s.st.store
            .lock()
            .unwrap()
            .seat(a)
            .unwrap()
            .unwrap()
            .lifecycle
            == SeatLifecycle::Attention
    };
    crate::common::m2::say(&s, &x, "crash").await;
    crate::common::wait_for("x in attention", || attention(&x)).await;
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    crate::common::m2::say(&s, &y, "crash").await;
    crate::common::wait_for("y in attention", || attention(&y)).await;
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    // A later event on x (here: attention again, with a new reason) must not move it behind y.
    let again = EventBody::SeatLifecycleChanged {
        lifecycle: SeatLifecycle::Attention,
        reason: Some("still lost".into()),
    };
    s.st.local_event(
        Actor::System("test".into()),
        Subject::Seat(x.clone()),
        Provenance::Heuristic,
        again,
    )
    .unwrap();
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    let ids: Vec<&str> = inbox
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert_eq!(ids, [format!("seat:{x}"), format!("seat:{y}")], "{inbox}");
    assert!(inbox[0]["opened_at_ms"].as_i64() < inbox[1]["opened_at_ms"].as_i64());
}

// ---- Fix round 2 -------------------------------------------------------------------------------

fn put(store: &mut Store, seat: &str, body: EventBody) -> EventRecord {
    store
        .append_local(
            Actor::Seat(seat.into()),
            Subject::Seat(seat.into()),
            Provenance::ProtocolObserved,
            body,
        )
        .unwrap()
        .event
}

fn native_question(id: &str, turn: &str) -> EventBody {
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: "Which?".into(),
        options: vec![],
        allow_other: true,
        tool: None,
        tool_input: None,
    };
    EventBody::QuestionRaised {
        question_id: id.into(),
        source: QuestionSource::NativeQuestion,
        prompt,
        turn_id: Some(turn.into()),
    }
}

fn answered(id: &str) -> EventBody {
    EventBody::QuestionAnswered {
        question_id: id.into(),
        answer: Answer::Text { text: "yes".into() },
        late: false,
    }
}

fn answer_failed(id: &str) -> EventBody {
    EventBody::QuestionAnswerFailed {
        question_id: id.into(),
        reason: "Sculptured's agent runner is not connected".into(),
    }
}

/// A failed delivery opens the question again, with a fresh deadline (the team's full timeout, counted
/// from the failure), only while the agent can still take an answer. Otherwise it is withdrawn with the
/// reason: the turn ended, the process ended, or the program had already settled it itself (the Codex
/// "ghost": its own `QuestionResolved` landed between the record and the failure).
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn a_failed_answer_reopens_only_while_the_agent_can_take_it() {
    let mut store = Store::open_in_memory().unwrap();
    let rig = rigspec::compile(TWO_STUBS).unwrap();
    store
        .upsert_rig(
            &rig,
            &controld::store::RigSource::File {
                path: "/specs/o.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    let running = || EventBody::SeatLifecycleChanged {
        lifecycle: SeatLifecycle::Running,
        reason: None,
    };
    let (a, b) = ("dev.a@o", "dev.b@o");

    put(&mut store, a, running());
    put(
        &mut store,
        a,
        EventBody::TurnStarted {
            turn_id: "t1".into(),
        },
    );
    let raised_at = put(&mut store, a, native_question("q-reopen", "t1")).at_ms;
    put(&mut store, a, answered("q-reopen"));
    std::thread::sleep(std::time::Duration::from_millis(20));
    let failed_at = put(&mut store, a, answer_failed("q-reopen")).at_ms;
    assert!(failed_at > raised_at);

    put(&mut store, b, running());
    put(
        &mut store,
        b,
        EventBody::TurnStarted {
            turn_id: "t1".into(),
        },
    );
    put(&mut store, b, native_question("q-turn", "t1"));
    put(&mut store, b, answered("q-turn"));
    put(
        &mut store,
        b,
        EventBody::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Interrupted,
        },
    );
    put(&mut store, b, answer_failed("q-turn"));
    put(
        &mut store,
        b,
        EventBody::TurnStarted {
            turn_id: "t2".into(),
        },
    );
    put(&mut store, b, native_question("q-settled", "t2"));
    put(&mut store, b, answered("q-settled"));
    put(
        &mut store,
        b,
        EventBody::QuestionResolved {
            question_id: "q-settled".into(),
        },
    );
    put(&mut store, b, answer_failed("q-settled"));
    put(&mut store, b, native_question("q-process", "t2"));
    put(&mut store, b, answered("q-process"));
    put(
        &mut store,
        b,
        EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Stopped,
            reason: None,
        },
    );
    put(&mut store, b, answer_failed("q-process"));

    let s = crate::common::m2::serve_with_store(store).await;
    let find = |all: &Value, id: &str| {
        all.as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"] == id)
            .unwrap()
            .clone()
    };
    let qa = questions_of(&s, a).await;
    let reopened = find(&qa, "q-reopen");
    assert_eq!(reopened["state"], "open", "{reopened}");
    assert!(reopened["answer"].is_null());
    assert_eq!(
        reopened["deadline_ms"].as_i64(),
        Some(failed_at + 600_000),
        "a fresh, full timeout from the failure"
    );
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(a)
            .unwrap()
            .unwrap()
            .open_questions,
        1
    );
    let qb = questions_of(&s, b).await;
    for (id, words) in [
        ("q-turn", "turn ended before the answer reached it"),
        ("q-settled", "settled this question itself"),
        ("q-process", "process ended before the answer reached it"),
    ] {
        let q = find(&qb, id);
        assert_eq!(q["state"], "withdrawn", "{q}");
        assert!(
            q["withdrawn_reason"].as_str().unwrap().contains(words),
            "{id}: {q}"
        );
    }
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(b)
            .unwrap()
            .unwrap()
            .open_questions,
        0
    );
}

/// The project's Questions page: every question its agents asked, answered or not, newest first.
#[tokio::test]
async fn a_projects_questions_list_every_agents_questions_newest_first() {
    let (s, agent) = ready().await;
    crate::common::m2::say(&s, &agent, "ask:Which colour?|Blue|Green").await;
    let first = raised(&s, &agent, 0).await;
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{first}/answer"),
        answer_body("Green"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let after = crate::common::m2::events(&s.st, &agent).last().unwrap().seq;
    crate::common::m2::say(&s, &agent, "ask:Which size?|S|M").await;
    let second = raised(&s, &agent, after).await;
    let team =
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .rig;
    let (status, list) =
        crate::common::m2::get(&s.router, &format!("/api/teams/{team}/questions")).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [second.as_str(), first.as_str()], "newest first");
    assert_eq!(list[1]["answer"]["text"], "Green", "with the answer given");
    assert_eq!(list[0]["state"], "open");
}
