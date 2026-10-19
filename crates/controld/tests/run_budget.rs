//! Run budgets end to end, with fake-claude agents (no model is called): each `spend` call reports its
//! tokens, and with a plan file adds to a shared 5-hour percent, like one account's agents.
mod common;
use axum::http::StatusCode;
use common::m2::*;
use common::{fake_bin, lifecycle, wait_for};
use controld::budget::BudgetState;
use drivers::claude::ClaudeStreamDriver;
use model::SeatLifecycle;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

async fn claude_runner(s: &Served) -> hostd::supervisor::Supervisor {
    connect(s, vec![Arc::new(ClaudeStreamDriver { program: fake_bin("fake-claude") })], None).await
}

/// A team of independent Claude agents in `dir`, each with its own job, and `budget`.
async fn team(s: &Served, dir: &Path, jobs: &[(&str, String)], budget: Value) -> String {
    let (status, rec) = call(&s.router, "POST", "/api/teams", json!({ "request_id": rid(), "template": "blank", "name": "Spend" })).await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = jobs.iter().enumerate().map(|(i, (id, job))| json!({ "id": id, "group_id": null, "name": id, "kind": "claude",
        "job": job, "folder": dir.display().to_string(), "x": 40 + 260 * i as i32, "y": 40 })).collect();
    draft["connections"] = json!([]);
    draft["budget"] = budget;
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    id
}

fn state(s: &Served, id: &str) -> BudgetState {
    controld::budget::status(&s.st, id).unwrap().expect("a budget").state
}

async fn wait_long(what: &str, pred: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !pred() {
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn plan_percent(file: &Path) -> f64 {
    std::fs::read_to_string(file).map(|s| s.trim().parse().unwrap_or(0.0)).unwrap_or(0.0)
}

#[tokio::test]
async fn a_run_wraps_up_at_soft_and_stops_at_hard_within_the_bound() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let plan = dir.path().join("plan");
    // Each call: 1000 tokens and 0.5% of the window, 120 ms apart (real calls take seconds; checks run
    // every 0.1 s at most); two agents together.
    let job = format!("spend:400:1000:120:{}:0.5", plan.display());
    let id = team(&s, dir.path(), &[("a", job.clone()), ("b", job)], json!({ "plan": [{ "provider": "claude", "window": "five_hour", "soft": 10.0, "hard": 20.0 }] })).await;
    start_team(&s, &id).await;
    wait_long("the run wraps up", || !matches!(state(&s, &id), BudgetState::Normal)).await;
    wait_long("the run is stopped", || matches!(state(&s, &id), BudgetState::Stopped { .. })).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    wait_for("both agents stopped", || [&a, &b].iter().all(|x| lifecycle(&s.st, x) == Some(SeatLifecycle::Stopped))).await;
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert!(r.wrap_up_ms.is_some(), "it wrapped up before it stopped");
    let BudgetState::Stopped { reason, .. } = &r.state else { unreachable!() };
    assert!(reason.starts_with("Claude 5-hour:") && reason.contains("hard limit of 20.0%"), "{reason}");
    // The account's whole use since the first reading (0.5%), against 20%: the stop comes one call
    // per working agent early (the reserve), so only in-flight calls can pass it.
    let account = plan_percent(&plan) - 0.5;
    assert!(account <= 20.0 + 2.0 * 0.5, "the run added {account:.1}% against a 20% hard limit");
    assert!(account >= 15.0, "it ran most of its budget first: {account:.1}%");
    let rate: String = s.st.store.lock().unwrap().connection().query_row("SELECT json FROM settings WHERE k = 'budget-rate:claude:five_hour'", [], |r| r.get(0)).unwrap();
    assert!((rate.parse::<f64>().unwrap() - 0.5 / 1010.0).abs() < 1e-6, "the learned rate is kept per plan window: {rate}");
}

#[tokio::test]
async fn with_no_plan_reading_a_learned_rate_still_stops_the_run() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    // An earlier run learned 0.001% per token; this run's CLIs report no plan readings at all.
    s.st.store.lock().unwrap().connection().execute("INSERT INTO settings (k, json) VALUES ('budget-rate:claude:five_hour', '0.001')", []).unwrap();
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:400:1000:120".into())], json!({ "plan": [{ "provider": "claude", "window": "five_hour", "hard": 10.0 }] })).await;
    start_team(&s, &id).await;
    wait_long("the run is stopped on the estimate", || matches!(state(&s, &id), BudgetState::Stopped { .. })).await;
    let st = controld::budget::status(&s.st, &id).unwrap().unwrap();
    assert!(st.meters[0].estimated, "{:?}", st.meters);
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert!(r.peak_tokens <= 10_000 + 2 * 1010, "10% at 0.001%/token is 10 000 tokens: {}", r.peak_tokens);
}

