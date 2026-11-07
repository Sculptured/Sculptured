//! Questions agents raise: answers, expiry, withdrawal, permission prompts and Needs you. Contract tests
//! raise questions through Sculptured's own `request_input` handler on a recorded agent; live tests
//! (`live_…`) have a real Claude agent ask.
#[path = "common/asking.rs"]
mod asking;
mod common;

use asking::{A, B};
use axum::http::StatusCode;

use common::m2::{Served, rid};
use controld::store::Store;

use model::{
    Actor, Answer, EventBody, EventRecord, MessageIntent, Provenance, QuestionPrompt,
    QuestionSource, SeatActivity, SeatLifecycle, Subject, TurnStatus,
};
use serde_json::{Value, json};

/// A Job that has the agent ask one question through Sculptured's tool and wait for the answer.
const ASK_JOB: &str = "Call the mcp__rig__request_input tool exactly once with question \"Still there?\". Wait for its result, then reply with the single word OK and nothing else.";

fn answer_body(text: &str) -> Value {
    json!({ "request_id": rid(), "answer": { "kind": "text", "text": text } })
}

async fn questions_of(s: &Served, seat: &str) -> Value {
    crate::common::m2::get(&s.router, &format!("/api/seats/{seat}/questions"))
        .await
        .1
}

fn open_questions(s: &Served, seat: &str) -> u32 {
    s.st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .unwrap()
        .open_questions
}

async fn inbox(s: &Served) -> Value {
    crate::common::m2::get(&s.router, "/api/inbox").await.1
}

fn lifecycle(l: SeatLifecycle, reason: Option<&str>) -> EventBody {
    EventBody::SeatLifecycleChanged {
        lifecycle: l,
        reason: reason.map(Into::into),
    }
}

/// Records `body` on `seat` as Sculptured's own heuristic (a lifecycle it inferred).
fn inferred(s: &Served, seat: &str, body: EventBody) {
    s.st.local_event(
        Actor::System("test".into()),
        Subject::Seat(seat.into()),
        Provenance::Heuristic,
        body,
    )
    .unwrap();
}

fn set_timeout_ms(s: &Served, ms: u64) {
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(ms))
        .unwrap();
}

#[tokio::test]
async fn an_answered_question_reaches_the_agent_once() {
    let s = asking::recorded_agents().await;
    let asked = asking::ask(
        &s,
        A,
        json!({ "question": "Which colour?", "options": ["Blue", "Green"] }),
    )
    .await;
    let q = asked.id.clone();
    let inbox_now = inbox(&s).await;
    assert_eq!(inbox_now[0]["id"], format!("q:{q}"));
    assert_eq!(inbox_now[0]["kind"], "question");
    assert_eq!(
        inbox_now[0]["question"]["prompt"]["options"][1]["label"],
        "Green"
    );
    let open = inbox_now[0]["question"]["opened_at_ms"].as_i64().unwrap();
    assert_eq!(
        inbox_now[0]["question"]["deadline_ms"].as_i64().unwrap() - open,
        600_000,
        "D8 default"
    );
    crate::common::wait_for("waiting for input", || {
        let v = s.st.store.lock().unwrap().seat(A).unwrap().unwrap();
        (v.open_questions, v.activity) == (1, SeatActivity::WaitingInput)
    })
    .await;

    let body = answer_body("Green");
    let uri = format!("/api/questions/{q}/answer");
    let (status, v) = crate::common::m2::call(&s.router, "POST", &uri, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "answered");
    let (ok, result, _) = asked.reply().await;
    assert!(ok);
    assert_eq!(
        (result["text"].as_str(), result["timed_out"].as_bool()),
        (Some("Green"), Some(false)),
        "the agent's call got the answer, once"
    );
    assert_eq!(
        crate::common::m2::call(&s.router, "POST", &uri, body)
            .await
            .0,
        StatusCode::OK,
        "same request id replays"
    );
    let (status, err) = crate::common::m2::call(&s.router, "POST", &uri, answer_body("Blue")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["error"]["code"], "question_closed");
    let answers = crate::common::m2::events(&s.st, A)
        .into_iter()
        .filter(|e| matches!(e.body, EventBody::QuestionAnswered { .. }))
        .count();
    assert_eq!(answers, 1);
    assert!(s.st.seat_waits.lock().unwrap().is_empty());
    assert!(inbox(&s).await.as_array().unwrap().is_empty());
    assert_eq!(open_questions(&s, A), 0);
}

