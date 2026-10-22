//! Seat-tool calls as controld handles them: invalid calls are evidence, questions are capped,
//! `approve` and `record_decision` are only for the agents and questions they belong to.
mod common;

use common::m2::Served;

use model::{
    Actor, DecisionSource, EventBody, Provenance, QuestionPrompt, QuestionSource, Subject,
};
use serde_json::json;

/// Review Focus #5: invalid calls become `report-rejected` evidence and a tool error, never an interruption.
#[tokio::test]
async fn invalid_tool_calls_are_rejected_reports() {
    let s = crate::common::m2::serve().await;
    let (a, b) = ("team.a@t", "team.b@t");
    crate::common::seat_tools::expired_question(&s, a, "q-a", QuestionSource::RequestInput, None);
    let (ok, _, err) = crate::common::seat_tools::direct(
        &s,
        b,
        "record_decision",
        json!({ "question_id": "q-a", "choice": "Blue" }),
    )
    .await;
    assert!(!ok && err.unwrap().contains("not one of your questions"));
    for (tool, args) in [
        (
            "report",
            json!({ "summary": "x", "findings": [1, { "nope": 2 }] }),
        ),
        ("request_input", json!({ "question": "   " })),
        ("launch_missiles", json!({})),
        ("record_decision", json!({ "question_id": "q-a" })),
    ] {
        assert!(
            !crate::common::seat_tools::direct(&s, b, tool, args).await.0,
            "{tool}"
        );
    }
    let tools: Vec<String> = crate::common::seat_tools::rejected(&s, b)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert_eq!(
        tools,
        [
            "record_decision",
            "report",
            "request_input",
            "launch_missiles",
            "record_decision"
        ]
    );
    let raw_sizes: Vec<usize> = crate::common::m2::events(&s.st, b)
        .iter()
        .filter_map(|e| match &e.body {
            EventBody::ReportRejected { raw, .. } => Some(raw.to_string().len()),
            _ => None,
        })
        .collect();
    assert!(raw_sizes.iter().all(|n| *n <= 17 * 1024), "{raw_sizes:?}");
}

/// Under hostd's 1 MiB limit for a call (a bigger one never reaches controld), a huge summary is cut.
#[tokio::test]
async fn a_huge_summary_is_cut_not_refused() {
    let s = crate::common::m2::serve().await;
    let (ok, ..) = crate::common::seat_tools::direct(
        &s,
        "team.b@t",
        "report",
        json!({ "summary": "x".repeat(900_000) }),
    )
    .await;
    assert!(ok);
    let found = crate::common::m2::events(&s.st, "team.b@t")
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::FindingsReported { summary, .. } => Some(summary),
            _ => None,
        });
    assert!(found.unwrap().len() <= 1100);
}

/// A huge or malformed question is capped (Review Focus #5) and stored as raised, not refused.
#[tokio::test]
async fn a_huge_question_is_capped_not_refused() {
    let s = crate::common::m2::serve().await;
    let options: Vec<String> = (0..500)
        .map(|i| format!("option {i} {}", "x".repeat(300)))
        .collect();
    let (call, _link, q) = crate::common::seat_tools::request_call(
        &s,
        "team.a@t",
        json!({ "question": "q".repeat(1_000_000), "options": options }),
    )
    .await;
    let prompt: QuestionPrompt = serde_json::from_value(
        crate::common::seat_tools::question_view(&s, "team.a@t", &q).await["prompt"].clone(),
    )
    .unwrap();
    assert!(prompt.text.len() <= 4 * 1024 + 8, "{}", prompt.text.len());
    assert_eq!(prompt.options.len(), 20);
    assert!(
        prompt
            .options
            .iter()
            .all(|o| o.label.chars().count() <= 201)
    );
    controld::questions::expire(&s.st, &q).await;
    crate::common::seat_tools::ended(call).await;
}

