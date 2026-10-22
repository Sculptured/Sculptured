//! Answers whose delivery is in doubt, "Send again", and an answer that gives up waiting. A file of its own:
//! it shortens the host's reply timeout and the answer's claim wait for its whole process (test seams).
mod common;
#[path = "common/knobbed.rs"]
mod knobbed;

use axum::http::StatusCode;

use common::m2::{Served, rid};

use knobbed::{AnswerPlan, Knobbed, REFUSAL};

use model::EventBody;
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

async fn raised(s: &Served, seat: &str) -> String {
    let e = crate::common::m2::wait_event(&s.st, seat, "a question", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    })
    .await;
    let EventBody::QuestionRaised { question_id, .. } = e.body else {
        unreachable!()
    };
    question_id
}

fn answer_body(text: &str) -> Value {
    json!({ "request_id": rid(), "answer": { "kind": "text", "text": text } })
}

async fn view_of(s: &Served, seat: &str, q: &str) -> Value {
    let (_, all) = crate::common::m2::get(&s.router, &format!("/api/seats/{seat}/questions")).await;
    all.as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == q)
        .unwrap()
        .clone()
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

fn count(s: &Served, seat: &str, pred: impl Fn(&EventBody) -> bool) -> usize {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .filter(|e| pred(&e.body))
        .count()
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

/// Ruling (Important 1): an answer the host never confirmed stays answered, is recorded as in doubt and
/// shown in Needs you; "Send again" re-sends the recorded answer once (idempotent by request id),
/// records no second answer, and clears the doubt.
#[tokio::test]
async fn a_lost_answer_shows_in_needs_you_and_is_sent_again_once() {
    tune();
    let knobs = Knobbed {
        plan: vec![AnswerPlan::Drop(ms(700))],
        ..Knobbed::default()
    };
    let calls = knobs.calls.clone();
    let (s, agent) = crate::knobbed::ready_with(knobs).await;
    crate::common::m2::say(&s, &agent, "ask:Pick|A|B").await;
    let q = raised(&s, &agent).await;

    let (status, err) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        answer_body("A"),
    )
    .await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::BAD_GATEWAY, Some("host_unreachable")),
        "{err}"
    );
    assert!(
        err["error"]["message"]
            .as_str()
            .unwrap()
            .contains("may not have reached the agent"),
        "{err}"
    );
    let v = view_of(&s, &agent, &q).await;
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
    assert_eq!(
        count(&s, &agent, |b| matches!(
            b,
            EventBody::QuestionAnswerUnconfirmed { .. }
        )),
        1
    );
    assert_eq!(
        count(&s, &agent, |b| matches!(
            b,
            EventBody::QuestionAnswerFailed { .. }
        )),
        0,
        "never reopened"
    );
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(
        (inbox[0]["id"].as_str(), inbox[0]["kind"].as_str()),
        (Some(format!("q:{q}").as_str()), Some("unconfirmed")),
        "{inbox}"
    );

    // The lost delivery finishes on the runner's side; then the operator sends it again.
    tokio::time::sleep(ms(600)).await;
    let body = json!({ "request_id": rid() });
    let (status, v) = send_again(&s, &q, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        (v["state"].as_str(), v["unconfirmed"].as_str()),
        (Some("answered"), None),
        "{v}"
    );
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: A"),
    )
    .await;
    assert_eq!(
        send_again(&s, &q, body).await.0,
        StatusCode::OK,
        "the same request id replays"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the lost delivery and one Send again; the replay sent nothing"
    );
    assert_eq!(
        count(&s, &agent, |b| matches!(
            b,
            EventBody::QuestionAnswered { .. }
        )),
        1,
        "no second answer is recorded"
    );
    assert_eq!(
        count(&s, &agent, |b| matches!(
            b,
            EventBody::QuestionAnswerConfirmed { .. }
        )),
        1
    );
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
    let (status, err) = send_again(&s, &q, json!({ "request_id": rid() })).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("answer_confirmed")),
        "{err}"
    );
}