#[tokio::test]
async fn token_and_time_budgets_enforce_without_plan_readings() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:400:1000:120".into())], json!({ "tokens": { "hard": 30000 } })).await;
    start_team(&s, &id).await;
    wait_long("the token budget stops the run", || matches!(state(&s, &id), BudgetState::Stopped { .. })).await;
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert!(r.wrap_up_ms.is_some(), "85% of 30 000 tokens wrapped up first");
    assert!(r.peak_tokens <= 30_000 + 1010, "{}", r.peak_tokens);
    let changes = s.st.store.lock().unwrap().latest_of_types(&model::Subject::Rig(id.clone()), &["budget-state-changed"], 10).unwrap();
    let states: Vec<String> = changes.iter().rev().map(|e| match &e.body {
        model::EventBody::BudgetStateChanged { state, reason } => { assert!(!reason.is_empty()); state.clone() }
        other => panic!("{other:?}"),
    }).collect();
    assert_eq!(states, ["wrap-up", "stopped"], "the run's log records each change, for reports of the run");

    let id = team(&s, dir.path(), &[("a", "spend:400:10:50".into())], json!({ "minutes": { "soft": 0, "hard": 1 } })).await;
    start_team(&s, &id).await;
    wait_long("a soft time limit of 0 wraps up at once", || matches!(state(&s, &id), BudgetState::WrapUp { .. })).await;
}

/// Turn intervals per agent, from its TurnStarted/TurnEnded events: (start seq, end seq).
fn turns(s: &Served, seat: &str) -> Vec<(i64, i64)> {
    let ev = events(&s.st, seat);
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
async fn a_delivery_waits_for_a_slot_then_runs() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let job = || "spend:10:100:30".to_string();
    let id = team(&s, dir.path(), &[("a", job()), ("b", job()), ("c", job())], json!({ "max_working": 1 })).await;
    start_team(&s, &id).await;
    let seats: Vec<String> = ["a", "b", "c"].iter().map(|a| seat(&id, a)).collect();
    wait_long("every agent ran its job", || seats.iter().all(|x| !turns(&s, x).is_empty())).await;
    let mut all: Vec<(i64, i64)> = seats.iter().flat_map(|x| turns(&s, x)).collect();
    all.sort();
    assert!(all.windows(2).all(|w| w[0].1 < w[1].0), "one agent worked at a time: {all:?}");
}

#[tokio::test]
async fn every_path_to_new_work_is_refused_past_the_soft_value() {
    use controld::budget::gate::{decide, Gate};
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "hello".into()), ("b", "hello".into())], json!({ "tokens": { "hard": 1000000 } })).await;
    start_team(&s, &id).await;
    let a = seat(&id, "a");
    wait_for("a's job ran", || !turns(&s, &a).is_empty()).await;
    {
        let store = s.st.store.lock().unwrap();
        let mut r = store.run_budget(&id).unwrap().unwrap();
        r.state = BudgetState::WrapUp { reason: "Tokens: test".into(), at_ms: 1 };
        store.put_run_budget(&id, &r).unwrap();
    }
    // A person's message, through the API.
    let (status, body) = call(&s.router, "POST", &format!("/api/seats/{a}/send"), json!({ "request_id": rid(), "text": "more", "intent": "inform" })).await;
    assert!(status.is_client_error() || status.is_server_error(), "{status} {body}");
    assert!(body.to_string().contains("Run budget: Tokens: test"), "{body}");
    // Every command that starts work, whoever sends it: the gate sits in front of every runner command.
    let store = s.st.store.lock().unwrap();
    let send = host_proto::HostCommand::Send { seat: a.clone(), message_id: "m".into(), intent: model::MessageIntent::Inform, text: "x".into() };
    assert!(matches!(decide(&store, &send).unwrap(), Gate::Refuse(r) if r.contains("Run budget")));
    let interrupt = host_proto::HostCommand::Interrupt { seat: a.clone() };
    assert_eq!(decide(&store, &interrupt).unwrap(), Gate::Pass, "interrupts and stops are never held");
    assert!(controld::budget::gate::refusal(&store, &id).unwrap().is_some(), "spawn_agent refuses before creating a child");
    store.record_control(&a, controld::store::ControlMode::Soft, 0).unwrap();
    assert_eq!(decide(&store, &send).unwrap(), Gate::Pass, "the wrap-up instruction reaches an agent that is wrapping up");
}

