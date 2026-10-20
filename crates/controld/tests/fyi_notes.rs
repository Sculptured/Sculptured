//! A status note ("fyi") to an idle agent does not wake it: it is held and reaches the agent with its
//! next message. Anything else, or a note to a busy agent, is delivered as before.
mod common;
use common::{m2::*, seat_tools::*, wait_for};
use model::{EventBody, SeatActivity};
use serde_json::json;
use std::time::Duration;

fn turns(s: &Served, seat: &str) -> usize {
    events(&s.st, seat).iter().filter(|e| matches!(e.body, EventBody::TurnStarted { .. })).count()
}

fn activity(s: &Served, seat: &str) -> Option<SeatActivity> {
    s.st.store.lock().unwrap().seat(seat).unwrap().map(|v| v.activity)
}

fn last_message(s: &Served, seat: &str) -> String {
    events(&s.st, seat).iter().rev().find_map(|e| match &e.body { EventBody::MessageQueued { text, .. } => Some(text.clone()), _ => None }).unwrap_or_default()
}

#[tokio::test]
async fn a_note_to_an_idle_agent_waits_for_its_next_message() {
    let (s, _sup, _dir) = with_tools().await;
    let id = practice_chain(&s, "notes", &[("a", "A", "Plan the work"), ("b", "B", "Do the work")], &[]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    wait_for("b idle after its start", || turns(&s, &b) >= 1 && activity(&s, &b) == Some(SeatActivity::Idle)).await;
    let before = turns(&s, &b);
    let (ok, _, error) = direct(&s, &a, "send_agent_message", json!({"request_id": "n1", "to": b, "text": "Status: half the chapters are drafted.", "fyi": true})).await;
    assert!(ok, "{error:?}");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(turns(&s, &b), before, "a note does not wake an idle agent");
    let (ok, _, error) = direct(&s, &a, "send_agent_message", json!({"request_id": "m1", "to": b, "text": "Please check chapter 3."})).await;
    assert!(ok, "{error:?}");
    wait_for("b's next turn", || turns(&s, &b) > before).await;
    let text = last_message(&s, &b);
    assert!(text.contains("half the chapters are drafted") && text.contains("Please check chapter 3."), "{text}");
    assert_eq!(turns(&s, &b), before + 1, "both in one turn");
}

#[tokio::test]
async fn a_note_to_a_busy_agent_is_delivered_as_before() {
    let (s, _sup, _dir) = with_tools().await;
    let id = practice_chain(&s, "busy", &[("a", "A", "Plan the work"), ("b", "B", "hang")], &[]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    wait_for("b working", || activity(&s, &b) == Some(SeatActivity::Working)).await;
    let (ok, result, error) = direct(&s, &a, "send_agent_message", json!({"request_id": "n2", "to": b, "text": "Status: fine.", "fyi": true})).await;
    assert!(ok, "{error:?}");
    let message = result["message_id"].as_str().unwrap().to_string();
    wait_for("delivered", || events(&s.st, &b).iter().any(|e| matches!(&e.body, EventBody::MessageDelivered { message_id } if *message_id == message))).await;
}