#[tokio::test]
async fn an_answer_must_fit_the_question() {
    let s = asking::recorded_agents().await;
    let q = asking::ask(&s, A, json!({ "question": "Pick", "options": ["A", "B"] }))
        .await
        .id;
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

/// D8: nobody answered in time, so the agent was told to decide; its next words are its decision.
#[tokio::test]
async fn an_expired_question_records_the_agents_own_decision() {
    let s = asking::recorded_agents().await;
    set_timeout_ms(&s, 300);
    let asked = asking::ask(
        &s,
        A,
        json!({ "question": "Pick one", "options": ["Red", "Blue"] }),
    )
    .await;
    let q = asked.id.clone();
    let (ok, result, _) = asked.reply().await;
    assert!(ok && result["timed_out"] == true, "{result}");
    asking::observed(
        &s,
        A,
        EventBody::AgentText {
            turn_id: None,
            text: "No answer came, so I decided: Red".into(),
        },
    );
    crate::common::m2::wait_event(&s.st, A, "the decision", |b| {
        matches!(b, EventBody::QuestionDecided { .. })
    })
    .await;
    let v = &questions_of(&s, A).await[0];
    assert_eq!(v["state"], "expired");
    assert_eq!(v["decision"]["text"], "No answer came, so I decided: Red");
    assert_eq!(v["decision"]["source"], "next-message");
    assert_eq!(v["decision"]["provenance"], "agent-reported");
    assert_eq!(inbox(&s).await[0]["kind"], "expired");
    let expiry = crate::common::m2::events(&s.st, A)
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
    assert!(inbox(&s).await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_silent_agent_leaves_its_decision_unknown() {
    let s = asking::recorded_agents().await;
    set_timeout_ms(&s, 300);
    asking::observed(
        &s,
        A,
        EventBody::TurnStarted {
            turn_id: "t1".into(),
        },
    );
    let asked = asking::ask(&s, A, json!({ "question": "Anything?" })).await;
    asked.reply().await;
    asking::observed(
        &s,
        A,
        EventBody::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Completed,
        },
    );
    crate::common::m2::wait_event(&s.st, A, "the decision", |b| {
        matches!(b, EventBody::QuestionDecided { .. })
    })
    .await;
    let v = &questions_of(&s, A).await[0];
    assert_eq!(
        (
            v["decision"]["text"].as_str(),
            v["decision"]["source"].as_str()
        ),
        (Some("unknown"), Some("unknown"))
    );
    assert_eq!(v["decision"]["provenance"], "heuristic");
}

/// Live: an answer after the question expired goes to the running agent as an overriding instruction,
/// once.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_late_answer_is_an_overriding_instruction() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    set_timeout_ms(&s, 300);
    let agent = asking::live_agent(
        &s,
        "Call the mcp__rig__request_input tool exactly once with question \"Pick one\" and options [\"Red\", \"Blue\"]. If nobody answers, choose Red and reply with the single word Red.",
        false,
    )
    .await;
    let (q, _) = asking::live_question(&s, &agent).await;
    crate::common::live::wait_event(
        &s.st,
        &agent,
        "the expiry",
        |b| matches!(b, EventBody::QuestionExpired { question_id } if *question_id == q),
    )
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
    let late = crate::common::live::wait_event(&s.st, &agent, "the override", |b| matches!(b, EventBody::MessageQueued { intent: MessageIntent::Instruct, text, .. } if text.starts_with("Late answer from the operator"))).await;
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
        asking_view(&s, &agent, &q).await["late_answer"]["text"],
        "Blue",
        "the first override stands"
    );
    crate::common::m2::stop_team(&s, &team_of(&s, &agent)).await;
}

