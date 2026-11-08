//! Run budgets. What a budget counts and decides is checked on usage and plan readings written to the
//! store (`budget_store`); what it does to running agents (wrap-up, stop, slots, raise, resume) runs
//! live agents on the real Claude Code CLI with its cheapest model (`common::live`). Live budgets are
//! small enough that whatever a real model call reports passes them.
#[path = "common/budget_store.rs"]
mod budget_store;
mod common;
use axum::http::StatusCode;

use budget_store::{Quiet, quiet};
use common::m2::{Served, rid};
use controld::budget::BudgetState;
use model::SeatLifecycle;
use serde_json::{Value, json};
use std::time::Duration;

/// The job of a live agent that needs only one cheap turn.
const OK_JOB: &str = "Reply with the single word OK and nothing else.";
/// The job of a live agent that should still be in its turn when the test acts.
const HANG_JOB: &str = "Use your Bash tool to run exactly: sleep 600";

/// A team of independent live Claude agents (on haiku, in `live::work_folder`), each with its own job,
/// and `budget`.
async fn team(s: &Served, jobs: &[(&str, &str)], budget: Value) -> String {
    let folder = common::live::work_folder(s);
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Spend" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = jobs
        .iter()
        .enumerate()
        .map(|(i, (id, job))| {
            json!({ "id": id, "group_id": null, "name": id, "kind": "claude",
        "job": job, "folder": folder, "x": 40 + 260 * i as i32, "y": 40 })
        })
        .collect();
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

/// A served controld with a live host (the real CLIs).
async fn live_served() -> (Served, hostd::supervisor::Supervisor) {
    let s = crate::common::m2::serve().await;
    let runner = common::live::connect(&s).await;
    (s, runner)
}

fn state(s: &Served, id: &str) -> BudgetState {
    controld::budget::status(&s.st, id)
        .unwrap()
        .expect("a budget")
        .state
}

/// Runs the budget check until `done` holds, after each of at most `steps` calls of `step`.
async fn spend_until(
    q: &mut Quiet,
    steps: usize,
    mut step: impl FnMut(&mut Quiet, usize),
    mut done: impl FnMut(&Quiet) -> bool,
) -> usize {
    for i in 0..steps {
        step(q, i);
        controld::budget::check(&q.st).await.unwrap();
        if done(q) {
            return i + 1;
        }
    }
    panic!("not done after {steps} steps");
}

#[tokio::test]
async fn plan_readings_wrap_a_run_up_at_soft_and_stop_it_at_hard_within_the_bound() {
    let mut q = quiet();
    let id = q
        .team(
            &["a", "b"],
            json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 20.0 }] }),
            |_| {},
        )
        .await;
    let (a, b) = (budget_store::seat(&id, "a"), budget_store::seat(&id, "b"));
    let (now, resets) = (model::now_ms(), model::now_ms() / 1000 + 3600);
    q.start(&id);
    q.turn_started(&a);
    q.turn_started(&b);
    // The account's first reading (0.5%) is the run's baseline.
    q.plan(&a, 0.5, resets, now);
    controld::budget::check(&q.st).await.unwrap();
    // Each call: 1010 tokens and 0.5% of the window, the two agents taking turns.
    let mut wrapped_up = false;
    spend_until(
        &mut q,
        400,
        |q, i| {
            let seat = if i % 2 == 0 { &a } else { &b };
            q.usage(seat, 10, 1000);
            q.plan(seat, 0.5 + 0.5 * (i + 1) as f64, resets, now + 1 + i as i64);
        },
        |q| {
            wrapped_up |= matches!(q.state(&id), BudgetState::WrapUp { .. });
            matches!(q.state(&id), BudgetState::Stopped { .. })
        },
    )
    .await;
    assert!(wrapped_up, "it wrapped up before it stopped");
    let r = q.record(&id);
    assert!(r.wrap_up_ms.is_some());
    let BudgetState::Stopped { reason, .. } = &r.state else {
        unreachable!()
    };
    assert!(
        reason.starts_with("Claude 5-hour:") && reason.contains("hard limit of 20.0%"),
        "{reason}"
    );
    // The stop comes one call per working agent early (the reserve), so only in-flight calls pass it.
    let account = r.plans[0].last.unwrap().percent - 0.5;
    assert!(
        account <= 20.0 + 2.0 * 0.5,
        "the run added {account:.1}% against a 20% hard limit"
    );
    assert!(
        account >= 15.0,
        "it ran most of its budget first: {account:.1}%"
    );
    let rate: String =
        q.st.store
            .lock()
            .unwrap()
            .connection()
            .query_row(
                "SELECT json FROM settings WHERE k = 'budget-rate:claude:five_hour'",
                [],
                |r| r.get(0),
            )
            .unwrap();
    assert!(
        (rate.parse::<f64>().unwrap() - 0.5 / 1010.0).abs() < 1e-6,
        "the learned rate is kept per plan window: {rate}"
    );
    assert_eq!(q.changes(&id), ["wrap-up", "stopped"]);
}

