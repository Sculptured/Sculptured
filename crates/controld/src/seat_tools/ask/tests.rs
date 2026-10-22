//! `approve`'s answers. Through `handle_call`, `approve` needs a restricted Claude agent, which only
//! Task 4's posture setting produces; these call it directly.

use super::{
    Answer, AppState, EventBody, Outcome, Provenance, QuestionSource, QuestionState, ToolError,
    Value, json,
};
use crate::store::Store;
use model::DecisionSource;
use std::time::Duration;

type Call = tokio::task::JoinHandle<Result<Outcome, ToolError>>;

fn state() -> AppState {
    AppState::new(Store::open_in_memory().unwrap(), "t".into(), vec![])
}

/// The id of the first question `seat` raised, once there is one.
async fn raised(st: &AppState, seat: &str) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let events = st
            .store
            .lock()
            .unwrap()
            .events_for_seat(seat, 0, 1000)
            .unwrap();
        if let Some(e) = events
            .into_iter()
            .find(|e| matches!(e.body, EventBody::QuestionRaised { .. }))
        {
            let EventBody::QuestionRaised {
                question_id,
                source,
                ..
            } = e.body
            else {
                unreachable!()
            };
            assert_eq!(
                (source, e.provenance),
                (QuestionSource::NativePermission, Provenance::AgentReported)
            );
            return question_id;
        }
        assert!(tokio::time::Instant::now() < deadline, "no question raised");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Starts an approve call for `seat`; returns it and the question it raised.
async fn approve_call(st: &AppState, seat: &str) -> (Call, String) {
    let args = json!({ "tool_name": "Bash", "input": { "command": "rm -rf build" }, "tool_use_id": "toolu_1" });
    let (st2, seat2) = (st.clone(), seat.to_string());
    let call =
        tokio::spawn(async move { crate::seat_tools::ask::approve(&st2, &seat2, &args).await });
    (call, raised(st, seat).await)
}

async fn behaviour(call: Call) -> Value {
    let Ok((result, _)) = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("the call failed")
    };
    serde_json::from_str(result["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn approve_answers_in_claudes_words() {
    let st = state();
    let (call, q) = approve_call(&st, "team.a@t").await;
    crate::questions::answer(
        &st,
        &q,
        Answer::Deny {
            reason: Some("not today".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        behaviour(call).await,
        json!({ "behavior": "deny", "message": "not today" })
    );
    let (call, q) = approve_call(&st, "team.b@t").await;
    crate::questions::answer(&st, &q, Answer::Allow)
        .await
        .unwrap();
    assert_eq!(
        behaviour(call).await,
        json!({ "behavior": "allow", "updatedInput": { "command": "rm -rf build" } })
    );
}

/// D8: a permission prompt nobody answers is allowed, and that is recorded as the decision.
#[tokio::test]
async fn an_unanswered_approve_is_allowed_on_timeout() {
    let st = state();
    let (call, q) = approve_call(&st, "team.a@t").await;
    crate::questions::expire(&st, &q).await;
    assert_eq!(
        behaviour(call).await,
        json!({ "behavior": "allow", "updatedInput": { "command": "rm -rf build" } })
    );
    let row = crate::projections::questions::question(st.store.lock().unwrap().connection(), &q)
        .unwrap()
        .unwrap();
    assert_eq!(row.state, QuestionState::Expired);
    assert_eq!(
        row.decision.map(|d| d.source),
        Some(DecisionSource::AllowedOnTimeout)
    );
}
