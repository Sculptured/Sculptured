//! A run paused at its plan budget, and continuing. When it pauses is checked on plan readings written
//! to the store (`budget_store`); stopping and starting its agents again runs live agents on the real
//! Claude Code CLI with its cheapest model (`common::live`). A real account's 5-hour use cannot be moved
//! on cue, so the live tests write the account's readings into the store, dated ahead of the CLIs' own
//! readings so that theirs never replace them.
#[path = "common/budget_store.rs"]
mod budget_store;
mod common;
use axum::http::StatusCode;

use budget_store::quiet;
use common::m2::{Served, rid};
use controld::budget::BudgetState;
use model::{EventBody, SeatLifecycle};
use serde_json::{Value, json};

const OK_JOB: &str = "Reply with the single word OK and nothing else.";

/// A team of two live Claude agents, `a` and `b` (on haiku, in `live::work_folder`), with `budget`.
async fn team(s: &Served, budget: Value) -> String {
    let folder = common::live::work_folder(s);
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Pause" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = ["a", "b"].iter().enumerate().map(|(i, id)| json!({ "id": id, "group_id": null, "name": id, "kind": "claude", "job": OK_JOB, "folder": folder, "x": 40 + 260 * i as i32, "y": 40 })).collect();
    draft["connections"] = json!([]);
    draft["models"] = json!({ "claude": common::live::CLAUDE_MODEL });
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

/// The signed-in account's Claude 5-hour use, written into the store as a CLI's report would be. Its
/// readings are dated an hour ahead, so the live CLIs' own (older) readings never replace them; the
/// window resets two hours from now.
struct Account {
    seq: u64,
    at_ms: i64,
    resets_at: i64,
}

impl Account {
    fn new() -> Self {
        Self {
            seq: 0,
            at_ms: model::now_ms() + 3_600_000,
            resets_at: model::now_ms() / 1000 + 7200,
        }
    }

    fn read(&mut self, s: &Served, percent: f64) {
        self.seq += 1;
        self.at_ms += 1000;
        let usage = model::PlanUsage {
            provider: "claude".into(),
            windows: vec![model::PlanWindow {
                name: "five_hour".into(),
                used_percent: Some(percent),
                resets_at: Some(self.resets_at),
            }],
            status: Some("allowed".into()),
            credits: None,
        };
        let ev = host_proto::HostEvent {
            host_seq: self.seq,
            at_ms: self.at_ms,
            actor: model::Actor::System("test".into()),
            subject: model::Subject::Host("account".into()),
            provenance: model::Provenance::ProtocolObserved,
            body: EventBody::PlanUsageReported { usage },
        };
        s.st.store.lock().unwrap().ingest("account", ev).unwrap();
    }
}

fn state(s: &Served, id: &str) -> BudgetState {
    controld::budget::status(&s.st, id)
        .unwrap()
        .expect("a budget")
        .state
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

fn changes(s: &Served, id: &str) -> Vec<String> {
    s.st.store
        .lock()
        .unwrap()
        .latest_of_types(
            &model::Subject::Rig(id.to_string()),
            &["budget-state-changed"],
            20,
        )
        .unwrap()
        .iter()
        .rev()
        .filter_map(|e| match &e.body {
            EventBody::BudgetStateChanged { state, .. } => Some(state.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn plan_readings_past_the_hard_value_pause_a_run_with_windows_left() {
    let mut q = quiet();
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 20.0, "pause": {} }] });
    let id = q.team(&["a", "b"], budget, |_| {}).await;
    let a = budget_store::seat(&id, "a");
    let (now, resets) = (model::now_ms(), model::now_ms() / 1000 + 3600);
    q.start(&id);
    q.plan(&a, 1.0, resets, now);
    controld::budget::check(&q.st).await.unwrap();
    assert_eq!(q.state(&id), BudgetState::Normal);
    q.plan(&a, 12.0, resets, now + 1);
    controld::budget::check(&q.st).await.unwrap();
    assert!(
        matches!(q.state(&id), BudgetState::WrapUp { .. }),
        "{:?}",
        q.state(&id)
    );
    assert_eq!(
        q.record(&id).paused_seats.len(),
        2,
        "the agents working at the wrap-up are the ones it continues with"
    );
    q.plan(&a, 25.0, resets, now + 2);
    controld::budget::check(&q.st).await.unwrap();
    let BudgetState::Paused {
        window,
        max_windows,
        reason,
        until_ms,
        ..
    } = q.state(&id)
    else {
        panic!("{:?}", q.state(&id))
    };
    assert_eq!((window, max_windows), (1, 3));
    assert!(
        reason.contains("paused until the window resets"),
        "{reason}"
    );
    assert_eq!(
        until_ms,
        resets * 1000 + 120_000,
        "until two minutes after the reset"
    );
    assert_eq!(q.changes(&id), ["wrap-up", "paused"]);
}

#[tokio::test]
async fn a_pausing_run_whose_agents_all_stopped_after_their_wrap_up_pauses() {
    let mut q = quiet();
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 20.0, "pause": {} }] });
    let id = q.team(&["a", "b"], budget, |_| {}).await;
    let (a, b) = (budget_store::seat(&id, "a"), budget_store::seat(&id, "b"));
    let (now, resets) = (model::now_ms(), model::now_ms() / 1000 + 3600);
    q.start(&id);
    q.plan(&a, 1.0, resets, now);
    controld::budget::check(&q.st).await.unwrap();
    q.plan(&a, 12.0, resets, now + 1);
    controld::budget::check(&q.st).await.unwrap();
    assert!(matches!(q.state(&id), BudgetState::WrapUp { .. }));
    // Both finish their wrap-up and stop, short of the hard value.
    for seat in [&a, &b] {
        q.host(
            seat,
            EventBody::SeatLifecycleChanged {
                lifecycle: SeatLifecycle::Stopped,
                reason: None,
            },
        );
    }
    controld::budget::check(&q.st).await.unwrap();
    let BudgetState::Paused { reason, .. } = q.state(&id) else {
        panic!("{:?}", q.state(&id))
    };
    assert!(
        reason.contains("every agent wrapped up at the soft value"),
        "{reason}"
    );
    assert_eq!(
        q.record(&id).paused_seats.len(),
        2,
        "both agents continue, though they stopped at their wrap-up"
    );
}

