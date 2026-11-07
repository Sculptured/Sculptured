//! A status note ("fyi") to an idle agent does not wake it: it is held and reaches the agent with its
//! next message. Anything else, or a note to a busy agent, is delivered as before. Live: the agents run
//! on the real Claude Code CLI; the test sends A's messages through Sculptured's tool handler.
mod common;

use common::m2::Served;

use model::{EventBody, SeatActivity};
use serde_json::json;
use std::time::Duration;

fn turns(s: &Served, seat: &str) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| matches!(e.body, EventBody::TurnStarted { .. }))
        .count()
}

fn activity(s: &Served, seat: &str) -> Option<SeatActivity> {
    s.st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .map(|v| v.activity)
}

fn last_message(s: &Served, seat: &str) -> String {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .rev()
        .find_map(|e| match &e.body {
            EventBody::MessageQueued { text, .. } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// A cheap job for a live agent.
const SHORT: &str = "Reply with one short sentence.";

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_note_to_an_idle_agent_waits_for_its_next_message() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _sup = crate::common::live::connect(&s).await;
    let id =
        crate::common::m2::chain(&s, "notes", &[("a", "A", SHORT), ("b", "B", SHORT)], &[]).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    crate::common::live::wait_for("b idle after its start", || {
        turns(&s, &b) >= 1 && activity(&s, &b) == Some(SeatActivity::Idle)
    })
    .await;
    let before = turns(&s, &b);
    let (ok, _, error) = crate::common::seat_tools::direct(&s, &a, "send_agent_message", json!({"request_id": "n1", "to": b, "text": "Status: half the chapters are drafted.", "fyi": true})).await;
    assert!(ok, "{error:?}");
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(turns(&s, &b), before, "a note does not wake an idle agent");
    let (ok, _, error) = crate::common::seat_tools::direct(
        &s,
        &a,
        "send_agent_message",
        json!({"request_id": "m1", "to": b, "text": "Please check chapter 3. Reply with one short sentence."}),
    )
    .await;
    assert!(ok, "{error:?}");
    crate::common::live::wait_for("b's next turn", || turns(&s, &b) > before).await;
    let text = last_message(&s, &b);
    assert!(
        text.contains("half the chapters are drafted") && text.contains("Please check chapter 3."),
        "{text}"
    );
    assert_eq!(turns(&s, &b), before + 1, "both in one turn");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_note_to_a_busy_agent_is_delivered_as_before() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _sup = crate::common::live::connect(&s).await;
    let id = crate::common::m2::chain(
        &s,
        "busy",
        &[
            ("a", "A", SHORT),
            ("b", "B", "Use your Bash tool to run exactly: sleep 600"),
        ],
        &[],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    crate::common::live::wait_for("b working", || {
        activity(&s, &b) == Some(SeatActivity::Working)
    })
    .await;
    let (ok, result, error) = crate::common::seat_tools::direct(
        &s,
        &a,
        "send_agent_message",
        json!({"request_id": "n2", "to": b, "text": "Status: fine.", "fyi": true}),
    )
    .await;
    assert!(ok, "{error:?}");
    assert!(
        result["text"].as_str().unwrap().starts_with("Message "),
        "not held: {result}"
    );
    let message = result["message_id"].as_str().unwrap().to_string();
    crate::common::live::wait_for("delivered", || crate::common::m2::events(&s.st, &b).iter().any(|e| matches!(&e.body, EventBody::MessageDelivered { message_id } if *message_id == message))).await;
    crate::common::m2::stop_team(&s, &id).await;
}
