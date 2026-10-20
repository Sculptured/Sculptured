//! team_status is read into an agent's context every time it is called: one short line per agent.
mod common;
use common::{m2::*, seat_tools::*, wait_for};
use model::{EventBody, SeatActivity};
use serde_json::json;

#[tokio::test]
async fn team_status_is_one_short_line_per_agent() {
    let (s, _sup, _dir) = with_tools().await;
    let id = practice_chain(&s, "status", &[("a", "Planner", "Plan the work"), ("b", "Builder", "hang")], &[]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    let working = |seat: &str| s.st.store.lock().unwrap().seat(seat).unwrap().is_some_and(|v| v.activity == SeatActivity::Working);
    wait_for("b working", || working(&b) && events(&s.st, &a).iter().any(|e| matches!(e.body, EventBody::TurnEnded { .. }))).await;
    let (ok, result, error) = direct(&s, &a, "team_status", json!({})).await;
    assert!(ok, "{error:?}");
    let text = result["text"].as_str().unwrap();
    assert!(text.lines().any(|l| l == "- a · Planner · idle"), "{text}");
    assert!(text.lines().any(|l| l == "- b · Builder · working"), "{text}");
    assert!(!text.contains('{'), "plain lines, not escaped JSON: {text}");
    assert!(text.len() < 300, "{} chars: {text}", text.len());
}
