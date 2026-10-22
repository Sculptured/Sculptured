//! A seat-tool question whose host connection closes while controld runs (the computer slept, its
//! network changed): the runner keeps the agent's call and sends it again when it reconnects, so the
//! question stays open. An answer that cannot reach the call is recorded as not delivered.
mod common;

use common::m2::Served;

use model::{Answer, EventBody};

fn answer_failed(s: &Served, seat: &str, id: &str) -> Option<String> {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::QuestionAnswerFailed {
                question_id,
                reason,
            } if question_id == id => Some(reason),
            _ => None,
        })
}

/// A running Practice agent (its host has no tool socket: these tests call the tools directly).
async fn agent(s: &Served) -> String {
    crate::common::m2::connect_stub(s).await;
    crate::common::m2::seat(&crate::common::m2::practice_team(s, "solo").await, "claude")
}

/// The question stays open, still waiting for its answer.
#[tokio::test]
async fn a_dropped_host_connection_keeps_the_question_open() {
    let s = crate::common::m2::serve().await;
    let seat = "team.a@t";
    let (call, link, q) = crate::common::seat_tools::which(&s, seat).await;
    drop(link);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(!call.is_finished());
    assert!(s.st.seat_waits.lock().unwrap().contains_key(&q));
    assert_eq!(
        crate::common::seat_tools::question_view(&s, seat, &q).await["state"],
        "open"
    );
}

/// Concern 4: the answer reached the waiting call, but its connection closed before the reply went
/// out. controld records the answer as failed and withdraws the question.
#[tokio::test]
async fn an_answer_whose_reply_cannot_go_out_is_recorded_as_failed() {
    let s = crate::common::m2::serve().await;
    let agent = agent(&s).await;
    let (call, link, q) = crate::common::seat_tools::which(&s, &agent).await;
    controld::questions::answer(
        &s.st,
        &q,
        Answer::Text {
            text: "Green".into(),
        },
    )
    .await
    .unwrap();
    // Closed before the waiting call runs again to send its reply.
    drop(link);
    crate::common::seat_tools::ended(call).await;
    let reason = answer_failed(&s, &agent, &q).expect("the lost answer is recorded");
    assert!(
        reason.contains("could not pass the answer on to the agent"),
        "{reason}"
    );
    assert_eq!(
        crate::common::seat_tools::question_view(&s, &agent, &q).await["state"],
        "withdrawn"
    );
}