/// `team`, then sets `advanced` on agent `agent` (e.g. its limits).
async fn with_advanced(s: &Served, id: &str, agent: &str, advanced: Value) {
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    let a = draft["agents"].as_array_mut().unwrap().iter_mut().find(|a| a["id"] == agent).unwrap();
    for (k, v) in advanced.as_object().unwrap() {
        a["advanced"][k] = v.clone();
    }
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn tokens_of(s: &Served, seat: &str) -> u64 {
    events(&s.st, seat).iter().map(|e| match e.body {
        model::EventBody::UsageReported { input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, .. } => input_tokens + output_tokens + cache_read_tokens + cache_write_tokens,
        _ => 0,
    }).sum()
}

#[tokio::test]
async fn an_agent_wraps_up_at_its_soft_tokens_and_stops_at_hard() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:400:1000:120".into())], json!({})).await;
    with_advanced(&s, &id, "a", json!({ "token_limit": { "tokens": 20000, "soft": 10000 } })).await;
    start_team(&s, &id).await;
    let a = seat(&id, "a");
    wait_long("the agent is stopped", || lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)).await;
    let asked = events(&s.st, &a).iter().any(|e| matches!(&e.body, model::EventBody::MessageQueued { text, .. } if text.contains("Soft stop requested")));
    assert!(asked, "it was asked to wrap up at 10 000 tokens");
    assert!(tokens_of(&s, &a) <= 20_000 + 2 * 1010, "stopped at the hard value: {}", tokens_of(&s, &a));
}

#[tokio::test]
async fn a_turn_limit_wraps_up_then_stops_the_agent() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "hello".into())], json!({})).await;
    with_advanced(&s, &id, "a", json!({ "turn_limit": { "hard": 2 } })).await;
    start_team(&s, &id).await;
    let a = seat(&id, "a");
    // Soft is 85% of 2 = 1 turn: after its job the agent is asked to wrap up (its 2nd turn), then stopped.
    wait_long("the agent is stopped", || lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)).await;
    assert_eq!(turns(&s, &a).len(), 2, "its job, then its wrap-up turn, both finished: {:?}", turns(&s, &a));
}

#[tokio::test]
async fn a_new_run_without_a_budget_is_not_governed_by_the_last_runs_record() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:400:1000:120".into())], json!({ "tokens": { "hard": 3000 }, "max_working": 1 })).await;
    start_team(&s, &id).await;
    wait_long("the first run is stopped", || matches!(state(&s, &id), BudgetState::Stopped { .. })).await;
    let a = seat(&id, "a");
    wait_for("its agent stopped", || lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)).await;
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft.as_object_mut().unwrap().remove("budget");
    draft["agents"][0]["job"] = json!("spend:3:100:20");
    let (status, body) = call(&s.router, "PUT", &format!("/api/teams/{id}"), json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let before = turns(&s, &a).len();
    start_team(&s, &id).await;
    wait_long("the new run's job ran", || turns(&s, &a).len() > before).await;
    assert!(controld::budget::status(&s.st, &id).unwrap().is_none(), "the board shows no budget for this run");
    let refused = events(&s.st, &a).iter().any(|e| matches!(&e.body, model::EventBody::MessageFailed { reason, .. } if reason.contains("Run budget")));
    assert!(!refused, "nothing was refused by the old run's budget");
}

#[tokio::test]
async fn an_output_only_token_limit_is_not_tripped_by_cache_reads() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    // Each call reads 1000 tokens from the cache but writes 10: 205 output tokens in all.
    let id = team(&s, dir.path(), &[("a", "spend:20:1000:20".into())], json!({})).await;
    with_advanced(&s, &id, "a", json!({ "token_limit": { "tokens": 1000, "counts": "output", "include_subagents": true } })).await;
    start_team(&s, &id).await;
    let a = seat(&id, "a");
    wait_long("its job finished", || !turns(&s, &a).is_empty()).await;
    assert_eq!(lifecycle(&s.st, &a), Some(SeatLifecycle::Running), "205 output tokens of 1000 stop nothing");
    assert_eq!(s.st.store.lock().unwrap().control(&a).unwrap(), None);
}