/// Important 3 (defence in depth): controld refuses `approve` for any agent that would not be launched
/// with it, whatever reached it: an unknown seat, or an unrestricted Practice agent.
#[tokio::test]
async fn approve_is_refused_for_an_agent_not_launched_with_it() {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s).await;
    let agent = crate::common::m2::seat(
        &crate::common::m2::practice_team(&s, "solo").await,
        "claude",
    );
    for who in ["team.a@t", agent.as_str()] {
        let (ok, _, err) = crate::common::seat_tools::direct(
            &s,
            who,
            "approve",
            json!({ "tool_name": "Bash", "input": { "command": "ls" } }),
        )
        .await;
        assert!(!ok && err.unwrap().contains("not available"), "{who}");
        assert_eq!(
            crate::common::seat_tools::rejected(&s, who)
                .into_iter()
                .map(|(t, _)| t)
                .collect::<Vec<_>>(),
            ["approve"],
            "{who}"
        );
    }
    assert!(
        !crate::common::m2::events(&s.st, &agent)
            .iter()
            .any(|e| matches!(e.body, EventBody::QuestionRaised { .. })),
        "nothing was asked"
    );
}

/// Expired questions of `team.a@t`: a permission prompt allowed on timeout, an agent question whose
/// decision Sculptured recorded as unknown, a native question, and one decided by the agent's words.
fn decision_fixtures(s: &Served) {
    let seat = "team.a@t";
    let decided = |id: &str, source: DecisionSource, provenance: Provenance| {
        let body = EventBody::QuestionDecided {
            question_id: id.into(),
            decision: "x".into(),
            rationale: None,
            source,
        };
        s.st.local_event(
            Actor::System("timeout".into()),
            Subject::Seat(seat.into()),
            provenance,
            body,
        )
        .unwrap();
    };
    crate::common::seat_tools::expired_question(
        s,
        seat,
        "q-perm",
        QuestionSource::NativePermission,
        Some("Bash"),
    );
    decided(
        "q-perm",
        DecisionSource::AllowedOnTimeout,
        Provenance::ProtocolObserved,
    );
    crate::common::seat_tools::expired_question(
        s,
        seat,
        "q-unknown",
        QuestionSource::RequestInput,
        None,
    );
    decided("q-unknown", DecisionSource::Unknown, Provenance::Heuristic);
    crate::common::seat_tools::expired_question(
        s,
        seat,
        "q-native",
        QuestionSource::NativeQuestion,
        None,
    );
    crate::common::seat_tools::expired_question(
        s,
        seat,
        "q-said",
        QuestionSource::RequestInput,
        None,
    );
    let said = EventBody::QuestionDecided {
        question_id: "q-said".into(),
        decision: "x".into(),
        rationale: None,
        source: DecisionSource::NextMessage,
    };
    s.st.local_event(
        Actor::Seat(seat.into()),
        Subject::Seat(seat.into()),
        Provenance::AgentReported,
        said,
    )
    .unwrap();
}

/// Important 5: never over what Sculptured observed (allowed on timeout) or recorded itself (unknown).
#[tokio::test]
async fn record_decision_never_overrides_sculptureds_own_record() {
    let s = crate::common::m2::serve().await;
    decision_fixtures(&s);
    for (id, kept) in [("q-perm", "allowed-on-timeout"), ("q-unknown", "unknown")] {
        let (ok, _, err) = crate::common::seat_tools::direct(
            &s,
            "team.a@t",
            "record_decision",
            json!({ "question_id": id, "choice": "Blue" }),
        )
        .await;
        assert!(!ok, "{id}: {err:?}");
        assert_eq!(
            crate::common::seat_tools::question_view(&s, "team.a@t", id).await["decision"]["source"],
            kept
        );
    }
    assert_eq!(
        crate::common::seat_tools::rejected(&s, "team.a@t").len(),
        2,
        "both refusals are evidence"
    );
}

/// The agent's structured decision on its own question replaces its unstructured one.
#[tokio::test]
async fn record_decision_records_the_agents_own_choice() {
    let s = crate::common::m2::serve().await;
    decision_fixtures(&s);
    for id in ["q-native", "q-said"] {
        let (ok, _, err) = crate::common::seat_tools::direct(
            &s,
            "team.a@t",
            "record_decision",
            json!({ "question_id": id, "choice": "Blue" }),
        )
        .await;
        assert!(ok, "{id}: {err:?}");
        let q = crate::common::seat_tools::question_view(&s, "team.a@t", id).await;
        assert_eq!(
            (
                q["decision"]["source"].as_str(),
                q["decision"]["text"].as_str()
            ),
            (Some("record-decision"), Some("Blue")),
            "{id}"
        );
    }
}
