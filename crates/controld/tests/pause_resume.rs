//! Pause and resume, for one agent and for a whole team: a paused agent stops at its next safe point
//! and keeps its session, and Resume continues that session (Practice agents, no model is called).
mod common;
use axum::http::StatusCode;
use common::lifecycle;
use common::m2::{connect_stub, rid, seat, start_team};
use common::pausing::*;
use model::SeatLifecycle;
use serde_json::{Value, json};

#[tokio::test]
async fn pause_agent_then_resume_continues_session() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let id = team(&s, "pause-one", &["a", "b"]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));

    let body = ok(control(&s, &a, "pause").await);
    assert_eq!((affected(&body), skipped(&body)), (vec![a.clone()], vec![]));
    let asked = queued(&s, &a);
    assert!(
        asked
            .iter()
            .any(|t| t.contains("Paused by You") && t.contains(".sculptured/notes/a.md")),
        "the agent is asked to stop at a checkpoint and write its note: {asked:?}"
    );
    wait_paused(&s, &[&a]).await;
    assert_eq!(lifecycle(&s.st, &b), Some(SeatLifecycle::Running));
    let again = ok(control(&s, &a, "pause").await);
    assert_eq!(skipped(&again)[0].1, "already_paused", "a no-op: {again}");
    let kept = session(&s, &a).expect("the paused agent keeps its session");

    let body = ok(control(&s, &a, "resume").await);
    assert_eq!(affected(&body), vec![a.clone()]);
    wait_lifecycle(&s, &[&a], SeatLifecycle::Running).await;
    assert_resumed(&s, &a, Some(kept));
    assert_eq!(mode(&s, &a).await, Value::Null);
}

#[tokio::test]
async fn team_pause_affects_all_live_agents_and_skips_stopped() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let (id, [a, b, c]) = three_with_c_stopped(&s, "pause-all").await;

    let request = rid();
    let body = ok(team_action(&s, &id, "pause", &request).await);
    assert_eq!(affected(&body), vec![a.clone(), b.clone()]);
    let skipped = skipped(&body);
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!((&skipped[0].0, skipped[0].1.as_str()), (&c, "not_running"));
    assert_eq!(skipped[0].2, "It is not running (stopped)");
    let (status, again) = team_action(&s, &id, "pause", &request).await;
    assert_eq!(
        (status, &again),
        (StatusCode::OK, &body),
        "the same request answers the same"
    );

    wait_paused(&s, &[&a, &b]).await;
    assert_eq!(
        mode(&s, &c).await,
        json!("hard"),
        "a stopped agent stays stopped"
    );
}

#[tokio::test]
async fn team_resume_restarts_paused() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let (id, [a, b, c]) = three_with_c_stopped(&s, "resume-all").await;
    ok(team_action(&s, &id, "pause", &rid()).await);
    wait_paused(&s, &[&a, &b]).await;
    let kept = (session(&s, &a), session(&s, &b));

    let body = ok(team_action(&s, &id, "resume", &rid()).await);
    assert_eq!(affected(&body), vec![a.clone(), b.clone()]);
    let skipped = skipped(&body);
    assert_eq!(
        (skipped.len(), &skipped[0].0, skipped[0].1.as_str()),
        (1, &c, "not_paused"),
        "the agent stopped before the pause is not paused"
    );
    wait_lifecycle(&s, &[&a, &b], SeatLifecycle::Running).await;
    assert_resumed(&s, &a, kept.0);
    assert_resumed(&s, &b, kept.1);
    assert_eq!(lifecycle(&s.st, &c), Some(SeatLifecycle::Stopped));
}

#[tokio::test]
async fn pause_when_nothing_running_is_empty_result_not_error() {
    let s = common::m2::serve().await;
    let _runner = connect_stub(&s).await;
    let never = team(&s, "never-started", &["a"]).await;
    for action in ["pause", "resume"] {
        let body = ok(team_action(&s, &never, action, &rid()).await);
        assert_eq!(body, json!({ "affected": [], "skipped": [] }), "{action}");
    }

    let id = team(&s, "all-stopped", &["a", "b"]).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    common::m2::stop_team(&s, &id).await;
    wait_lifecycle(&s, &[&a, &b], SeatLifecycle::Stopped).await;
    for action in ["pause", "resume"] {
        let body = ok(team_action(&s, &id, action, &rid()).await);
        assert!(affected(&body).is_empty(), "{action}: {body}");
        assert_eq!(skipped(&body).len(), 2, "{action}: {body}");
    }

    let (status, body) = team_action(&s, "no-such-team", "pause", &rid()).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