#[tokio::test]
async fn while_paused_the_person_can_switch_to_resume_or_stop() {
    let mut q = quiet();
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 10.0, "pause": { "max_windows": 2 } }] });
    let id = q.team(&["a", "b"], budget, |_| {}).await;
    let a = budget_store::seat(&id, "a");
    let (now, resets) = (model::now_ms(), model::now_ms() / 1000 + 3600);
    q.start(&id);
    q.plan(&a, 1.0, resets, now);
    controld::budget::check(&q.st).await.unwrap();
    q.plan(&a, 12.0, resets, now + 1);
    controld::budget::check(&q.st).await.unwrap();
    assert!(
        matches!(q.state(&id), BudgetState::Paused { .. }),
        "{:?}",
        q.state(&id)
    );
    let url = format!("/api/teams/{id}/budget/paused");
    let post = |body: Value| budget_store::call(&q.router, "POST", &url, body);
    let (status, _) =
        post(json!({ "request_id": rid(), "action": "mode", "continue_with": "resume" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        q.record(&id).continue_with,
        Some(model::ContinueMode::Resume)
    );
    let (status, _) = post(json!({ "request_id": rid(), "action": "stop" })).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        matches!(q.state(&id), BudgetState::Stopped { .. }),
        "stopped while paused stays stopped"
    );
    let (status, body) = post(json!({ "request_id": rid(), "action": "continue" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "nothing to continue: {body}");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn live_a_run_pauses_at_its_plan_budget_and_continues_from_its_notes() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let mut account = Account::new();
    account.read(&s, 1.0);
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 20.0, "pause": {} }] });
    let id = team(&s, budget).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    account.read(&s, 12.0);
    common::live::wait_for("the run wraps up", || {
        !matches!(state(&s, &id), BudgetState::Normal)
    })
    .await;
    common::live::wait_for("a pausing run's wrap-up asks for a checkpoint note", || {
        texts(&s, &a)
            .iter()
            .any(|t| t.contains(".sculptured/notes/a.md"))
    })
    .await;
    account.read(&s, 25.0);
    common::live::wait_for("the run pauses", || {
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
    common::live::wait_for("both agents stopped", || {
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
    common::live::wait_for("both agents start again", || {
        tenures(&s, &a) > before.0 && tenures(&s, &b) > before.1
    })
    .await;
    common::live::wait_for("the continuation message", || {
        texts(&s, &a)
            .iter()
            .any(|t| t.contains("continues (window 2 of 3)"))
    })
    .await;
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert_eq!(r.windows, 1);
    let log = changes(&s, &id);
    let paused = log.iter().position(|c| c == "paused").expect("paused");
    assert!(
        log[paused..].iter().any(|c| c == "continued"),
        "the run's log says it continued: {log:?}"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_after_the_window_resets_the_run_continues_by_itself_resuming_when_chosen() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let mut account = Account::new();
    account.read(&s, 1.0);
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 10.0, "pause": { "continue_with": "resume" } }] });
    let id = team(&s, budget).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    account.read(&s, 12.0);
    common::live::wait_for("the run pauses", || {
        matches!(state(&s, &id), BudgetState::Paused { .. })
    })
    .await;
    common::live::wait_for("the agents stop", || {
        [&a, &b]
            .iter()
            .all(|x| crate::common::lifecycle(&s.st, x) == Some(SeatLifecycle::Stopped))
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
    common::live::wait_for("the agent resumes its conversation", resumed).await;
    common::live::wait_for("it is told to continue", || {
        texts(&s, &a)
            .iter()
            .any(|t| t.contains("Continue your job where you stopped"))
    })
    .await;
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_run_whose_agents_finish_their_wrap_up_below_the_hard_value_pauses_and_continues_with_all_of_them()
 {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _runner = common::live::connect(&s).await;
    let mut account = Account::new();
    account.read(&s, 1.0);
    // Past the soft value (10%) and far short of the hard one (99%): the agents wrap up and stop.
    let budget = json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 99.0, "pause": {} }] });
    let id = team(&s, budget).await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    account.read(&s, 12.0);
    common::live::wait_for("the run pauses once every agent has wrapped up", || {
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
    common::live::wait_for("both agents start again", || {
        tenures(&s, &a) > before.0 && tenures(&s, &b) > before.1
    })
    .await;
}
