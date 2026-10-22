//! A run paused at its plan budget, and continuing: fake-claude agents with a shared plan file (no
//! model is called).
mod common;
use axum::http::StatusCode;

use common::fake_bin;
use common::m2::{Served, rid};
use controld::budget::BudgetState;
use drivers::claude::ClaudeStreamDriver;
use model::{EventBody, SeatLifecycle};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

async fn team(s: &Served, dir: &Path, job: &str, budget: Value) -> String {
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Pause" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = ["a", "b"].iter().enumerate().map(|(i, id)| json!({ "id": id, "group_id": null, "name": id, "kind": "claude", "job": job, "folder": dir.display().to_string(), "x": 40 + 260 * i as i32, "y": 40 })).collect();
    draft["connections"] = json!([]);
    draft["budget"] = budget;
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

fn state(s: &Served, id: &str) -> BudgetState {
    controld::budget::status(&s.st, id)
        .unwrap()
        .expect("a budget")
        .state
}

async fn wait_long(what: &str, pred: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !pred() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn texts(s: &Served, seat: &str) -> Vec<String> {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::MessageQueued { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn tenures(s: &Served, seat: &str) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| matches!(e.body, EventBody::TenureStarted { .. }))
        .count()
}

#[tokio::test]
async fn a_run_pauses_at_its_plan_budget_and_continues_from_its_notes() {
    let s = crate::common::m2::serve().await;
    let _runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let job = format!(
        "spend:400:1000:120:{}:0.5",
        dir.path().join("plan").display()
    );
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 20.0, "pause": {} }] });
    let id = team(&s, dir.path(), &job, budget).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    wait_long("the run wraps up", || {
        !matches!(state(&s, &id), BudgetState::Normal)
    })
    .await;
    assert!(
        texts(&s, &a)
            .iter()
            .any(|t| t.contains(".sculptured/notes/a.md")),
        "a pausing run's wrap-up asks for a checkpoint note"
    );
    wait_long("the run pauses", || {
        matches!(state(&s, &id), BudgetState::Paused { .. })
    })
    .await;
    let BudgetState::Paused {
        window,
        max_windows,
        reason,
        ..
    } = state(&s, &id)
    else {
        unreachable!()
    };
    assert_eq!((window, max_windows), (1, 3));
    assert!(
        reason.contains("paused until the window resets"),
        "{reason}"
    );
    crate::common::wait_for("both agents stopped", || {
        [&a, &b]
            .iter()
            .all(|x| crate::common::lifecycle(&s.st, x) == Some(SeatLifecycle::Stopped))
    })
    .await;
    let before = (tenures(&s, &a), tenures(&s, &b));
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/budget/paused"),
        json!({ "request_id": rid(), "action": "continue" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    wait_long("both agents start again", || {
        tenures(&s, &a) > before.0 && tenures(&s, &b) > before.1
    })
    .await;
    wait_long("the continuation message", || {
        texts(&s, &a)
            .iter()
            .any(|t| t.contains("continues (window 2 of 3)"))
    })
    .await;
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert_eq!(r.windows, 1);
    assert!(
        !matches!(r.state, BudgetState::Paused { .. }),
        "{:?}",
        r.state
    );
}

#[tokio::test]
async fn while_paused_the_person_can_switch_to_resume_or_stop() {
    let s = crate::common::m2::serve().await;
    let _runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let job = format!(
        "spend:400:1000:120:{}:0.5",
        dir.path().join("plan").display()
    );
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 10.0, "pause": { "max_windows": 2 } }] });
    let id = team(&s, dir.path(), &job, budget).await;
    crate::common::m2::start_team(&s, &id).await;
    wait_long("the run pauses", || {
        matches!(state(&s, &id), BudgetState::Paused { .. })
    })
    .await;
    let url = format!("/api/teams/{id}/budget/paused");
    let post = |body: Value| crate::common::m2::call(&s.router, "POST", &url, body);
    let (status, _) =
        post(json!({ "request_id": rid(), "action": "mode", "continue_with": "resume" })).await;
    assert_eq!(status, StatusCode::OK);
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert_eq!(r.continue_with, Some(model::ContinueMode::Resume));
    let (status, _) = post(json!({ "request_id": rid(), "action": "stop" })).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(state(&s, &id), BudgetState::Stopped { .. }),
        "stopped while paused stays stopped"
    );
    let (status, body) = post(json!({ "request_id": rid(), "action": "continue" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "nothing to continue: {body}");
}

#[tokio::test]
async fn after_the_window_resets_the_run_continues_by_itself_resuming_when_chosen() {
    let s = crate::common::m2::serve().await;
    let _runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let job = format!(
        "spend:400:1000:120:{}:0.5",
        dir.path().join("plan").display()
    );
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 10.0, "pause": { "continue_with": "resume" } }] });
    let id = team(&s, dir.path(), &job, budget).await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    wait_long("the run pauses", || {
        matches!(state(&s, &id), BudgetState::Paused { .. })
    })
    .await;
    crate::common::wait_for("the agents stop", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    // The window's reset time passes.
    let mut r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    if let BudgetState::Paused { until_ms, .. } = &mut r.state {
        *until_ms = model::now_ms() - 1;
    }
    s.st.store.lock().unwrap().put_run_budget(&id, &r).unwrap();
    let resumed = || {
        crate::common::m2::events(&s.st, &a).iter().any(|e| {
            matches!(
                e.body,
                EventBody::TenureStarted {
                    cause: model::TenureCause::Resume,
                    ..
                }
            )
        })
    };
    wait_long("the agent resumes its conversation", resumed).await;
    wait_long("it is told to continue", || {
        texts(&s, &a)
            .iter()
            .any(|t| t.contains("Continue your job where you stopped"))
    })
    .await;
}

#[tokio::test]
async fn a_run_whose_agents_finish_their_wrap_up_below_the_hard_value_pauses_and_continues_with_all_of_them()
 {
    let s = crate::common::m2::serve().await;
    let _runner = crate::common::m2::connect(
        &s,
        vec![Arc::new(ClaudeStreamDriver {
            program: fake_bin("fake-claude"),
        })],
        None,
    )
    .await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    // Each agent spends 12 calls of 0.5%: together past the soft value (10%), short of the hard one (20%).
    let job = format!("spend:12:1000:60:{}:0.5", dir.path().join("plan").display());
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 20.0, "pause": {} }] });
    let id = team(&s, dir.path(), &job, budget).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    wait_long("the run pauses once every agent has wrapped up", || {
        matches!(state(&s, &id), BudgetState::Paused { .. })
    })
    .await;
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert_eq!(
        r.paused_seats.len(),
        2,
        "both agents continue, though they stopped at their wrap-up: {:?}",
        r.paused_seats
    );
    let before = (tenures(&s, &a), tenures(&s, &b));
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/budget/paused"),
        json!({ "request_id": rid(), "action": "continue" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    wait_long("both agents start again", || {
        tenures(&s, &a) > before.0 && tenures(&s, &b) > before.1
    })
    .await;
}