/// A refused "Send again" keeps the question answered (never reopened) and records the refusal verbatim
/// as the reason for the doubt; Needs you keeps showing it.
#[tokio::test]
async fn a_refused_send_again_keeps_the_answer_and_records_why() {
    tune();
    let knobs = Knobbed {
        plan: vec![AnswerPlan::Drop(ms(700)), AnswerPlan::Refuse(ms(0))],
        ..Knobbed::default()
    };
    let (s, agent) = crate::knobbed::ready_with(knobs).await;
    crate::common::m2::say(&s, &agent, "ask:Pick|A|B").await;
    let q = raised(&s, &agent).await;
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/questions/{q}/answer"),
            answer_body("B")
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    tokio::time::sleep(ms(600)).await;

    let (status, err) = send_again(&s, &q, json!({ "request_id": rid() })).await;
    assert_eq!(
        (status, err["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("answer_refused")),
        "{err}"
    );
    assert!(
        err["error"]["message"].as_str().unwrap().contains(REFUSAL),
        "{err}"
    );
    let v = view_of(&s, &agent, &q).await;
    assert_eq!(
        (v["state"].as_str(), v["answer"]["text"].as_str()),
        (Some("answered"), Some("B")),
        "{v}"
    );
    assert!(
        v["unconfirmed"].as_str().unwrap().contains(REFUSAL),
        "the refusal, verbatim: {v}"
    );
    assert_eq!(
        count(&s, &agent, |b| matches!(
            b,
            EventBody::QuestionAnswerFailed { .. }
        )),
        0,
        "never reopened"
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
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(
        inbox[0]["kind"], "unconfirmed",
        "still in Needs you: {inbox}"
    );

    let (status, v) = send_again(&s, &q, json!({ "request_id": rid() })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: B"),
    )
    .await;
    assert!(
        crate::common::m2::get(&s.router, "/api/inbox")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
}

/// The program's own "resolved" notice for an answered question confirms it: the doubt clears by itself.
#[tokio::test]
async fn the_programs_resolved_notice_confirms_a_doubtful_answer() {
    tune();
    let (s, agent) = crate::knobbed::ready_with(Knobbed {
        plan: vec![AnswerPlan::Delay(ms(700))],
        ..Knobbed::default()
    })
    .await;
    crate::common::m2::say(&s, &agent, "ask:Pick|A|B").await;
    let q = raised(&s, &agent).await;
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/questions/{q}/answer"),
            answer_body("A")
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    assert!(view_of(&s, &agent, &q).await["unconfirmed"].is_string());
    crate::common::m2::wait_event(&s.st, &agent, "the late delivery", |b| {
        matches!(b, EventBody::QuestionResolved { .. })
    })
    .await;
    let v = wait_view(&s, &agent, &q, "the doubt to clear", |v| {
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
    let (status, err) = send_again(&s, &q, json!({ "request_id": rid() })).await;
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
    let (s, agent) = crate::knobbed::ready_with(Knobbed {
        plan: vec![AnswerPlan::Drop(ms(700))],
        ..Knobbed::default()
    })
    .await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(300))
        .unwrap();
    crate::common::m2::say(&s, &agent, "ask:Pick one|Red|Blue").await;
    let q = raised(&s, &agent).await;
    crate::common::m2::wait_event(&s.st, &agent, "the doubt", |b| {
        matches!(b, EventBody::QuestionAnswerUnconfirmed { .. })
    })
    .await;
    tokio::time::sleep(ms(600)).await;
    let v = view_of(&s, &agent, &q).await;
    assert_eq!(v["state"], "expired");
    assert!(
        v["unconfirmed"].as_str().unwrap().contains("did not reply"),
        "{v}"
    );
    assert!(
        v["decision"].is_null(),
        "no unknown decision while the expiry may still arrive: {v}"
    );

    let (status, v) = send_again(&s, &q, json!({ "request_id": rid() })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let v = wait_view(&s, &agent, &q, "the decision", |v| {
        v["decision"].is_object()
    })
    .await;
    assert_eq!(v["decision"]["text"], "No answer came, so I decided: Red");
    assert_eq!(v["decision"]["source"], "next-message");
    assert!(v["unconfirmed"].is_null());
}

/// Ruling (Important 1): an answer waits for the question at most the claim wait, then says so plainly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_answer_gives_up_when_the_question_stays_busy() {
    tune();
    let (s, agent) = crate::knobbed::ready_with(Knobbed::default()).await;
    crate::common::m2::say(&s, &agent, "ask:Pick|A|B").await;
    let q = raised(&s, &agent).await;
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
        answer_body("A"),
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
        answer_body("A"),
    )
    .await;
    assert_eq!(
        (status, v["state"].as_str()),
        (StatusCode::OK, Some("expired")),
        "once free, the answer is late: {v}"
    );
}