#[tokio::test]
async fn stop_everything_now_stops_every_agent_at_once_and_the_board_sees_the_meters() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let job = || "spend:400:1000:120".to_string();
    let id = team(&s, dir.path(), &[("a", job()), ("b", job())], json!({ "tokens": { "hard": 10000000 }, "max_working": 5 })).await;
    start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    wait_for("both are spending", || tokens_of(&s, &a) > 0 && tokens_of(&s, &b) > 0).await;
    let (status, body) = get(&s.router, &format!("/api/teams/{id}/budget")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["run"]["meters"][0]["what"], "Tokens", "{body}");
    assert_eq!(body["run"]["state"]["state"], "normal");
    assert_eq!(body["waiting"], json!([]));
    let t = std::time::Instant::now();
    let (status, body) = call(&s.router, "POST", &format!("/api/teams/{id}/stop-now"), json!({ "request_id": rid() })).await;
    assert_eq!((status, body["stopped"].as_u64()), (StatusCode::OK, Some(2)), "{body}");
    wait_for("both stopped", || [&a, &b].iter().all(|x| lifecycle(&s.st, x) == Some(SeatLifecycle::Stopped))).await;
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
}

fn tenures(s: &Served, seat: &str) -> Vec<(model::TenureCause, Option<String>)> {
    events(&s.st, seat).into_iter().filter_map(|e| match e.body {
        model::EventBody::TenureStarted { cause, native_session_id, .. } => Some((cause, native_session_id)),
        _ => None,
    }).collect()
}

#[tokio::test]
async fn start_can_resume_each_agents_last_conversation_or_start_fresh() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "hello".into())], json!({})).await;
    let a = seat(&id, "a");
    let (_, body) = get(&s.router, &format!("/api/teams/{id}/resumable")).await;
    assert_eq!(body["agents"], json!([]), "nothing to resume before a first run");
    start_team(&s, &id).await;
    wait_long("the job ran", || turns(&s, &a).len() == 1).await;
    stop_team(&s, &id).await;
    wait_for("stopped", || lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)).await;
    let first = tenures(&s, &a)[0].1.clone().expect("a session id");

    let (_, body) = get(&s.router, &format!("/api/teams/{id}/resumable")).await;
    assert_eq!(body["agents"], json!([{ "seat": a, "name": "a" }]));
    let (status, body) = call(&s.router, "POST", &format!("/api/teams/{id}/start"), json!({ "request_id": rid(), "resume": true })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    wait_long("the resumed job ran", || turns(&s, &a).len() == 2).await;
    assert_eq!(tenures(&s, &a)[1], (model::TenureCause::Resume, Some(first.clone())), "the agent continued its own conversation");
    stop_team(&s, &id).await;
    wait_for("stopped again", || lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)).await;

    start_team(&s, &id).await;
    wait_long("the fresh job ran", || turns(&s, &a).len() == 3).await;
    let t = &tenures(&s, &a)[2];
    assert!(t.0 != model::TenureCause::Resume && t.1.as_deref() != Some(first.as_str()), "a plain Start begins a new conversation: {t:?}");
}

