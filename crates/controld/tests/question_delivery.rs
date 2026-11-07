//! Answers whose delivery is in doubt, and an answer that gives up waiting. A file of its own: it shortens
//! the answer's claim wait for its whole process (a test seam). The doubts are recorded as hostd reports
//! them; questions in flight come from Sculptured's own `request_input` handler.
#[path = "common/asking.rs"]
mod asking;
mod common;

use asking::A;
use axum::http::StatusCode;

use common::m2::{Served, rid};

use model::{Answer, EventBody, QuestionPrompt, QuestionSource};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const REPLY_MS: u64 = 300;
const CLAIM_MS: u64 = 400;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The same short waits for every test here: a host that says nothing for 300 ms has not confirmed
/// a command, and an answer waits at most 400 ms for a question someone else holds.
fn tune() {
    controld::seams::HOST_REPLY_TIMEOUT_MS.store(REPLY_MS, Ordering::SeqCst);
    controld::seams::CLAIM_WAIT_MS.store(CLAIM_MS, Ordering::SeqCst);
}

async fn view_of(s: &Served, seat: &str, q: &str) -> Value {
    crate::common::seat_tools::question_view(s, seat, q).await
}

async fn send_again(s: &Served, q: &str, body: Value) -> (StatusCode, Value) {
    crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/send-again"),
        body,
    )
    .await
}

async fn wait_view(
    s: &Served,
    seat: &str,
    q: &str,
    what: &str,
    pred: impl Fn(&Value) -> bool,
) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let v = view_of(s, seat, q).await;
        if pred(&v) {
            return v;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}: {v}"
        );
        tokio::time::sleep(ms(20)).await;
    }
}

/// A native question `q` of agent A, recorded as hostd maps it, on turn `t1`.
fn native_question(store: &mut controld::store::Store, q: &str) {
    let prompt = QuestionPrompt {
        urgency: None,
        header: None,
        text: "Pick".into(),
        options: vec![],
        allow_other: true,
        tool: None,
        tool_input: None,
    };
    for body in [
        EventBody::TurnStarted {
            turn_id: "t1".into(),
        },
        EventBody::QuestionRaised {
            question_id: q.into(),
            source: QuestionSource::NativeQuestion,
            prompt,
            turn_id: Some("t1".into()),
        },
    ] {
        put(store, body);
    }
}

fn put(store: &mut controld::store::Store, body: EventBody) {
    store
        .append_local(
            model::Actor::Seat(A.into()),
            model::Subject::Seat(A.into()),
            model::Provenance::ProtocolObserved,
            body,
        )
        .unwrap();
}

fn unconfirmed(q: &str) -> EventBody {
    EventBody::QuestionAnswerUnconfirmed {
        question_id: q.into(),
        reason: format!("Sculptured's agent runner did not reply within {REPLY_MS} ms"),
    }
}

/// An answer the host never confirmed stays answered, in doubt and in Needs you; the program's own
/// "resolved" notice for it confirms it, so the doubt clears by itself and "Send again" has nothing to send.
#[tokio::test]
async fn the_programs_resolved_notice_confirms_a_doubtful_answer() {
    tune();
    let mut store = asking::store_with_agents(&[A]);
    native_question(&mut store, "q-1");
    put(
        &mut store,
        EventBody::QuestionAnswered {
            question_id: "q-1".into(),
            answer: Answer::Text { text: "A".into() },
            late: false,
        },
    );
    put(&mut store, unconfirmed("q-1"));
    let s = crate::common::m2::serve_with_store(store).await;
    let v = view_of(&s, A, "q-1").await;
    assert_eq!(
        (v["state"].as_str(), v["answer"]["text"].as_str()),
        (Some("answered"), Some("A")),
        "{v}"
    );
    assert!(
        v["unconfirmed"]
            .as_str()
            .unwrap()
            .contains("did not reply within 300 ms"),
        "the host's words: {v}"
    );
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(
        (inbox[0]["id"].as_str(), inbox[0]["kind"].as_str()),
        (Some("q:q-1"), Some("unconfirmed")),
        "{inbox}"
    );
    asking::observed(
        &s,
        A,
        EventBody::QuestionResolved {
            question_id: "q-1".into(),
        },
    );
    let v = wait_view(&s, A, "q-1", "the doubt to clear", |v| {
        v["unconfirmed"].is_null()
    })
    .await;
    assert_eq!(v["state"], "answered");
    assert!(
        crate::common::m2::get(&s.router, "/api/inbox")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (status, err) = send_again(&s, "q-1", json!({ "request_id": rid() })).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("answer_confirmed")),
        "{err}"
    );
}

