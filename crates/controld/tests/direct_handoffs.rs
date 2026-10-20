//! An agent that messages the agent it hands work to during its work turn has already handed the work
//! over: the automatic handoff does not send its result again (it would wake the receiver for nothing),
//! and the receiver's turn on that message is its work, so its own result still moves on.
mod common;
use common::{m2::*, seat_tools::*, wait_for};
use model::{Actor, EventBody};
use std::time::Duration;

fn turns(s: &Served, seat: &str) -> usize {
    events(&s.st, seat).iter().filter(|e| matches!(e.body, EventBody::TurnStarted { .. })).count()
}

fn handoff_messages(s: &Served, seat: &str) -> Vec<String> {
    events(&s.st, seat)
        .iter()
        .filter(|e| matches!(&e.actor, Actor::System(a) if a == "handoff"))
        .filter_map(|e| match &e.body { EventBody::MessageQueued { text, .. } => Some(text.clone()), _ => None })
        .collect()
}

fn handed_over(s: &Served, seat: &str) -> usize {
    events(&s.st, seat).iter().filter(|e| matches!(e.body, EventBody::HandedOver { .. })).count()
}

fn completed(s: &Served, seat: &str) -> usize {
    events(&s.st, seat).iter().filter(|e| matches!(e.body, EventBody::TurnEnded { status: model::TurnStatus::Completed, .. })).count()
}

const MESSAGE_B: &str = r#"send_agent_message:{"request_id":"d1","to":"b","text":"Build chapter 1."}"#;

#[tokio::test]
async fn a_direct_message_during_the_work_turn_stands_in_for_the_handoff() {
    let (s, _sup, _dir) = with_tools().await;
    let id = practice_chain(&s, "direct", &[("a", "A", MESSAGE_B), ("b", "B", "")], &[("a", "b")]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    wait_for("a finished and b answered", || completed(&s, &a) >= 1 && completed(&s, &b) >= 1).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(handoff_messages(&s, &b), Vec::<String>::new(), "no second message");
    assert_eq!(turns(&s, &b), 1, "b runs once");
    assert_eq!(handed_over(&s, &b), 1, "the direct message is recorded as the handoff");
}

#[tokio::test]
async fn the_receivers_turn_on_that_message_is_its_work() {
    let (s, _sup, _dir) = with_tools().await;
    let id = practice_chain(&s, "direct3", &[("a", "A", MESSAGE_B), ("b", "B", ""), ("c", "C", "Check it")], &[("a", "b"), ("b", "c")]).await;
    start_team(&s, &id).await;
    let c = seat(&id, "c");
    wait_for("c handed b's result", || handoff_messages(&s, &c).iter().any(|t| t.contains("Build chapter 1."))).await;
}
