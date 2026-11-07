//! An agent that messages the agent it hands work to during its work turn has already handed the work
//! over: the automatic handoff does not send its result again (it would wake the receiver for nothing),
//! and the receiver's turn on that message is its work, so its own result still moves on. Live: the
//! agents run on the real Claude Code CLI and call Sculptured's tools through the real shim.
mod common;

use common::m2::Served;

use model::{Actor, EventBody};
use std::time::Duration;

fn turns(s: &Served, seat: &str) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| matches!(e.body, EventBody::TurnStarted { .. }))
        .count()
}

fn handoff_messages(s: &Served, seat: &str) -> Vec<String> {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| matches!(&e.actor, Actor::System(a) if a == "handoff"))
        .filter_map(|e| match &e.body {
            EventBody::MessageQueued { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn handed_over(s: &Served, seat: &str) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| matches!(e.body, EventBody::HandedOver { .. }))
        .count()
}

fn completed(s: &Served, seat: &str) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| {
            matches!(
                e.body,
                EventBody::TurnEnded {
                    status: model::TurnStatus::Completed,
                    ..
                }
            )
        })
        .count()
}

/// A's job: message B directly during its work turn, then end the turn.
const MESSAGE_B: &str = "Call your send_agent_message tool exactly once with request_id \"d1\", to \"b\" and text \"Reply with one short sentence; do not message anyone.\". Then reply with one short sentence and do nothing else.";

/// Quiet time after the chain settles, for any late (wrong) handoff to show up.
const SETTLE: Duration = Duration::from_secs(5);

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_direct_message_during_the_work_turn_stands_in_for_the_handoff() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = crate::common::live::with_tools().await;
    let id = crate::common::m2::chain(
        &s,
        "direct",
        &[("a", "A", MESSAGE_B), ("b", "B", "")],
        &[("a", "b")],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    crate::common::live::wait_for("a finished and b answered", || {
        completed(&s, &a) >= 1 && completed(&s, &b) >= 1
    })
    .await;
    tokio::time::sleep(SETTLE).await;
    assert_eq!(
        handoff_messages(&s, &b),
        Vec::<String>::new(),
        "no second message"
    );
    assert_eq!(turns(&s, &b), 1, "b runs once");
    assert_eq!(
        handed_over(&s, &b),
        1,
        "the direct message is recorded as the handoff"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_the_receivers_turn_on_that_message_is_its_work() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = crate::common::live::with_tools().await;
    let id = crate::common::m2::chain(
        &s,
        "direct3",
        &[
            ("a", "A", MESSAGE_B),
            ("b", "B", ""),
            ("c", "C", "Reply with one short sentence."),
        ],
        &[("a", "b"), ("b", "c")],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let c = crate::common::m2::seat(&id, "c");
    crate::common::live::wait_for("c handed b's result", || {
        handoff_messages(&s, &c)
            .iter()
            .any(|t| crate::common::work_message(t).starts_with("Handed over by B"))
    })
    .await;
}