/// Minor 4: an expiry whose delivery is unconfirmed says it may not have reached the agent and records no
/// "unknown" decision at once; the agent's own words decide it once the expiry gets through.
#[tokio::test]
async fn an_expiry_that_may_be_lost_waits_for_the_agents_words() {
    tune();
    let mut store = asking::store_with_agents(&[A]);
    native_question(&mut store, "q-1");
    store
        .append_local(
            model::Actor::System("timeout".into()),
            model::Subject::Seat(A.into()),
            model::Provenance::ProtocolObserved,
            EventBody::QuestionExpired {
                question_id: "q-1".into(),
            },
        )
        .unwrap();
    put(&mut store, unconfirmed("q-1"));
    let s = crate::common::m2::serve_with_store(store).await;
    tokio::time::sleep(ms(600)).await;
    let v = view_of(&s, A, "q-1").await;
    assert_eq!(v["state"], "expired");
    assert!(
        v["unconfirmed"].as_str().unwrap().contains("did not reply"),
        "{v}"
    );
    assert!(
        v["decision"].is_null(),
        "no unknown decision while the expiry may still arrive: {v}"
    );
    asking::observed(
        &s,
        A,
        EventBody::AgentText {
            turn_id: Some("t1".into()),
            text: "No answer came, so I decided: Red".into(),
        },
    );
    let v = wait_view(&s, A, "q-1", "the decision", |v| v["decision"].is_object()).await;
    assert_eq!(v["decision"]["text"], "No answer came, so I decided: Red");
    assert_eq!(v["decision"]["source"], "next-message");
}

/// Ruling (Important 1): an answer waits for the question at most the claim wait, then says so plainly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_answer_gives_up_when_the_question_stays_busy() {
    tune();
    let s = asking::recorded_agents().await;
    // A permission prompt, answered "allow": once late, that answer needs no message to the agent.
    asking::restrict(&s);
    let q = asking::ask_permission(&s, A).await.id;
    let allow = || json!({ "request_id": rid(), "answer": { "kind": "allow" } });
    let inside = Arc::new(AtomicBool::new(false));
    let (hook_q, hook_inside) = (q.clone(), inside.clone());
    *controld::seams::IN_EXPIRE_BEFORE_RECORD.lock().unwrap() = Some(Arc::new(move |id: &str| {
        if id == hook_q {
            hook_inside.store(true, Ordering::SeqCst);
            std::thread::sleep(ms(1500));
        }
    }));
    let (st, id) = (s.st.clone(), q.clone());
    let expiry = tokio::spawn(async move { controld::questions::expire(&st, &id).await });
    common::wait_for("the expiry to hold the question", || {
        inside.load(Ordering::SeqCst)
    })
    .await;
    let started = tokio::time::Instant::now();
    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        allow(),
    )
    .await;
    let waited = started.elapsed();
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("question_busy")),
        "{err}"
    );
    assert_eq!(
        err["error"]["message"],
        "Someone else is answering this question right now; try again in a moment."
    );
    assert!(
        waited >= ms(CLAIM_MS) && waited < ms(1400),
        "waited {waited:?}"
    );
    expiry.await.unwrap();
    *controld::seams::IN_EXPIRE_BEFORE_RECORD.lock().unwrap() = None;
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        allow(),
    )
    .await;
    assert_eq!(
        (status, v["state"].as_str()),
        (StatusCode::OK, Some("expired")),
        "once free, the answer is late: {v}"
    );
}
