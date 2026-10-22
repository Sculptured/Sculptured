//! Codex agents take part in handoffs: every Codex turn that ends `completed` after reading its
//! work is a work turn. fake-codex plays the CLI; nothing calls a model.
mod common;

use axum::http::StatusCode;

use common::m2::{Served, rid};
use controld::handoffs::CONTINUE_PREFIX;
use controld::reconciler::{HANDOFF_ACTOR, TEAM_START_ACTOR};
use drivers::claude::ClaudeStreamDriver;
use drivers::codex::CodexAppServerDriver;
use drivers::stub::StubDriver;

use model::{EventBody, SeatLifecycle, TurnStatus};
use serde_json::json;
use std::sync::Arc;

async fn codex_host(s: &Served) {
    let codex = CodexAppServerDriver {
        program: crate::common::fake_bin("fake-codex"),
        native_questions: false,
    };
    crate::common::m2::connect(s, vec![Arc::new(StubDriver), Arc::new(codex)], None).await;
    crate::common::wait_for("the Codex driver", || {
        s.st.hosts.has_driver("local", "codex-app-server")
    })
    .await;
}

/// A host offering Codex (fake-codex) and Claude (fake-claude) agents.
async fn mixed_host(s: &Served) {
    let codex = CodexAppServerDriver {
        program: crate::common::fake_bin("fake-codex"),
        native_questions: false,
    };
    let claude = ClaudeStreamDriver {
        program: crate::common::fake_bin("fake-claude"),
    };
    crate::common::m2::connect(
        s,
        vec![Arc::new(StubDriver), Arc::new(codex), Arc::new(claude)],
        None,
    )
    .await;
    crate::common::wait_for("the Codex and Claude drivers", || {
        s.st.hosts.has_driver("local", "codex-app-server")
            && s.st.hosts.has_driver("local", "claude-stream")
    })
    .await;
}

/// A started Codex team of `(id, name, job)` agents working in `<home>/work`, joined by
/// "hands work to" links.
async fn codex_team(
    s: &Served,
    name: &str,
    agents: &[(&str, &str, &str)],
    links: &[(&str, &str)],
) -> String {
    let agents: Vec<(&str, &str, &str, &str)> = agents
        .iter()
        .map(|(id, name, job)| (*id, *name, "codex", *job))
        .collect();
    team(s, name, &agents, links).await
}

/// A started team of `(id, name, kind, job)` agents working in `<home>/work`, joined by
/// "hands work to" links.
async fn team(
    s: &Served,
    name: &str,
    agents: &[(&str, &str, &str, &str)],
    links: &[(&str, &str)],
) -> String {
    team_with_models(s, name, agents, links, json!({})).await
}

/// `team`, with the draft's model defaults set to `models`.
async fn team_with_models(
    s: &Served,
    name: &str,
    agents: &[(&str, &str, &str, &str)],
    links: &[(&str, &str)],
    models: serde_json::Value,
) -> String {
    let work = s.home.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "practice": false, "name": name }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = agents
        .iter()
        .enumerate()
        .map(|(i, (id, name, kind, job))| {
            json!({ "id": id, "group_id": null, "name": name, "kind": kind, "job": job,
                    "folder": work.display().to_string(), "x": 40 + 260 * i as i32, "y": 40 })
        })
        .collect();
    draft["connections"] =
        links.iter().map(|(from, to)| json!({ "id": format!("c-{from}-{to}"), "from": from, "to": to, "kind": "hands-work-to" })).collect();
    draft["models"] = models;
    let id = draft["id"].as_str().unwrap().to_string();
    let revision = draft["revision"].clone();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": revision, "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, issues) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/validate"),
        json!({}),
    )
    .await;
    assert_eq!(
        issues,
        json!([]),
        "a team with Codex agents has nothing to fix and no warning"
    );
    // Not `start_team`: it waits for every agent to run, and a Job such as `crash-once` ends a
    // head agent's process before a later layer has started.
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("boot finished", || !s.st.is_booting(&id)).await;
    id
}