#[tokio::test]
async fn with_no_plan_reading_a_learned_rate_still_stops_the_run() {
    let mut q = quiet();
    // An earlier run learned 0.001% per token; this run's CLIs report no plan readings at all.
    q.st.store
        .lock()
        .unwrap()
        .connection()
        .execute(
            "INSERT INTO settings (k, json) VALUES ('budget-rate:claude:five_hour', '0.001')",
            [],
        )
        .unwrap();
    let id = q
        .team(
            &["a"],
            json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 10.0 }] }),
            |_| {},
        )
        .await;
    let a = budget_store::seat(&id, "a");
    q.start(&id);
    q.turn_started(&a);
    spend_until(
        &mut q,
        400,
        |q, _| q.usage(&a, 10, 1000),
        |q| matches!(q.state(&id), BudgetState::Stopped { .. }),
    )
    .await;
    let st = controld::budget::status(&q.st, &id).unwrap().unwrap();
    assert!(st.meters[0].estimated, "{:?}", st.meters);
    let r = q.record(&id);
    assert!(
        r.peak_tokens <= 10_000 + 2 * 1010,
        "10% at 0.001%/token is 10 000 tokens: {}",
        r.peak_tokens
    );
}

#[tokio::test]
async fn token_and_time_budgets_enforce_without_plan_readings() {
    let mut q = quiet();
    let id = q
        .team(&["a"], json!({ "tokens": { "hard": 30000 } }), |_| {})
        .await;
    let a = budget_store::seat(&id, "a");
    q.start(&id);
    q.turn_started(&a);
    spend_until(
        &mut q,
        400,
        |q, _| q.usage(&a, 10, 1000),
        |q| matches!(q.state(&id), BudgetState::Stopped { .. }),
    )
    .await;
    let r = q.record(&id);
    assert!(
        r.wrap_up_ms.is_some(),
        "85% of 30 000 tokens wrapped up first"
    );
    assert!(r.peak_tokens <= 30_000 + 1010, "{}", r.peak_tokens);
    assert_eq!(
        q.changes(&id),
        ["wrap-up", "stopped"],
        "the run's log records each change, for reports of the run"
    );

    let id = q
        .team(
            &["a"],
            json!({ "minutes": { "soft": 0, "hard": 1 } }),
            |_| {},
        )
        .await;
    q.start(&id);
    controld::budget::check(&q.st).await.unwrap();
    assert!(
        matches!(q.state(&id), BudgetState::WrapUp { .. }),
        "a soft time limit of 0 wraps up at once"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_token_budget_stops_a_working_agent_at_once() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    // Any real model call passes 1 token: the first usage report is already past the hard value.
    let id = team(&s, &[("a", HANG_JOB)], json!({ "tokens": { "hard": 1 } })).await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("the token budget stops the run", || {
        matches!(state(&s, &id), BudgetState::Stopped { .. })
    })
    .await;
    common::live::wait_for("the agent is stopped mid-turn", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    let BudgetState::Stopped { reason, .. } = &r.state else {
        unreachable!()
    };
    assert!(reason.starts_with("Tokens:"), "{reason}");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_past_the_soft_value_every_agent_wraps_up_then_stops() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let id = team(
        &s,
        &[("a", OK_JOB), ("b", OK_JOB)],
        json!({ "tokens": { "soft": 1, "hard": 1_000_000_000_u64 } }),
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    common::live::wait_for("the run wraps up", || {
        matches!(state(&s, &id), BudgetState::WrapUp { .. })
    })
    .await;
    for agent in ["a", "b"] {
        let seat = crate::common::m2::seat(&id, agent);
        common::live::wait_for("the agent is asked to wrap up", || {
            crate::common::m2::events(&s.st, &seat).iter().any(|e| matches!(&e.body, model::EventBody::MessageQueued { text, .. } if text.contains("Soft stop requested")))
        })
        .await;
        common::live::wait_for("the agent stops after its wrap-up turn", || {
            crate::common::lifecycle(&s.st, &seat) == Some(SeatLifecycle::Stopped)
        })
        .await;
    }
    assert!(
        matches!(state(&s, &id), BudgetState::WrapUp { .. }),
        "nothing reached the hard value"
    );
}

/// Turn intervals per agent, from its TurnStarted/TurnEnded events: (start seq, end seq).
fn turns(s: &Served, seat: &str) -> Vec<(i64, i64)> {
    let ev = crate::common::m2::events(&s.st, seat);
    let mut out = vec![];
    let mut open = None;
    for e in ev {
        match e.body {
            model::EventBody::TurnStarted { .. } => open = Some(e.seq),
            model::EventBody::TurnEnded { .. } => out.extend(open.take().map(|o| (o, e.seq))),
            _ => {}
        }
    }
    out
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_delivery_waits_for_a_slot_then_runs() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let id = team(
        &s,
        &[("a", OK_JOB), ("b", OK_JOB), ("c", OK_JOB)],
        json!({ "max_working": 1 }),
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let seats: Vec<String> = ["a", "b", "c"]
        .iter()
        .map(|a| crate::common::m2::seat(&id, a))
        .collect();
    common::live::wait_for("every agent ran its job", || {
        seats.iter().all(|x| !turns(&s, x).is_empty())
    })
    .await;
    let mut all: Vec<(i64, i64)> = seats.iter().flat_map(|x| turns(&s, x)).collect();
    all.sort();
    assert!(
        all.windows(2).all(|w| w[0].1 < w[1].0),
        "one agent worked at a time: {all:?}"
    );
}

#[tokio::test]
async fn every_path_to_new_work_is_refused_past_the_soft_value() {
    use controld::budget::gate::{Gate, decide};
    let mut q = quiet();
    let id = q
        .team(
            &["a", "b"],
            json!({ "tokens": { "hard": 1000000 } }),
            |_| {},
        )
        .await;
    q.start(&id);
    let a = budget_store::seat(&id, "a");
    {
        let store = q.st.store.lock().unwrap();
        let mut r = store.run_budget(&id).unwrap().unwrap();
        r.state = BudgetState::WrapUp {
            reason: "Tokens: test".into(),
            at_ms: 1,
        };
        store.put_run_budget(&id, &r).unwrap();
    }
    // A person's message, through the API.
    let (status, body) = budget_store::call(
        &q.router,
        "POST",
        &format!("/api/seats/{a}/send"),
        json!({ "request_id": rid(), "text": "more", "intent": "inform" }),
    )
    .await;
    assert!(
        status.is_client_error() || status.is_server_error(),
        "{status} {body}"
    );
    assert!(
        body.to_string().contains("Run budget: Tokens: test"),
        "{body}"
    );
    // Every command that starts work, whoever sends it: the gate sits in front of every runner command.
    let store = q.st.store.lock().unwrap();
    let send = host_proto::HostCommand::Send {
        seat: a.clone(),
        message_id: "m".into(),
        intent: model::MessageIntent::Inform,
        text: "x".into(),
    };
    assert!(matches!(decide(&store, &send).unwrap(), Gate::Refuse(r) if r.contains("Run budget")));
    let interrupt = host_proto::HostCommand::Interrupt { seat: a.clone() };
    assert_eq!(
        decide(&store, &interrupt).unwrap(),
        Gate::Pass,
        "interrupts and stops are never held"
    );
    assert!(
        controld::budget::gate::refusal(&store, &id)
            .unwrap()
            .is_some(),
        "spawn_agent refuses before creating a child"
    );
    store
        .record_control(&a, controld::store::ControlMode::Soft, 0)
        .unwrap();
    assert_eq!(
        decide(&store, &send).unwrap(),
        Gate::Pass,
        "the wrap-up instruction reaches an agent that is wrapping up"
    );
}

/// Sets `advanced` on agent `agent` of team `id` (e.g. its limits).
async fn with_advanced(s: &Served, id: &str, agent: &str, advanced: Value) {
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    let a = draft["agents"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["id"] == agent)
        .unwrap();
    let matching_advanced = advanced.as_object().unwrap();
    for (k, v) in matching_advanced {
        a["advanced"][k] = v.clone();
    }
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn tokens_of(s: &Served, seat: &str) -> u64 {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .map(|e| match e.body {
            model::EventBody::UsageReported {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                ..
            } => input_tokens + output_tokens + cache_read_tokens + cache_write_tokens,
            _ => 0,
        })
        .sum()
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_agent_wraps_up_at_its_soft_tokens_and_stops_after() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let id = team(&s, &[("a", OK_JOB)], json!({})).await;
    // Any real model call passes the soft value; none comes near the hard one.
    with_advanced(
        &s,
        &id,
        "a",
        json!({ "token_limit": { "tokens": 1_000_000_000_u64, "soft": 1 } }),
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("the agent is stopped", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let asked = crate::common::m2::events(&s.st, &a).iter().any(|e| matches!(&e.body, model::EventBody::MessageQueued { text, .. } if text.contains("Soft stop requested")));
    assert!(asked, "it was asked to wrap up past its soft value");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_turn_limit_wraps_up_then_stops_the_agent() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let id = team(&s, &[("a", OK_JOB)], json!({})).await;
    with_advanced(&s, &id, "a", json!({ "turn_limit": { "hard": 2 } })).await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    // Soft is 85% of 2 = 1 turn: after its job the agent is asked to wrap up (its 2nd turn), then stopped.
    common::live::wait_for("the agent is stopped", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    assert_eq!(
        turns(&s, &a).len(),
        2,
        "its job, then its wrap-up turn, both finished: {:?}",
        turns(&s, &a)
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_new_run_without_a_budget_is_not_governed_by_the_last_runs_record() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    // A real model call reads far more than 3000 tokens: the first run is stopped.
    let id = team(
        &s,
        &[("a", OK_JOB)],
        json!({ "tokens": { "hard": 3000 }, "max_working": 1 }),
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    common::live::wait_for("the first run is stopped", || {
        matches!(state(&s, &id), BudgetState::Stopped { .. })
    })
    .await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("its agent stopped", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft.as_object_mut().unwrap().remove("budget");
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = turns(&s, &a).len();
    crate::common::m2::start_team(&s, &id).await;
    common::live::wait_for("the new run's job ran", || turns(&s, &a).len() > before).await;
    assert!(
        controld::budget::status(&s.st, &id).unwrap().is_none(),
        "the board shows no budget for this run"
    );
    let refused = crate::common::m2::events(&s.st, &a).iter().any(|e| matches!(&e.body, model::EventBody::MessageFailed { reason, .. } if reason.contains("Run budget")));
    assert!(!refused, "nothing was refused by the old run's budget");
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_output_only_token_limit_is_not_tripped_by_cache_reads() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    // A one-word reply writes a few output tokens but reads thousands (instructions, cache).
    let id = team(&s, &[("a", OK_JOB)], json!({})).await;
    with_advanced(
        &s,
        &id,
        "a",
        json!({ "token_limit": { "tokens": 1000, "counts": "output", "include_subagents": true } }),
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("its job finished", || !turns(&s, &a).is_empty()).await;
    assert!(tokens_of(&s, &a) > 1000, "it read more than the limit");
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(
        crate::common::lifecycle(&s.st, &a),
        Some(SeatLifecycle::Running),
        "a few output tokens of 1000 stop nothing"
    );
    assert_eq!(s.st.store.lock().unwrap().control(&a).unwrap(), None);
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stop_everything_now_stops_every_agent_at_once_and_the_board_sees_the_meters() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let id = team(
        &s,
        &[("a", HANG_JOB), ("b", HANG_JOB)],
        json!({ "tokens": { "hard": 10000000 }, "max_working": 5 }),
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (
        crate::common::m2::seat(&id, "a"),
        crate::common::m2::seat(&id, "b"),
    );
    common::live::wait_for("both are spending", || {
        tokens_of(&s, &a) > 0 && tokens_of(&s, &b) > 0
    })
    .await;
    let (status, body) =
        crate::common::m2::get(&s.router, &format!("/api/teams/{id}/budget")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["run"]["meters"][0]["what"], "Tokens", "{body}");
    assert_eq!(body["run"]["state"]["state"], "normal");
    assert_eq!(body["waiting"], json!([]));
    let t = std::time::Instant::now();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/stop-now"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(
        (status, body["stopped"].as_u64()),
        (StatusCode::OK, Some(2)),
        "{body}"
    );
    crate::common::wait_for("both stopped", || {
        [&a, &b]
            .iter()
            .all(|x| crate::common::lifecycle(&s.st, x) == Some(SeatLifecycle::Stopped))
    })
    .await;
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}

fn tenures(s: &Served, seat: &str) -> Vec<(model::TenureCause, Option<String>)> {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .filter_map(|e| match e.body {
            model::EventBody::TenureStarted {
                cause,
                native_session_id,
                ..
            } => Some((cause, native_session_id)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_start_can_resume_each_agents_last_conversation_or_start_fresh() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let id = team(&s, &[("a", OK_JOB)], json!({})).await;
    let a = crate::common::m2::seat(&id, "a");
    let (_, body) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/resumable")).await;
    assert_eq!(
        body["agents"],
        json!([]),
        "nothing to resume before a first run"
    );
    crate::common::m2::start_team(&s, &id).await;
    common::live::wait_for("the job ran", || turns(&s, &a).len() == 1).await;
    crate::common::m2::stop_team(&s, &id).await;
    common::live::wait_for("stopped", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let first = tenures(&s, &a)[0].1.clone().expect("a session id");

    let (_, body) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/resumable")).await;
    assert_eq!(body["agents"], json!([{ "seat": a, "name": "a" }]));
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid(), "resume": true }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("the resumed job ran", || turns(&s, &a).len() == 2).await;
    assert_eq!(
        tenures(&s, &a)[1],
        (model::TenureCause::Resume, Some(first.clone())),
        "the agent continued its own conversation"
    );
    crate::common::m2::stop_team(&s, &id).await;
    common::live::wait_for("stopped again", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;

    crate::common::m2::start_team(&s, &id).await;
    common::live::wait_for("the fresh job ran", || turns(&s, &a).len() == 3).await;
    let t = &tenures(&s, &a)[2];
    assert!(
        t.0 != model::TenureCause::Resume && t.1.as_deref() != Some(first.as_str()),
        "a plain Start begins a new conversation: {t:?}"
    );
}

/// Protections: an agent past its team's context ceiling writes its notes between turns, then
/// continues in a fresh conversation. The ceiling is set low (30k) and the agent reads three files of
/// about 15k tokens each, so its context more than doubles from its start.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn live_an_agent_past_the_context_ceiling_continues_fresh_from_its_notes() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let folder = std::path::PathBuf::from(common::live::work_folder(&s));
    for n in 1..=3 {
        let text: String = (0..1500)
            .map(|i| format!("line {i} of part {n}: amber basalt cobalt delta ember\n"))
            .collect();
        std::fs::write(folder.join(format!("part{n}.txt")), text).unwrap();
    }
    let job = "Use your Read tool to read part1.txt, then part2.txt, then part3.txt, each whole, from your folder. Then reply with the single word OK.";
    let id = team(&s, &[("a", job)], json!({})).await;
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft["protection"] =
        json!({ "on": true, "read_kb": 1024, "shell_chars": 8000, "context_tokens": 30000 });
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    let has = |pred: &dyn Fn(&model::EventBody) -> bool| {
        crate::common::m2::events(&s.st, &a)
            .iter()
            .any(|e| pred(&e.body))
    };
    common::live::wait_for("the ceiling is reached", || {
        has(&|b| {
            matches!(
                b,
                model::EventBody::ContextCeilingReached {
                    ceiling: 30_000,
                    ..
                }
            )
        })
    })
    .await;
    common::live::wait_for("the note is asked for", || has(&|b| matches!(b, model::EventBody::MessageQueued { text, .. } if text.contains(".sculptured/notes/a.md")))).await;
    common::live::wait_for("it continues fresh", || has(&|b| matches!(b, model::EventBody::MessageQueued { text, .. } if text.contains("continuing in a fresh conversation")))).await;
    let all = crate::common::m2::events(&s.st, &a);
    let fresh = all
        .iter()
        .filter(|e| matches!(e.body, model::EventBody::TenureStarted { .. }))
        .count();
    assert!(fresh >= 2, "a second, fresh conversation started");
    // The note's turn ran to its end before the restart: a turn ended after the ceiling was recorded
    // and before the second conversation began, and no turn was cut short.
    let seq_of = |p: &dyn Fn(&model::EventBody) -> bool| {
        all.iter()
            .filter(|e| p(&e.body))
            .map(|e| e.seq)
            .collect::<Vec<_>>()
    };
    let reached = seq_of(&|b| matches!(b, model::EventBody::ContextCeilingReached { .. }))[0];
    let second = seq_of(&|b| matches!(b, model::EventBody::TenureStarted { .. }))[1];
    assert!(
        seq_of(&|b| matches!(b, model::EventBody::TurnEnded { .. }))
            .iter()
            .any(|t| *t > reached && *t < second),
        "the note turn ended before the restart"
    );
    assert!(!all.iter().any(|e| matches!(&e.body, model::EventBody::TurnEnded { status, .. } if *status != model::TurnStatus::Completed)), "no turn was cut short");
}

fn ceilings(q: &Quiet, seat: &str) -> Vec<(u64, u64)> {
    q.st.store
        .lock()
        .unwrap()
        .events_for_seat(seat, 0, 500)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.body {
            model::EventBody::ContextCeilingReached {
                used_tokens,
                ceiling,
            } => Some((used_tokens, ceiling)),
            _ => None,
        })
        .collect()
}

/// An idle agent whose conversation grew past the team's ceiling (60k by default) and past twice its
/// start is asked for its note.
#[tokio::test]
async fn a_conversation_past_the_ceiling_and_twice_its_start_reaches_the_ceiling() {
    let mut q = quiet();
    let id = q.team(&["a"], json!({}), |_| {}).await;
    let a = budget_store::seat(&id, "a");
    q.start(&id);
    q.activity(&a, model::SeatActivity::Idle);
    q.context(&a, 20_000);
    q.context(&a, 50_000);
    controld::collaboration::context_ceiling::check(&q.st).await;
    assert_eq!(ceilings(&q, &a), [], "below the ceiling");
    q.context(&a, 80_000);
    controld::collaboration::context_ceiling::check(&q.st).await;
    assert_eq!(ceilings(&q, &a), [(80_000, 60_000)]);
}

/// A conversation that starts large and does not grow is not restarted: a fresh one would start just as
/// large, and restart again, in a loop.
#[tokio::test]
async fn a_conversation_that_starts_above_the_ceiling_but_does_not_grow_is_left_alone() {
    let mut q = quiet();
    let id = q.team(&["a"], json!({}), |_| {}).await;
    let a = budget_store::seat(&id, "a");
    q.start(&id);
    q.activity(&a, model::SeatActivity::Idle);
    q.context(&a, 70_000);
    q.context(&a, 70_500);
    controld::collaboration::context_ceiling::check(&q.st).await;
    assert_eq!(
        ceilings(&q, &a),
        [],
        "no ceiling for a conversation that did not grow"
    );
}

/// A computer's own cap (an idea from Gas Town's scheduler): at most N agents work at once on it, across
/// every team, so several teams together stay within one plan.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_computers_cap_holds_work_across_teams_until_a_slot_frees() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        "/api/hosts/local/limit",
        json!({ "max_working": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, hosts) = crate::common::m2::call(&s.router, "GET", "/api/hosts", json!(null)).await;
    assert_eq!(hosts[0]["max_working"], 1, "{hosts}");
    let one = team(&s, &[("a", OK_JOB)], json!({})).await;
    let two = team(&s, &[("b", OK_JOB)], json!({})).await;
    crate::common::m2::start_team(&s, &one).await;
    crate::common::m2::start_team(&s, &two).await;
    let seats = [
        crate::common::m2::seat(&one, "a"),
        crate::common::m2::seat(&two, "b"),
    ];
    common::live::wait_for("both teams' agents ran", || {
        seats.iter().all(|x| !turns(&s, x).is_empty())
    })
    .await;
    let mut all: Vec<(i64, i64)> = seats.iter().flat_map(|x| turns(&s, x)).collect();
    all.sort();
    assert!(
        all.windows(2).all(|w| w[0].1 < w[1].0),
        "one agent of the two teams worked at a time: {all:?}"
    );
    let held: Vec<String> = seats
        .iter()
        .flat_map(|x| crate::common::m2::events(&s.st, x))
        .filter_map(|e| match e.body {
            model::EventBody::MessageHeld { reason, .. } => Some(reason),
            _ => None,
        })
        .collect();
    assert!(
        held.iter().any(|r| r.contains("on this computer")),
        "{held:?}"
    );

    let (status, _) = crate::common::m2::call(
        &s.router,
        "PUT",
        "/api/hosts/local/limit",
        json!({ "max_working": 0 }),
    )
    .await;
    assert!(status.is_client_error(), "a cap of 0 would hold everything");
    let (status, _) = crate::common::m2::call(
        &s.router,
        "PUT",
        "/api/hosts/local/limit",
        json!({ "max_working": null }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "no cap");
}

#[tokio::test]
async fn raising_a_stopped_runs_token_limit_continues_it_for_this_run_only() {
    let mut q = quiet();
    let id = q
        .team(&["a"], json!({ "tokens": { "hard": 2000 } }), |_| {})
        .await;
    let a = budget_store::seat(&id, "a");
    q.start(&id);
    q.turn_started(&a);
    spend_until(
        &mut q,
        10,
        |q, _| q.usage(&a, 10, 1000),
        |q| matches!(q.state(&id), BudgetState::Stopped { .. }),
    )
    .await;
    let meters = controld::budget::status(&q.st, &id)
        .unwrap()
        .unwrap()
        .meters;
    assert_eq!(
        meters[0].limit, "tokens",
        "each meter says which limit it is, for Raise"
    );
    let raise = |hard: u64| json!({ "request_id": rid(), "limit": "tokens", "hard": hard });
    let url = format!("/api/teams/{id}/budget/raise");
    // Lowering is not a raise.
    let (status, body) = budget_store::call(&q.router, "POST", &url, raise(1000)).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("not_a_raise")),
        "{body}"
    );
    let (status, body) = budget_store::call(&q.router, "POST", &url, raise(1_000_000)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let r = q.record(&id);
    assert_eq!(r.state, BudgetState::Normal, "the run continues");
    assert_eq!(r.budget.tokens.unwrap().hard, 1_000_000);
    let (_, team) =
        budget_store::call(&q.router, "GET", &format!("/api/teams/{id}"), Value::Null).await;
    assert_eq!(
        team["draft"]["budget"]["tokens"]["hard"], 2000,
        "the saved budget is unchanged"
    );
    assert!(
        q.changes(&id).iter().any(|c| c == "raised"),
        "the run's log says it was raised"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_raising_a_stopped_runs_token_limit_sets_its_agent_working_again() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    // A real model call reads far more than 2000 tokens: the run is stopped.
    let id = team(&s, &[("a", OK_JOB)], json!({ "tokens": { "hard": 2000 } })).await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("the token budget stops the run", || {
        matches!(state(&s, &id), BudgetState::Stopped { .. })
    })
    .await;
    common::live::wait_for("its agent stopped", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let turns_before = turns(&s, &a).len();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/budget/raise"),
        json!({ "request_id": rid(), "limit": "tokens", "hard": 100_000_000 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("the stopped agent works again", || {
        turns(&s, &a).len() > turns_before
    })
    .await;
}

#[tokio::test]
async fn a_limit_the_run_does_not_have_cannot_be_raised() {
    let mut q = quiet();
    let id = q
        .team(&["a"], json!({ "tokens": { "hard": 100000 } }), |_| {})
        .await;
    q.start(&id);
    let (status, body) = budget_store::call(
        &q.router,
        "POST",
        &format!("/api/teams/{id}/budget/raise"),
        json!({ "request_id": rid(), "limit": "minutes", "hard": 60 }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("no_such_limit")),
        "{body}"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn live_raising_an_agents_own_token_limit_lets_it_carry_on_for_this_run_only() {
    let Some(_) = common::live::require() else {
        return;
    };
    let (s, _runner) = live_served().await;
    let id = team(&s, &[("a", OK_JOB)], json!({})).await;
    // A real model call reads far more than 2000 tokens: the agent is stopped by its limit.
    with_advanced(&s, &id, "a", json!({ "token_limit": { "tokens": 2000 } })).await;
    crate::common::m2::start_team(&s, &id).await;
    let a = crate::common::m2::seat(&id, "a");
    common::live::wait_for("the agent is stopped by its limit", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let url = format!("/api/teams/{id}/agents/a/limits/raise");
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &url,
        json!({ "request_id": rid(), "limit": "tokens", "hard": 1000 }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("not_a_raise")),
        "{body}"
    );
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &url,
        json!({ "request_id": rid(), "limit": "turns", "hard": 9 }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("no_such_limit")),
        "{body}"
    );
    let turns_before = turns(&s, &a).len();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &url,
        json!({ "request_id": rid(), "limit": "tokens", "hard": 100_000_000 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("the agent works again", || {
        turns(&s, &a).len() > turns_before
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_ne!(
        s.st.store.lock().unwrap().control(&a).unwrap(),
        Some(controld::store::ControlMode::Hard),
        "the raised limit is not stopped again at the old value"
    );
    let (_, budget) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/budget")).await;
    let row = budget["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["seat"] == a)
        .unwrap()
        .clone();
    assert_eq!(
        (
            row["member"].as_str(),
            row["token_limit"]["tokens"].as_u64()
        ),
        (Some("a"), Some(100_000_000)),
        "the board shows the raised limit: {row}"
    );
    let (_, team) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
    let saved = team["draft"]["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == "a")
        .unwrap()["advanced"]["token_limit"]["tokens"]
        .clone();
    assert_eq!(saved, 2000, "the saved limit is unchanged");
}