/// Protections: an agent past its team's context ceiling writes its notes between turns, then
/// continues in a fresh conversation.
#[tokio::test]
async fn an_agent_past_the_context_ceiling_continues_fresh_from_its_notes() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    // The context grows from 20k to 80k over the turn: past the default 60k ceiling, and past twice the
    // conversation's 20k start.
    let id = team(&s, dir.path(), &[("a", "grow:3:20000:20000".into())], json!({})).await;
    start_team(&s, &id).await;
    let a = seat(&id, "a");
    let has = |pred: &dyn Fn(&model::EventBody) -> bool| events(&s.st, &a).iter().any(|e| pred(&e.body));
    wait_long("the ceiling is reached", || has(&|b| matches!(b, model::EventBody::ContextCeilingReached { ceiling: 60_000, .. }))).await;
    wait_long("the note is asked for", || has(&|b| matches!(b, model::EventBody::MessageQueued { text, .. } if text.contains(".sculptured/notes/a.md")))).await;
    wait_long("it continues fresh", || has(&|b| matches!(b, model::EventBody::MessageQueued { text, .. } if text.contains("continuing in a fresh conversation")))).await;
    let all = events(&s.st, &a);
    let fresh = all.iter().filter(|e| matches!(e.body, model::EventBody::TenureStarted { .. })).count();
    assert!(fresh >= 2, "a second, fresh conversation started");
    // The note's turn ran to its end before the restart: a turn ended after the ceiling was recorded
    // and before the second conversation began, and no turn was cut short.
    let seq_of = |p: &dyn Fn(&model::EventBody) -> bool| all.iter().filter(|e| p(&e.body)).map(|e| e.seq).collect::<Vec<_>>();
    let reached = seq_of(&|b| matches!(b, model::EventBody::ContextCeilingReached { .. }))[0];
    let second = seq_of(&|b| matches!(b, model::EventBody::TenureStarted { .. }))[1];
    assert!(seq_of(&|b| matches!(b, model::EventBody::TurnEnded { .. })).iter().any(|t| *t > reached && *t < second), "the note turn ended before the restart");
    assert!(!all.iter().any(|e| matches!(&e.body, model::EventBody::TurnEnded { status, .. } if *status != model::TurnStatus::Completed)), "no turn was cut short");
}

/// A conversation that starts large and does not grow is not restarted: a fresh one would start just as
/// large, and restart again, in a loop.
#[tokio::test]
async fn a_conversation_that_starts_above_the_ceiling_but_does_not_grow_is_left_alone() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:2:70000:20".into())], json!({})).await;
    start_team(&s, &id).await;
    let a = seat(&id, "a");
    wait_long("the turn ends", || turns(&s, &a).len() == 1).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!events(&s.st, &a).iter().any(|e| matches!(e.body, model::EventBody::ContextCeilingReached { .. })), "no ceiling for a conversation that did not grow");
}

/// A computer's own cap (an idea from Gas Town's scheduler): at most N agents work at once on it, across
/// every team, so several teams together stay within one plan.
#[tokio::test]
async fn a_computers_cap_holds_work_across_teams_until_a_slot_frees() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let (status, body) = call(&s.router, "PUT", "/api/hosts/local/limit", json!({ "max_working": 1 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, hosts) = call(&s.router, "GET", "/api/hosts", json!(null)).await;
    assert_eq!(hosts[0]["max_working"], 1, "{hosts}");
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let job = || "spend:10:100:30".to_string();
    let one = team(&s, dir.path(), &[("a", job())], json!({})).await;
    let two = team(&s, dir.path(), &[("b", job())], json!({})).await;
    start_team(&s, &one).await;
    start_team(&s, &two).await;
    let seats = [seat(&one, "a"), seat(&two, "b")];
    wait_long("both teams' agents ran", || seats.iter().all(|x| !turns(&s, x).is_empty())).await;
    let mut all: Vec<(i64, i64)> = seats.iter().flat_map(|x| turns(&s, x)).collect();
    all.sort();
    assert!(all.windows(2).all(|w| w[0].1 < w[1].0), "one agent of the two teams worked at a time: {all:?}");
    let held: Vec<String> = seats.iter().flat_map(|x| events(&s.st, x)).filter_map(|e| match e.body { model::EventBody::MessageHeld { reason, .. } => Some(reason), _ => None }).collect();
    assert!(held.iter().any(|r| r.contains("on this computer")), "{held:?}");

    let (status, _) = call(&s.router, "PUT", "/api/hosts/local/limit", json!({ "max_working": 0 })).await;
    assert!(status.is_client_error(), "a cap of 0 would hold everything");
    let (status, _) = call(&s.router, "PUT", "/api/hosts/local/limit", json!({ "max_working": null })).await;
    assert_eq!(status, StatusCode::OK, "no cap");
}

#[tokio::test]
async fn raising_a_stopped_runs_token_limit_continues_it_for_this_run_only() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:400:1000:3".into())], json!({ "tokens": { "hard": 2000 } })).await;
    start_team(&s, &id).await;
    wait_long("the token budget stops the run", || matches!(state(&s, &id), BudgetState::Stopped { .. })).await;
    let meters = controld::budget::status(&s.st, &id).unwrap().unwrap().meters;
    assert_eq!(meters[0].limit, "tokens", "each meter says which limit it is, for Raise");
    let raise = |hard: u64| json!({ "request_id": rid(), "limit": "tokens", "hard": hard });
    // Lowering is not a raise.
    let (status, body) = call(&s.router, "POST", &format!("/api/teams/{id}/budget/raise"), raise(1000)).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("not_a_raise")), "{body}");
    let turns_before = turns(&s, &seat(&id, "a")).len();
    let (status, body) = call(&s.router, "POST", &format!("/api/teams/{id}/budget/raise"), raise(1_000_000)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(state(&s, &id), BudgetState::Normal, "the run continues");
    let r = s.st.store.lock().unwrap().run_budget(&id).unwrap().unwrap();
    assert_eq!(r.budget.tokens.unwrap().hard, 1_000_000);
    wait_long("the stopped agent works again", || turns(&s, &seat(&id, "a")).len() > turns_before).await;
    let (_, team) = call(&s.router, "GET", &format!("/api/teams/{id}"), Value::Null).await;
    assert_eq!(team["draft"]["budget"]["tokens"]["hard"], 2000, "the saved budget is unchanged");
    let changes = s.st.store.lock().unwrap().latest_of_types(&model::Subject::Rig(id.clone()), &["budget-state-changed"], 10).unwrap();
    assert!(changes.iter().any(|e| matches!(&e.body, model::EventBody::BudgetStateChanged { state, .. } if state == "raised")), "the run's log says it was raised");
}