fn handoffs(s: &Served, seat: &str) -> Vec<String> {
    crate::common::m2::messages_by(&s.st, seat, HANDOFF_ACTOR)
        .into_iter()
        .map(|(_, _, text)| text)
        .collect()
}

fn stuck_reason(s: &Served, seat: &str) -> Option<String> {
    s.st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .unwrap()
        .handoff_stuck
        .map(|h| h.reason)
}

#[tokio::test]
async fn codex_agents_hand_work_along_the_chain() {
    let s = crate::common::m2::serve().await;
    codex_host(&s).await;
    let id = codex_team(
        &s,
        "Codex chain",
        &[
            ("lead", "Lead", "Plan the page."),
            ("builder", "Builder", "Build what the lead hands you."),
            ("reviewer", "Reviewer", "Review it."),
        ],
        &[("lead", "builder"), ("builder", "reviewer")],
    )
    .await;
    let (lead, builder, reviewer) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
        crate::common::m2::seat(&id, "reviewer"),
    );
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&lead)
            .unwrap()
            .unwrap()
            .runtime,
        rigspec::Runtime::Codex
    );
    crate::common::wait_for("the reviewer's handoff", || {
        !handoffs(&s, &reviewer).is_empty()
    })
    .await;
    assert_eq!(
        handoffs(&s, &builder),
        [
            "Handed over by Lead\n\nBuild what the lead hands you.\n\n--- Lead's result ---\ncodex received: Plan the page."
        ]
    );
    let to_reviewer = handoffs(&s, &reviewer);
    assert_eq!(to_reviewer.len(), 1);
    assert!(
        to_reviewer[0].starts_with("Handed over by Builder\n\nReview it.\n\n--- Builder's result ---\ncodex received: Handed over by Lead"),
        "{}",
        to_reviewer[0]
    );
    crate::common::m2::wait_event(&s.st, &reviewer, "the reviewer's work turn", |b| {
        matches!(
            b,
            EventBody::TurnEnded {
                status: TurnStatus::Completed,
                ..
            }
        )
    })
    .await;
    assert_eq!(
        crate::common::m2::messages_by(&s.st, &lead, TEAM_START_ACTOR).len(),
        1,
        "only the head agent gets its Job at Start"
    );
    for agent in [&lead, &builder, &reviewer] {
        assert!(crate::common::m2::events(&s.st, agent).iter().any(|e| matches!(&e.body,
            EventBody::MessageQueued { text, .. } if text.contains("Team architecture (operator configuration):")
                && text.contains("hands work to") && text.contains("Your role:"))), "briefing captured for {agent}");
        let consumed = crate::common::m2::events(&s.st, agent)
            .iter()
            .filter(|e| matches!(e.body, EventBody::MessageConsumed { .. }))
            .count();
        assert_eq!(consumed, 1, "{agent}: one message, one turn");
        assert_eq!(stuck_reason(&s, agent), None, "{agent}");
    }
}

#[tokio::test]
async fn a_failed_codex_turn_leaves_the_work_unfinished() {
    let s = crate::common::m2::serve().await;
    codex_host(&s).await;
    let id = codex_team(
        &s,
        "Codex fail",
        &[
            ("lead", "Lead", "fail"),
            ("builder", "Builder", "Build it."),
        ],
        &[("lead", "builder")],
    )
    .await;
    let (lead, builder) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
    );
    crate::common::m2::wait_event(&s.st, &lead, "the failed turn", |b| {
        matches!(
            b,
            EventBody::TurnEnded {
                status: TurnStatus::Failed,
                ..
            }
        )
    })
    .await;
    crate::common::wait_for("Lead stuck", || {
        stuck_reason(&s, &lead).as_deref() == Some("Lead's work did not finish (failed)")
    })
    .await;
    crate::common::wait_for("Builder blocked by Lead", || {
        stuck_reason(&s, &builder).as_deref() == Some("waiting on Lead (stuck)")
    })
    .await;
    assert!(
        handoffs(&s, &builder).is_empty(),
        "nothing is handed over from a failed turn"
    );
    assert!(crate::common::m2::events(&s.st, &lead).iter().any(|e| matches!(&e.body, EventBody::Unrecognized { raw } if raw["turnError"]["message"] == "fake-codex: the model request failed")));

    crate::common::m2::say(&s, &lead, "continue").await;
    crate::common::wait_for("the builder's handoff", || {
        !handoffs(&s, &builder).is_empty()
    })
    .await;
    assert!(
        handoffs(&s, &builder)[0].ends_with("--- Lead's result ---\ncodex received: continue"),
        "{:?}",
        handoffs(&s, &builder)
    );
    crate::common::wait_for("Lead no longer stuck", || stuck_reason(&s, &lead).is_none()).await;
}