/// Review Focus #2: an answer and the timeout firing at once — exactly one resolution reaches the
/// agent's call. On a multi-thread runtime, with permission prompts (`approve` of a restricted agent)
/// answered "allow", so a losing answer is a late one that needs no message to the agent. Rounds
/// alternate which side holds the question first:
/// - expiry first: a test hook pauses `expire` inside its claim, between its "still open?" check and
///   its record (the window the claim guards), and the answer arrives meanwhile;
/// - answer first: the answer is recorded, and the expiry fires right after.
///
/// Without the claim in `expire`, an expiry-first round replies twice and records both an answer and an
/// expiry for one question.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn answer_racing_the_timeout_has_one_winner() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let s = asking::recorded_agents().await;
    asking::restrict(&s);
    let allow = || json!({ "request_id": rid(), "answer": { "kind": "allow" } });
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
        let asked = asking::ask_permission(&s, A).await;
        let q = asked.id.clone();
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
            let answered = crate::common::m2::call(&s.router, "POST", &uri, allow()).await;
            expiry.await.unwrap();
            answered
        } else {
            let (router, uri) = (s.router.clone(), uri.clone());
            let answer = tokio::spawn(async move {
                crate::common::m2::call(&router, "POST", &uri, allow()).await
            });
            crate::common::m2::wait_event(&s.st, A, "the answer", |b| {
                matches!(b, EventBody::QuestionAnswered { question_id, late: false, .. } if *question_id == q)
            })
            .await;
            controld::questions::expire(&s.st, &q).await;
            answer.await.unwrap()
        };
        assert_eq!(status, StatusCode::OK, "{v}");
        let (ok, result, _) = asked.reply().await;
        assert!(
            ok,
            "round {round}: exactly one resolution reached the call: {result}"
        );
        assert_eq!(open_questions(&s, A), 0, "round {round}");
        let v = questions_of(&s, A)
            .await
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["id"] == q.as_str())
            .unwrap()
            .clone();
        match (v["state"].as_str(), expiry_first) {
            (Some("answered"), false) => assert!(v["expired_seq"].is_null()),
            (Some("expired"), true) => assert_eq!(
                v["late_answer"]["kind"], "allow",
                "the losing answer became a late one"
            ),
            other => panic!("round {round}: {other:?}"),
        }
        states.push(v["state"].as_str().unwrap().to_string());
    }
    *controld::seams::IN_EXPIRE_BEFORE_RECORD.lock().unwrap() = None;
    // Over the whole log: no question was both answered (on time) and expired.
    let all = crate::common::m2::events(&s.st, A);
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

/// Live: stopping a real agent while its question is open withdraws the question.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_process_end_withdraws_its_open_question() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    let agent = asking::live_agent(&s, ASK_JOB, false).await;
    let (q, source) = asking::live_question(&s, &agent).await;
    assert_eq!(source, QuestionSource::RequestInput);
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{agent}/stop"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    crate::common::live::wait_event(&s.st, &agent, "stopped", |b| {
        matches!(b, EventBody::TenureEnded { .. })
    })
    .await;
    crate::common::live::wait_for("withdrawn", || open_questions(&s, &agent) == 0).await;
    let v = &questions_of(&s, &agent).await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some(q.as_str()), Some("withdrawn"))
    );
    // Whichever ended first: the process, its turn, or the agent's waiting call (the tool's process
    // went with the CLI). Each is an honest reason, as long as the question is withdrawn.
    let reason = v["withdrawn_reason"].as_str().unwrap();
    assert!(
        ["process ended", "interrupted", "stopped waiting"]
            .iter()
            .any(|r| reason.contains(r)),
        "{reason}"
    );
}

/// Live: interrupting a real agent's turn while its question is open withdraws the question.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_interrupted_turn_withdraws_its_question() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    let agent = asking::live_agent(&s, ASK_JOB, false).await;
    let (q, _) = asking::live_question(&s, &agent).await;
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{agent}/interrupt"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    crate::common::live::wait_for("withdrawn", || open_questions(&s, &agent) == 0).await;
    let v = questions_of(&s, &agent)
        .await
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == q.as_str())
        .unwrap()
        .clone();
    assert_eq!(v["state"], "withdrawn", "{v}");
    crate::common::m2::stop_team(&s, &team_of(&s, &agent)).await;
}

/// Review Focus #4 (controld half): a question nobody can answer any more is withdrawn on start.
#[tokio::test]
async fn orphaned_questions_are_withdrawn_on_start() {
    let mut store = asking::store_with_agents(&[A]);
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
            Actor::Seat(A.into()),
            Subject::Seat(A.into()),
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
    let e = crate::common::m2::wait_event(&s.st, A, "approve withdrawn", |b| matches!(b, EventBody::QuestionWithdrawn { question_id, .. } if question_id == "q-approve")).await;
    let EventBody::QuestionWithdrawn { reason, .. } = e.body else {
        unreachable!()
    };
    assert!(
        reason.contains("restarted"),
        "a seat-tool question of a running agent has no waiter either: {reason}"
    );
    let a = s.st.store.lock().unwrap().seat(A).unwrap().unwrap();
    assert_eq!((a.lifecycle, a.open_questions), (SeatLifecycle::Running, 0));
    assert!(inbox(&s).await.as_array().unwrap().is_empty());
}

