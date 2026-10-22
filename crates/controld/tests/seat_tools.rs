//! Seat tools end to end: Practice agents call them through hostd's real socket and the host link.
mod common;

use common::m2::{Served, rid};

use host_proto::SeatToolsParams;

use model::{
    Actor, EventBody, PermissionPosture, Provenance, QuestionSource, ReportVerdict, SeatActivity,
};
use serde_json::json;

async fn raised(s: &Served, agent: &str) -> String {
    let e = crate::common::m2::wait_event(&s.st, agent, "a question", |b| {
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

#[tokio::test(flavor = "multi_thread")]
async fn a_request_input_question_is_answered_through_the_socket() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    crate::common::m2::say(&s, &agent, "request:Which colour?|Blue|Green").await;
    let q = raised(&s, &agent).await;
    common::wait_for("waiting for the operator", || {
        let v = s.st.store.lock().unwrap().seat(&agent).unwrap().unwrap();
        (v.open_questions, v.activity) == (1, SeatActivity::WaitingInput)
    })
    .await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["question"]["prompt"]["text"], "Which colour?");
    let (status, v) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/questions/{q}/answer"),
        json!({ "request_id": rid(), "answer": { "kind": "text", "text": "Green" } }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK, "{v}");
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: Green"),
    )
    .await;
    assert!(s.st.seat_waits.lock().unwrap().is_empty());
    // Ruling Q1: the answer is recorded before it is delivered, so the log reads answered → reply.
    let answered = seq_of(
        &s,
        &agent,
        |b| matches!(b, EventBody::QuestionAnswered { question_id, .. } if *question_id == q),
    );
    let replied = seq_of(
        &s,
        &agent,
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub got answer: Green"),
    );
    assert!(
        answered < replied,
        "answered at {answered}, replied at {replied}"
    );
    common::wait_for("working again", || {
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
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_request_records_the_agents_structured_decision() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(300))
        .unwrap();
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    crate::common::m2::say(&s, &agent, "request:Pick one|Red|Blue").await;
    let q = raised(&s, &agent).await;
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "I decided: Red"),
    )
    .await;
    let (_, qs) = crate::common::m2::get(&s.router, &format!("/api/seats/{agent}/questions")).await;
    assert_eq!(qs[0]["id"], q.as_str());
    assert_eq!(qs[0]["state"], "expired");
    assert_eq!(qs[0]["decision"]["source"], "record-decision");
    assert_eq!(qs[0]["decision"]["text"], "Red");
    assert_eq!(
        qs[0]["decision"]["rationale"],
        "a Practice agent picks the first option"
    );
    assert_eq!(qs[0]["decision"]["provenance"], "agent-reported");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_report_becomes_findings_on_the_running_turn() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    crate::common::m2::say(
        &s,
        &agent,
        "report:Two problems|missing test|typo in README",
    )
    .await;
    let f = crate::common::m2::wait_event(&s.st, &agent, "findings", |b| {
        matches!(b, EventBody::FindingsReported { .. })
    })
    .await;
    let EventBody::FindingsReported {
        turn_id,
        summary,
        findings,
        verdict,
    } = f.body
    else {
        unreachable!()
    };
    assert_eq!(summary, "Two problems");
    assert_eq!(
        findings.iter().map(|f| f.text.as_str()).collect::<Vec<_>>(),
        ["missing test", "typo in README"]
    );
    assert_eq!(verdict, ReportVerdict::ProblemsFound);
    assert_eq!(f.provenance, Provenance::AgentReported);
    let turn = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .rev()
        .find_map(|e| match e.body {
            EventBody::TurnStarted { turn_id } if e.seq < f.seq => Some(turn_id),
            _ => None,
        });
    assert_eq!(turn_id, turn, "attached to the turn that was running");
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the reply",
        |b| matches!(b, EventBody::AgentText { text, .. } if text == "stub reported: Two problems"),
    )
    .await;
}

/// Important 1, end to end through hostd's socket and the host link: a Practice agent's 1.2 MB report
/// is refused at hostd in plain words and recorded on the agent; the link stays up for the next call.
#[tokio::test(flavor = "multi_thread")]
async fn an_oversized_report_is_refused_and_the_link_keeps_working() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    crate::common::m2::say(&s, &agent, &format!("report:{}", "x".repeat(1_200_000))).await;
    let reply = crate::common::m2::wait_event(&s.st, &agent, "the agent's reply", |b| matches!(b, EventBody::AgentText { text, .. } if text.starts_with("stub could not report"))).await;
    let EventBody::AgentText { text, .. } = reply.body else {
        unreachable!()
    };
    assert_eq!(
        text,
        format!("stub could not report: {}", hostd::seat_tools::TOO_LARGE)
    );
    let (tool, reason) = crate::common::seat_tools::rejected(&s, &agent)
        .pop()
        .expect("recorded on the agent");
    assert_eq!(
        (tool.as_str(), reason.as_str()),
        ("report", hostd::seat_tools::TOO_LARGE)
    );
    assert!(
        s.st.hosts.is_connected("local"),
        "the host link is still up"
    );
    crate::common::m2::say(&s, &agent, "report:Small|one finding").await;
    crate::common::m2::wait_event(
        &s.st,
        &agent,
        "the next report",
        |b| matches!(b, EventBody::FindingsReported { summary, .. } if summary == "Small"),
    )
    .await;
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
      - { id: d, agent_ref: "local:d", profile: default, runtime: stub, cwd: "." }
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
    assert_eq!(
        controld::seat_tools::launch_params(&st, &rig.seats[3], PermissionPosture::Restricted),
        Some(SeatToolsParams {
            approve: false,
            workflow: true
        }),
        "a Practice agent asks through perm:"
    );
}

/// Urgency (agent-reported, an idea from Gas Town's escalation severity): an urgent question comes first
/// in Needs you, even when asked later; an unknown urgency is refused.
#[tokio::test(flavor = "multi_thread")]
async fn an_urgent_question_comes_first_in_needs_you() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let (one, two) = (
        crate::common::m2::practice_team(&s, "solo").await,
        crate::common::m2::practice_team(&s, "solo").await,
    );
    let (a, b) = (
        crate::common::m2::seat(&one, "claude"),
        crate::common::m2::seat(&two, "claude"),
    );
    let (_ca, _ra, _qa) = crate::common::seat_tools::request_call(
        &s,
        &a,
        json!({ "question": "Which font?", "urgency": "low" }),
    )
    .await;
    let (_cb, _rb, qb) = crate::common::seat_tools::request_call(
        &s,
        &b,
        json!({ "question": "The build server is down: wait or skip?", "urgency": "high" }),
    )
    .await;
    let (_, inbox) = crate::common::m2::get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["id"], format!("q:{qb}"), "{inbox}");
    assert_eq!(inbox[0]["question"]["prompt"]["urgency"], "high");
    let (ok, _, error) = crate::common::seat_tools::direct(
        &s,
        &a,
        "request_input",
        json!({ "question": "x", "urgency": "now!" }),
    )
    .await;
    assert!(!ok && error.unwrap_or_default().contains("urgency"));
}