#[tokio::test]
async fn a_crashed_codex_agent_restarts_and_continues_its_work() {
    let s = crate::common::m2::serve().await;
    codex_host(&s).await;
    let id = codex_team(
        &s,
        "Codex crash",
        &[
            ("lead", "Lead", "crash-once"),
            ("builder", "Builder", "Build it."),
        ],
        &[("lead", "builder")],
    )
    .await;
    let (lead, builder) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
    );
    crate::common::wait_for("Lead needs attention", || {
        crate::common::lifecycle(&s.st, &lead) == Some(SeatLifecycle::Attention)
    })
    .await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{lead}/restart"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::wait_for("the builder's handoff", || {
        !handoffs(&s, &builder).is_empty()
    })
    .await;
    let jobs: Vec<String> = crate::common::m2::messages_by(&s.st, &lead, TEAM_START_ACTOR)
        .into_iter()
        .map(|(_, _, t)| t)
        .collect();
    assert_eq!(jobs.len(), 2, "{jobs:?}");
    assert_eq!(
        jobs[1],
        format!("{CONTINUE_PREFIX}\n\ncrash-once"),
        "a fresh process gets the continue message with the work"
    );
    assert!(handoffs(&s, &builder)[0].contains(&format!(
        "--- Lead's result ---\ncodex received: {CONTINUE_PREFIX}"
    )));
}

fn tenure_driver(s: &Served, seat: &str) -> Option<String> {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::TenureStarted { driver, .. } => Some(driver),
            _ => None,
        })
}