/// A process lost without a TenureEnded (hostd restarted, the machine slept) leaves only an
/// `attention` lifecycle; its open question is withdrawn, not left open with buttons that fail.
#[tokio::test]
async fn a_lost_process_withdraws_its_open_question() {
    let s = asking::recorded_agents().await;
    let q = asking::ask(&s, A, json!({ "question": "Still there?" }))
        .await
        .id;
    inferred(
        &s,
        A,
        lifecycle(SeatLifecycle::Attention, Some("lost with its host")),
    );
    crate::common::wait_for("withdrawn", || open_questions(&s, A) == 0).await;
    let v = &questions_of(&s, A).await[0];
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
async fn an_agent_that_needs_attention_is_in_the_inbox() {
    let s = asking::recorded_agents().await;
    inferred(
        &s,
        A,
        lifecycle(SeatLifecycle::Attention, Some("the agent's program exited")),
    );
    let inbox_now = inbox(&s).await;
    assert_eq!(inbox_now[0]["id"], format!("seat:{A}"));
    assert_eq!(inbox_now[0]["kind"], "attention");
    assert_eq!(inbox_now[0]["seat_view"]["id"], A);
}

/// Live: an agent whose Job turn was interrupted owes its handoff; it waits in Needs you as stuck.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_stuck_handoff_is_in_the_inbox() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _host = crate::common::live::connect(&s).await;
    let id = crate::common::m2::chain(
        &s,
        "Stuck chain",
        &[
            (
                "planner",
                "Planner",
                "Use your Bash tool to run exactly: sleep 600",
            ),
            (
                "builder",
                "Builder",
                "Reply with the single word OK and nothing else.",
            ),
        ],
        &[("planner", "builder")],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let planner = crate::common::m2::seat(&id, "planner");
    crate::common::live::wait_event(
        &s.st,
        &planner,
        "the sleep",
        |b| matches!(b, EventBody::ToolCalled { name, .. } if name == "Bash"),
    )
    .await;
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{planner}/interrupt"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    crate::common::live::wait_for("stuck", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&planner)
            .unwrap()
            .unwrap()
            .handoff_stuck
            .is_some()
    })
    .await;
    let inbox_now = inbox(&s).await;
    assert!(
        inbox_now
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "stuck" && i["seat"] == planner.as_str()),
        "{inbox_now}"
    );
    crate::common::m2::stop_team(&s, &id).await;
}

/// The user's real run: a turn ended on the CLI's usage limit (recorded as hostd reads the CLI's words).
/// It waits in Needs you with those words until the next turn starts.
#[tokio::test]
async fn a_usage_limit_waits_in_needs_you() {
    let s = asking::recorded_agents().await;
    let text = "You've hit your session limit · resets 4:30am (America/New_York)";
    asking::observed(
        &s,
        A,
        EventBody::TurnStarted {
            turn_id: "t1".into(),
        },
    );
    asking::observed(
        &s,
        A,
        EventBody::LimitReached {
            turn_id: Some("t1".into()),
            kind: model::LimitKind::Usage,
            resets: Some("4:30am".into()),
            text: text.into(),
        },
    );
    asking::observed(
        &s,
        A,
        EventBody::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Failed,
        },
    );
    let inbox_now = inbox(&s).await;
    let item = inbox_now
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["seat"] == A)
        .expect("in Needs you")
        .clone();
    assert_eq!(item["kind"], "limit");
    assert_eq!(item["seat_view"]["limit_hit"]["resets"], "4:30am");
    assert_eq!(item["seat_view"]["limit_hit"]["text"], text);
    // The operator's "continue" starts the next turn: the limit is no longer the agent's state.
    asking::observed(
        &s,
        A,
        EventBody::TurnStarted {
            turn_id: "t2".into(),
        },
    );
    let inbox_now = inbox(&s).await;
    assert!(
        !inbox_now
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "limit"),
        "{inbox_now}"
    );
}

// ---- Fix round 1 -------------------------------------------------------------------------------

