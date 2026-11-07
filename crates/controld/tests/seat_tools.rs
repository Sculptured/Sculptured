//! Seat tools end to end: real agents call them through the real shim, hostd's socket and the host
//! link (`live_…`); contract tests call Sculptured's own handlers on a recorded agent.
#[path = "common/asking.rs"]
mod asking;
mod common;

use common::m2::{Served, rid};

use host_proto::SeatToolsParams;

use model::{Actor, EventBody, PermissionPosture, Provenance, QuestionSource, SeatActivity};
use serde_json::json;

async fn raised(s: &Served, agent: &str) -> String {
    let e = crate::common::live::wait_event(&s.st, agent, "a question", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    })
    .await;
    let EventBody::QuestionRaised {
        question_id,
        source,
        ..
    } = e.body
    else {
        unreachable!()
    };
    assert_eq!(source, QuestionSource::RequestInput);
    assert_eq!(
        (e.actor, e.provenance),
        (Actor::Seat(agent.into()), Provenance::AgentReported)
    );
    question_id
}

fn seq_of(s: &Served, seat: &str, pred: impl Fn(&EventBody) -> bool) -> i64 {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .find(|e| pred(&e.body))
        .map(|e| e.seq)
        .expect("the event")
}

/// Live, end to end through the real shim, hostd's socket and the host link: a real agent asks with
/// `request_input`, the operator answers, and the agent's call gets the answer after it is recorded.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_request_input_question_is_answered_through_the_socket() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    let agent = asking::live_agent(
        &s,
        "Call the mcp__rig__request_input tool exactly once with question \"Which colour?\" and options [\"Blue\", \"Green\"]. Wait for its result, then reply with the answer in one word.",
        false,
    )
    .await;
    let q = raised(&s, &agent).await;
    let call = asking::live_call_of(&s, &agent, "request_input").await;
    crate::common::live::wait_for("waiting for the operator", || {
        let v = s.st.store.lock().unwrap().seat(&agent).unwrap().unwrap();
        (v.open_questions, v.activity) == (1, SeatActivity::WaitingInput)
    })
    .await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["id"], format!("q:{q}"), "{inbox}");
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        json!({ "request_id": rid(), "answer": { "kind": "text", "text": "Green" } }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{v}");
    let (result_seq, is_error) = asking::live_result_of(&s, &agent, &call).await;
    assert!(!is_error, "the call got its answer");
    assert!(s.st.seat_waits.lock().unwrap().is_empty());
    // Ruling Q1: the answer is recorded before it is delivered, so the log reads answered → result.
    let answered = seq_of(
        &s,
        &agent,
        |b| matches!(b, EventBody::QuestionAnswered { question_id, .. } if *question_id == q),
    );
    assert!(
        answered < result_seq,
        "answered at {answered}, the call's result at {result_seq}"
    );
    crate::common::live::wait_for("working again", || {
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .activity
            != SeatActivity::WaitingInput
    })
    .await;
    stop(&s, &agent).await;
}

/// Live: a real agent's question nobody answers expires, the agent is told to decide, and what it
/// decided is recorded as its own (a `record_decision` call, or its next words).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_an_expired_request_records_the_agents_decision() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(300))
        .unwrap();
    let agent = asking::live_agent(
        &s,
        "Call the mcp__rig__request_input tool exactly once with question \"Pick one\" and options [\"Red\", \"Blue\"]. If it says nobody answered, follow its instructions and choose Red.",
        false,
    )
    .await;
    let q = raised(&s, &agent).await;
    crate::common::live::wait_event(
        &s.st,
        &agent,
        "the decision",
        |b| matches!(b, EventBody::QuestionDecided { question_id, .. } if *question_id == q),
    )
    .await;
    let v = crate::common::seat_tools::question_view(&s, &agent, &q).await;
    assert_eq!(v["state"], "expired");
    assert!(
        ["record-decision", "next-message"].contains(&v["decision"]["source"].as_str().unwrap()),
        "{v}"
    );
    assert_eq!(v["decision"]["provenance"], "agent-reported", "{v}");
    stop(&s, &agent).await;
}