/// A Claude lead, a Codex builder and a Claude reviewer (fake-claude and fake-codex): work crosses
/// from Claude to Codex and back, and each agent does exactly one work turn.
#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn claude_and_codex_agents_hand_work_to_each_other() {
    let s = crate::common::m2::serve().await;
    mixed_host(&s).await;
    let id = team(
        &s,
        "Mixed chain",
        &[
            ("lead", "Lead", "claude", "Plan the page."),
            (
                "builder",
                "Builder",
                "codex",
                "Build what the lead hands you.",
            ),
            ("reviewer", "Reviewer", "claude", "Review it."),
        ],
        &[("lead", "builder"), ("builder", "reviewer")],
    )
    .await;
    let (lead, builder, reviewer) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
        crate::common::m2::seat(&id, "reviewer"),
    );
    crate::common::m2::wait_event(&s.st, &reviewer, "the reviewer's work turn", |b| {
        matches!(
            b,
            EventBody::TurnEnded {
                status: TurnStatus::Completed,
                ..
            }
        )
    })
    .await;
    assert_eq!(tenure_driver(&s, &lead).as_deref(), Some("claude-stream"));
    assert_eq!(
        tenure_driver(&s, &builder).as_deref(),
        Some("codex-app-server")
    );
    assert_eq!(
        tenure_driver(&s, &reviewer).as_deref(),
        Some("claude-stream")
    );
    assert_eq!(
        handoffs(&s, &builder),
        [
            "Handed over by Lead\n\nBuild what the lead hands you.\n\n--- Lead's result ---\nfake: Plan the page."
        ],
        "Claude's result reaches Codex"
    );
    let to_reviewer = handoffs(&s, &reviewer);
    assert_eq!(to_reviewer.len(), 1);
    assert!(
        to_reviewer[0].starts_with(
            "Handed over by Builder\n\nReview it.\n\n--- Builder's result ---\ncodex received: Handed over by Lead\n\nBuild what the lead hands you."
        ),
        "Codex's result reaches Claude: {}",
        to_reviewer[0]
    );
    assert!(
        crate::common::m2::agent_texts(&s.st, &reviewer)
            .iter()
            .any(|t| t.starts_with("fake: Handed over by Builder")),
        "{:?}",
        crate::common::m2::agent_texts(&s.st, &reviewer)
    );
    for agent in [&lead, &builder, &reviewer] {
        let evs = crate::common::m2::events(&s.st, agent);
        let consumed = evs
            .iter()
            .filter(|e| matches!(e.body, EventBody::MessageConsumed { .. }))
            .count();
        let work = evs
            .iter()
            .filter(|e| {
                matches!(
                    e.body,
                    EventBody::TurnEnded {
                        status: TurnStatus::Completed,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            (consumed, work),
            (1, 1),
            "{agent}: one message, one work turn"
        );
        assert_eq!(stuck_reason(&s, agent), None, "{agent}");
    }
}

/// Codex starts no turn mid-turn, so a message sent while a turn runs waits in the driver. When the
/// app-server crashes first, that message never reached Codex: it fails in plain words.
#[tokio::test]
async fn a_message_held_in_the_driver_fails_when_codex_crashes() {
    let s = crate::common::m2::serve().await;
    codex_host(&s).await;
    let id = codex_team(
        &s,
        "Codex held",
        &[("lead", "Lead", "crash-on-interrupt")],
        &[],
    )
    .await;
    let lead = crate::common::m2::seat(&id, "lead");
    crate::common::m2::wait_event(&s.st, &lead, "the turn is running", |b| {
        matches!(b, EventBody::ToolCalled { .. })
    })
    .await;
    crate::common::m2::say(&s, &lead, "held until the turn ends").await;
    let held = crate::common::m2::events(&s.st, &lead)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::MessageQueued {
                message_id, text, ..
            } if common::work_message(&text) == "held until the turn ends" => Some(message_id),
            _ => None,
        })
        .expect("queued");
    crate::common::m2::wait_event(
        &s.st,
        &lead,
        "the driver took the message",
        |b| matches!(b, EventBody::MessageDelivered { message_id } if *message_id == held),
    )
    .await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{lead}/interrupt"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::m2::wait_event(&s.st, &lead, "the crash", |b| {
        matches!(
            b,
            EventBody::TenureEnded {
                exit_code: Some(3),
                ..
            }
        )
    })
    .await;
    crate::common::wait_for("the held message failed", || {
        s.st.store
            .lock()
            .unwrap()
            .message_reason(&held)
            .unwrap()
            .as_deref()
            == Some(controld::store::STOPPED_BEFORE_READING)
    })
    .await;
    assert_eq!(
        controld::store::STOPPED_BEFORE_READING,
        "the agent stopped before reading this message"
    );
    assert!(
        !crate::common::m2::events(&s.st, &lead).iter().any(|e| matches!(&e.body, EventBody::MessageConsumed { message_id, .. } if *message_id == held)),
        "Codex never read it"
    );
}

/// Codex names the model it runs only in its answer to the thread request, which the driver reads
/// during the handshake; the turn's tokens are priced from that model.
#[tokio::test]
async fn a_codex_turn_on_a_listed_model_reports_its_cost() {
    let s = crate::common::m2::serve().await;
    codex_host(&s).await;
    let id = team_with_models(
        &s,
        "Priced Codex",
        &[("solo", "Solo", "codex", "Do it.")],
        &[],
        json!({ "codex": "gpt-6-astra" }),
    )
    .await;
    let solo = crate::common::m2::seat(&id, "solo");
    crate::common::m2::wait_event(&s.st, &solo, "a usage report", |b| {
        matches!(b, EventBody::UsageReported { .. })
    })
    .await;
    let costs: Vec<_> = crate::common::m2::events(&s.st, &solo)
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::UsageReported { cost, .. } => Some(cost),
            _ => None,
        })
        .collect();
    assert!(
        costs.iter().all(|c| c
            .as_ref()
            .is_some_and(|c| c.source == model::pricing::CostSource::OpenaiListPrices)),
        "{costs:?}"
    );
}