/// Ruling Q1: the answer is recorded before it is delivered: when the agent's call gets its reply,
/// the answer is already in the log.
#[tokio::test]
async fn an_answer_is_recorded_before_the_agent_hears_it() {
    let s = asking::recorded_agents().await;
    let asking::Asked { call, mut link, id } = asking::ask(
        &s,
        A,
        json!({ "question": "Which colour?", "options": ["Blue", "Green"] }),
    )
    .await;
    let st = s.st.clone();
    let heard = tokio::spawn(async move {
        let reply = link.recv().await.expect("the reply");
        let answered = crate::common::m2::events(&st, A)
            .iter()
            .any(|e| matches!(e.body, EventBody::QuestionAnswered { .. }));
        (reply, answered)
    });
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{id}/answer"),
        answer_body("Green"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    crate::common::seat_tools::ended(call).await;
    let (reply, answered) = heard.await.unwrap();
    assert!(
        matches!(reply, host_proto::ToHost::SeatToolReply { ok: true, .. }),
        "{reply:?}"
    );
    assert!(answered, "recorded before the reply went out");
    assert_eq!(questions_of(&s, A).await[0]["state"], "answered");
}

#[tokio::test]
async fn a_failed_turn_withdraws_its_question() {
    let s = asking::recorded_agents().await;
    asking::observed(
        &s,
        A,
        EventBody::TurnStarted {
            turn_id: "t1".into(),
        },
    );
    let q = asking::ask(&s, A, json!({ "question": "Wait?" })).await.id;
    let turn = crate::common::m2::events(&s.st, A)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::QuestionRaised { turn_id, .. } => turn_id,
            _ => None,
        });
    assert_eq!(turn.as_deref(), Some("t1"), "raised on the running turn");
    asking::observed(
        &s,
        A,
        EventBody::TurnEnded {
            turn_id: "t1".into(),
            status: TurnStatus::Failed,
        },
    );
    crate::common::wait_for("withdrawn", || open_questions(&s, A) == 0).await;
    let v = &questions_of(&s, A).await[0];
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

fn native_prompt(text: &str, options: &[&str], allow_other: bool) -> QuestionPrompt {
    QuestionPrompt {
        urgency: None,
        header: None,
        text: text.into(),
        options: options
            .iter()
            .map(|l| model::QuestionOption {
                label: (*l).into(),
                description: None,
            })
            .collect(),
        allow_other,
        tool: None,
        tool_input: None,
    }
}

/// The program settled the question itself (Codex can): it is withdrawn, and an answer is refused.
#[tokio::test]
async fn a_question_the_program_settled_itself_is_withdrawn() {
    let s = asking::recorded_agents().await;
    asking::observed(
        &s,
        A,
        EventBody::QuestionRaised {
            question_id: "q-native".into(),
            source: QuestionSource::NativeQuestion,
            prompt: native_prompt("Wait?", &[], true),
            turn_id: None,
        },
    );
    asking::observed(
        &s,
        A,
        EventBody::QuestionResolved {
            question_id: "q-native".into(),
        },
    );
    crate::common::wait_for("withdrawn", || open_questions(&s, A) == 0).await;
    let v = &questions_of(&s, A).await[0];
    assert_eq!(v["state"], "withdrawn");
    assert!(
        v["withdrawn_reason"].as_str().unwrap().contains("settled"),
        "{v}"
    );
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/questions/q-native/answer",
        answer_body("now"),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("question_closed"))
    );
}

/// A Job that makes a restricted agent ask permission for one harmless shell command in its folder.
const PERMISSION_JOB: &str =
    "Use your Bash tool to run exactly: echo sculptured-ok. Then reply with the single word OK.";

/// The first Bash call of `agent`, with its result: (permission question, Bash call id).
async fn live_permission(s: &Served, agent: &str) -> (String, String) {
    let (q, source) = asking::live_question(s, agent).await;
    assert_eq!(source, QuestionSource::NativePermission, "the approve tool");
    let call = asking::live_call_of(s, agent, "Bash").await;
    (q, call)
}

