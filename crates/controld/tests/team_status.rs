//! team_status is read into an agent's context every time it is called: one short line per agent.
mod common;

use common::m2::seat;

use model::{EventBody, SeatActivity};
use serde_json::json;

#[tokio::test]
async fn team_status_is_one_short_line_per_agent() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_chain(
        &s,
        "status",
        &[("a", "Planner", "Plan the work"), ("b", "Builder", "hang")],
        &[],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    let working = |seat: &str| {
        s.st.store
            .lock()
            .unwrap()
            .seat(seat)
            .unwrap()
            .is_some_and(|v| v.activity == SeatActivity::Working)
    };
    crate::common::wait_for("b working", || {
        working(&b)
            && crate::common::m2::events(&s.st, &a)
                .iter()
                .any(|e| matches!(e.body, EventBody::TurnEnded { .. }))
    })
    .await;
    let (ok, result, error) =
        crate::common::seat_tools::direct(&s, &a, "team_status", json!({})).await;
    assert!(ok, "{error:?}");
    let text = result["text"].as_str().unwrap();
    assert!(text.lines().any(|l| l == "- a · Planner · idle"), "{text}");
    assert!(
        text.lines().any(|l| l == "- b · Builder · working"),
        "{text}"
    );
    assert!(!text.contains('{'), "plain lines, not escaped JSON: {text}");
    assert!(text.len() < 300, "{} chars: {text}", text.len());
}