#[tokio::test]
async fn a_limit_the_run_does_not_have_cannot_be_raised() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:10:10:1".into())], json!({ "tokens": { "hard": 100000 } })).await;
    start_team(&s, &id).await;
    let (status, body) = call(&s.router, "POST", &format!("/api/teams/{id}/budget/raise"), json!({ "request_id": rid(), "limit": "minutes", "hard": 60 })).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("no_such_limit")), "{body}");
}

#[tokio::test]
async fn raising_an_agents_own_token_limit_lets_it_carry_on_for_this_run_only() {
    let s = serve().await;
    let _runner = claude_runner(&s).await;
    let dir = tempfile::tempdir_in(s.home.path()).unwrap();
    let id = team(&s, dir.path(), &[("a", "spend:400:1000:6".into())], json!({})).await;
    with_advanced(&s, &id, "a", json!({ "token_limit": { "tokens": 2000 } })).await;
    start_team(&s, &id).await;
    let a = seat(&id, "a");
    wait_long("the agent is stopped by its limit", || lifecycle(&s.st, &a) == Some(SeatLifecycle::Stopped)).await;
    let url = format!("/api/teams/{id}/agents/a/limits/raise");
    let (status, body) = call(&s.router, "POST", &url, json!({ "request_id": rid(), "limit": "tokens", "hard": 1000 })).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("not_a_raise")), "{body}");
    let (status, body) = call(&s.router, "POST", &url, json!({ "request_id": rid(), "limit": "turns", "hard": 9 })).await;
    assert_eq!((status, body["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("no_such_limit")), "{body}");
    let turns_before = turns(&s, &a).len();
    let (status, body) = call(&s.router, "POST", &url, json!({ "request_id": rid(), "limit": "tokens", "hard": 1_000_000 })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    wait_long("the agent works again", || turns(&s, &a).len() > turns_before).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_ne!(s.st.store.lock().unwrap().control(&a).unwrap(), Some(controld::store::ControlMode::Hard), "the raised limit is not stopped again at the old value");
    let (_, budget) = get(&s.router, &format!("/api/teams/{id}/budget")).await;
    let row = budget["agents"].as_array().unwrap().iter().find(|x| x["seat"] == a).unwrap().clone();
    assert_eq!((row["member"].as_str(), row["token_limit"]["tokens"].as_u64()), (Some("a"), Some(1_000_000)), "the board shows the raised limit: {row}");
    let (_, team) = get(&s.router, &format!("/api/teams/{id}")).await;
    let saved = team["draft"]["agents"].as_array().unwrap().iter().find(|x| x["id"] == "a").unwrap()["advanced"]["token_limit"]["tokens"].clone();
    assert_eq!(saved, 2000, "the saved limit is unchanged");
}