/// Live: a real agent's `report` call becomes findings on the turn that was running.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_report_becomes_findings_on_the_running_turn() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = asking::live_host().await;
    let agent = asking::live_agent(
        &s,
        "Call the mcp__rig__report tool exactly once with summary \"Two problems\", findings [\"missing test\", \"typo in README\"] and verdict \"problems-found\". Then reply with the single word OK.",
        false,
    )
    .await;
    let f = crate::common::live::wait_event(&s.st, &agent, "findings", |b| {
        matches!(b, EventBody::FindingsReported { .. })
    })
    .await;
    let EventBody::FindingsReported {
        turn_id, findings, ..
    } = f.body
    else {
        unreachable!()
    };
    assert!(!findings.is_empty());
    assert_eq!(
        (f.actor, f.provenance),
        (Actor::Seat(agent.clone()), Provenance::AgentReported)
    );
    let turn = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .rev()
        .find_map(|e| match e.body {
            EventBody::TurnStarted { turn_id } if e.seq < f.seq => Some(turn_id),
            _ => None,
        });
    assert!(turn.is_some());
    assert_eq!(turn_id, turn, "attached to the turn that was running");
    stop(&s, &agent).await;
}

async fn stop(s: &Served, agent: &str) {
    let team = s.st.store.lock().unwrap().seat(agent).unwrap().unwrap().rig;
    crate::common::m2::stop_team(s, &team).await;
}

#[test]
fn launch_params_offer_approve_only_to_a_restricted_claude_agent() {
    let rig = rigspec::compile(
        r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: claude-code, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: terminal, cwd: "." }
      - { id: c, agent_ref: "local:c", profile: default, runtime: codex, cwd: "." }
"#,
    )
    .unwrap();
    let st = controld::AppState::new(
        controld::store::Store::open_in_memory().unwrap(),
        "t".into(),
        vec![],
    );
    assert_eq!(
        controld::seat_tools::launch_params(&st, &rig.seats[0], PermissionPosture::Unrestricted),
        Some(SeatToolsParams {
            approve: false,
            workflow: true
        })
    );
    assert_eq!(
        controld::seat_tools::launch_params(&st, &rig.seats[0], PermissionPosture::Restricted),
        Some(SeatToolsParams {
            approve: true,
            workflow: true
        })
    );
    assert_eq!(
        controld::seat_tools::launch_params(&st, &rig.seats[1], PermissionPosture::Restricted),
        None,
        "a terminal has no MCP client"
    );
    assert_eq!(
        controld::seat_tools::launch_params(&st, &rig.seats[2], PermissionPosture::Restricted),
        Some(SeatToolsParams {
            approve: false,
            workflow: true
        }),
        "a restricted Codex agent asks through its own approvals, not the approve tool"
    );
}

/// Urgency (agent-reported, an idea from Gas Town's escalation severity): an urgent question comes first
/// in Needs you, even when asked later; an unknown urgency is refused.
#[tokio::test(flavor = "multi_thread")]
async fn an_urgent_question_comes_first_in_needs_you() {
    let s = asking::recorded_agents().await;
    let (a, b) = (asking::A, asking::B);
    let (_ca, _ra, _qa) = crate::common::seat_tools::request_call(
        &s,
        a,
        json!({ "question": "Which font?", "urgency": "low" }),
    )
    .await;
    let (_cb, _rb, qb) = crate::common::seat_tools::request_call(
        &s,
        b,
        json!({ "question": "The build server is down: wait or skip?", "urgency": "high" }),
    )
    .await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["id"], format!("q:{qb}"), "{inbox}");
    assert_eq!(inbox[0]["question"]["prompt"]["urgency"], "high");
    let (ok, _, error) = crate::common::seat_tools::direct(
        &s,
        a,
        "request_input",
        json!({ "question": "x", "urgency": "now!" }),
    )
    .await;
    assert!(!ok && error.unwrap_or_default().contains("urgency"));
}