/// Live, D8 at the server: a restricted agent's permission prompt nobody answers is allowed, that is
/// recorded as observed, and the command runs.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_unanswered_permission_prompt_is_allowed_and_recorded() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    set_timeout_ms(&s, 5000);
    let agent = asking::live_agent(&s, PERMISSION_JOB, true).await;
    let (q, call) = live_permission(&s, &agent).await;
    let inbox_now = inbox(&s).await;
    assert_eq!(
        (inbox_now[0]["id"].as_str(), inbox_now[0]["kind"].as_str()),
        (Some(format!("q:{q}").as_str()), Some("permission"))
    );
    let decided = crate::common::live::wait_event(
        &s.st,
        &agent,
        "the decision",
        |b| matches!(b, EventBody::QuestionDecided { question_id, .. } if *question_id == q),
    )
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
        .find(
            |e| matches!(&e.body, EventBody::QuestionExpired { question_id } if *question_id == q),
        )
        .unwrap();
    assert_eq!(expired.actor, Actor::System("timeout".into()));
    let v = asking_view(&s, &agent, &q).await;
    assert_eq!(v["state"], "expired");
    assert_eq!(v["decision"]["source"], "allowed-on-timeout");
    assert_eq!(v["decision"]["provenance"], "protocol-observed");
    let (_, is_error) = asking::live_result_of(&s, &agent, &call).await;
    assert!(!is_error, "the allowed command ran");
    crate::common::m2::stop_team(&s, &team_of(&s, &agent)).await;
}

/// Live: a restricted agent's permission prompt takes only allow or deny; allowed, the command runs.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_allowed_permission_prompt_runs_the_command() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    let agent = asking::live_agent(&s, PERMISSION_JOB, true).await;
    let (q, call) = live_permission(&s, &agent).await;
    let uri = format!("/api/questions/{q}/answer");
    let (status, err) = crate::common::m2::call(&s.router, "POST", &uri, answer_body("yes")).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("bad_answer")),
        "text to a permission prompt"
    );
    let allow = json!({ "request_id": rid(), "answer": { "kind": "allow" } });
    let (status, v) = crate::common::m2::call(&s.router, "POST", &uri, allow).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (_, is_error) = asking::live_result_of(&s, &agent, &call).await;
    assert!(!is_error, "the allowed command ran");
    crate::common::m2::stop_team(&s, &team_of(&s, &agent)).await;
}

async fn asking_view(s: &Served, seat: &str, id: &str) -> Value {
    crate::common::seat_tools::question_view(s, seat, id).await
}

fn team_of(s: &Served, seat: &str) -> String {
    s.st.store.lock().unwrap().seat(seat).unwrap().unwrap().rig
}

/// A permission prompt takes allow or deny, never text; a closed choice takes only one of its options.
#[tokio::test]
async fn permission_prompts_and_closed_choices_take_only_fitting_answers() {
    let s = asking::recorded_agents().await;
    let mut permission = native_prompt("Allow Bash to run?", &[], false);
    permission.tool = Some("Bash".into());
    permission.tool_input = Some(json!({ "command": "ls" }));
    for (id, source, prompt, answer) in [
        (
            "q-perm",
            QuestionSource::NativePermission,
            permission,
            "yes",
        ),
        (
            "q-choice",
            QuestionSource::NativeQuestion,
            native_prompt("Pick", &["A", "B"], false),
            "C",
        ),
    ] {
        asking::observed(
            &s,
            A,
            EventBody::QuestionRaised {
                question_id: id.into(),
                source,
                prompt,
                turn_id: None,
            },
        );
        let (status, err) = crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/questions/{id}/answer"),
            answer_body(answer),
        )
        .await;
        assert_eq!(
            (status, err["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("bad_answer")),
            "{id}: {err}"
        );
        assert_eq!(asking_view(&s, A, id).await["state"], "open", "{id}");
    }
}

/// A link blip (`stale`) withdraws nothing, and the timeout waits until the agent runs again.
#[tokio::test]
async fn a_stale_link_withdraws_nothing_and_holds_the_timeout() {
    let s = asking::recorded_agents().await;
    set_timeout_ms(&s, 800);
    let asked = asking::ask(&s, A, json!({ "question": "Wait?", "options": ["Stay"] })).await;
    let q = asked.id.clone();
    inferred(&s, A, lifecycle(SeatLifecycle::Stale, None));
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let v = &questions_of(&s, A).await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some(q.as_str()), Some("open")),
        "past its deadline, but the agent is away"
    );
    assert!(
        !crate::common::m2::events(&s.st, A)
            .into_iter()
            .any(|e| matches!(e.body, EventBody::QuestionExpired { .. }))
    );
    assert_eq!(open_questions(&s, A), 1);
    inferred(&s, A, lifecycle(SeatLifecycle::Running, None));
    crate::common::m2::wait_event(&s.st, A, "the expiry", |b| {
        matches!(b, EventBody::QuestionExpired { .. })
    })
    .await;
    let (ok, result, _) = asked.reply().await;
    assert!(
        ok && result["timed_out"] == true,
        "the agent is told: {result}"
    );
}

/// Review Focus #4: a native question of a running agent survives a controld restart (hostd still holds
/// it and can take the answer); a native question of an agent that no longer runs is withdrawn.
#[tokio::test]
async fn a_restart_keeps_native_questions_of_running_agents() {
    let mut store = asking::store_with_agents(&[A]);
    let native = |id: &str| EventBody::QuestionRaised {
        question_id: id.into(),
        source: QuestionSource::NativeQuestion,
        prompt: native_prompt("Which?", &[], true),
        turn_id: None,
    };
    // Raised through hostd's mapping: protocol-observed. The running agent's comes first.
    put(&mut store, A, native("q-native"));
    put(&mut store, B, native("q-gone"));

    let s = crate::common::m2::serve_with_store(store).await;
    let e = crate::common::m2::wait_event(&s.st, B, "withdrawn", |b| matches!(b, EventBody::QuestionWithdrawn { question_id, .. } if question_id == "q-gone")).await;
    let EventBody::QuestionWithdrawn { reason, .. } = e.body else {
        unreachable!()
    };
    assert!(reason.contains("process ended"), "{reason}");
    let v = &questions_of(&s, A).await[0];
    assert_eq!(
        (v["id"].as_str(), v["state"].as_str()),
        (Some("q-native"), Some("open")),
        "{v}"
    );
    assert!(
        !crate::common::m2::events(&s.st, A)
            .into_iter()
            .any(|e| matches!(e.body, EventBody::QuestionWithdrawn { .. }))
    );
    let inbox_now = inbox(&s).await;
    let ids: Vec<&str> = inbox_now
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
    let s = asking::recorded_agents().await;
    inferred(&s, A, lifecycle(SeatLifecycle::Attention, Some("lost")));
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    inferred(&s, B, lifecycle(SeatLifecycle::Attention, Some("lost")));
    tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    // A later event on A (here: attention again, with a new reason) must not move it behind B.
    inferred(
        &s,
        A,
        lifecycle(SeatLifecycle::Attention, Some("still lost")),
    );
    let inbox_now = inbox(&s).await;
    let ids: Vec<&str> = inbox_now
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [format!("seat:{A}"), format!("seat:{B}")],
        "{inbox_now}"
    );
    assert!(inbox_now[0]["opened_at_ms"].as_i64() < inbox_now[1]["opened_at_ms"].as_i64());
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
    EventBody::QuestionRaised {
        question_id: id.into(),
        source: QuestionSource::NativeQuestion,
        prompt: native_prompt("Which?", &[], true),
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
    let mut store = asking::store_with_agents(&[]);
    let running = || lifecycle(SeatLifecycle::Running, None);
    let (a, b) = (A, B);

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
    put(&mut store, b, lifecycle(SeatLifecycle::Stopped, None));
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
    assert_eq!(open_questions(&s, a), 1);
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
    assert_eq!(open_questions(&s, b), 0);
}

/// The project's Questions page: every question its agents asked, answered or not, newest first.
#[tokio::test]
async fn a_projects_questions_list_every_agents_questions_newest_first() {
    let s = asking::recorded_agents().await;
    let first = asking::ask(
        &s,
        A,
        json!({ "question": "Which colour?", "options": ["Blue", "Green"] }),
    )
    .await;
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{}/answer", first.id),
        answer_body("Green"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let first_id = first.id.clone();
    first.reply().await;
    let second = asking::ask(
        &s,
        A,
        json!({ "question": "Which size?", "options": ["S", "M"] }),
    )
    .await
    .id;
    let team = team_of(&s, A);
    let (status, list) =
        crate::common::m2::get(&s.router, &format!("/api/teams/{team}/questions")).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [second.as_str(), first_id.as_str()], "newest first");
    assert_eq!(list[1]["answer"]["text"], "Green", "with the answer given");
    assert_eq!(list[0]["state"], "open");
}
