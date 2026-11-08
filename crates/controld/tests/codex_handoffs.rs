//! Codex agents take part in handoffs: every Codex turn that ends `completed` after reading its
//! work is a work turn. Agents run on the real Codex CLI (and Claude Code for the mixed team) on the
//! cheapest models (live tests); the planner's view of a failed turn is also checked on events
//! written to the store.
mod common;
#[path = "common/crash.rs"]
mod crash;

use axum::http::StatusCode;

use common::m2::{Served, rid};
use controld::handoffs::CONTINUE_PREFIX;
use controld::reconciler::{HANDOFF_ACTOR, TEAM_START_ACTOR};
use controld::store::RigSource;
use hostd::supervisor::Supervisor;
use model::{Actor, EventBody, Provenance, SeatLifecycle, Subject, TurnStatus};
use serde_json::json;

/// The job each live agent gets unless a test needs another: one cheap reply.
const REPLY: &str = "Reply with one short sentence.";

/// A live agent's job that hangs (a real long shell command) the first time and replies straight
/// away after: `flag` marks the first time, in the agent's folder.
fn hang_once(flag: &str) -> String {
    format!(
        "Use your shell tool to run exactly: test -e {flag} || (touch {flag} && sleep 600)\nThen reply with one short sentence."
    )
}

/// The real Codex CLI, or None (with the reason printed) when this machine cannot run the test.
fn require() -> Option<()> {
    common::live::require_codex().map(|_| ())
}

/// A host on this computer offering the real Codex and Claude Code CLIs.
async fn host(s: &Served) -> Supervisor {
    let sup = common::live::connect(s).await;
    crate::common::wait_for("the Codex and Claude drivers", || {
        s.st.hosts.has_driver("local", "codex-app-server")
            && s.st.hosts.has_driver("local", "claude-stream")
    })
    .await;
    sup
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

/// A started team of `(id, name, kind, job)` agents working in `<home>/work` on the cheapest
/// models, joined by "hands work to" links.
async fn team(
    s: &Served,
    name: &str,
    agents: &[(&str, &str, &str, &str)],
    links: &[(&str, &str)],
) -> String {
    let id = draft_team(s, name, agents, links).await;
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
    // Not `start_team`: it waits for every agent to run, and a head agent may crash first.
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    common::live::wait_for("boot finished", || !s.st.is_booting(&id)).await;
    id
}

/// The team `team` starts, saved but not started.
async fn draft_team(
    s: &Served,
    name: &str,
    agents: &[(&str, &str, &str, &str)],
    links: &[(&str, &str)],
) -> String {
    let work = common::live::work_folder(s);
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": name }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = agents
        .iter()
        .enumerate()
        .map(|(i, (id, name, kind, job))| {
            json!({ "id": id, "group_id": null, "name": name, "kind": kind, "job": job,
                    "folder": work, "x": 40 + 260 * i as i32, "y": 40 })
        })
        .collect();
    draft["connections"] =
        links.iter().map(|(from, to)| json!({ "id": format!("c-{from}-{to}"), "from": from, "to": to, "kind": "hands-work-to" })).collect();
    draft["models"] =
        json!({ "claude": common::live::CLAUDE_MODEL, "codex": common::live::CODEX_MODEL });
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

fn count(s: &Served, seat: &str, pred: impl Fn(&EventBody) -> bool) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| pred(&e.body))
        .count()
}

fn completed(b: &EventBody) -> bool {
    matches!(
        b,
        EventBody::TurnEnded {
            status: TurnStatus::Completed,
            ..
        }
    )
}

#[tokio::test]
#[ignore = "live: real codex CLI on the cheapest model; run with --ignored"]
async fn live_codex_agents_hand_work_along_the_chain() {
    let Some(()) = require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _host = host(&s).await;
    let id = codex_team(
        &s,
        "Codex chain",
        &[
            ("lead", "Lead", REPLY),
            ("builder", "Builder", REPLY),
            ("reviewer", "Reviewer", REPLY),
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
    common::live::wait_event(&s.st, &reviewer, "the reviewer's work turn", completed).await;
    let to_builder = handoffs(&s, &builder);
    assert_eq!(to_builder.len(), 1, "{to_builder:?}");
    assert!(
        to_builder[0].starts_with(&format!(
            "Handed over by Lead\n\n{REPLY}\n\n--- Lead's result ---\n"
        )),
        "{}",
        to_builder[0]
    );
    let to_reviewer = handoffs(&s, &reviewer);
    assert_eq!(to_reviewer.len(), 1, "{to_reviewer:?}");
    assert!(
        to_reviewer[0].starts_with(&format!(
            "Handed over by Builder\n\n{REPLY}\n\n--- Builder's result ---\n"
        )),
        "{}",
        to_reviewer[0]
    );
    assert_eq!(
        crate::common::m2::messages_by(&s.st, &lead, TEAM_START_ACTOR).len(),
        1,
        "only the head agent gets its Job at Start"
    );
    for agent in [&lead, &builder, &reviewer] {
        assert!(crate::common::m2::events(&s.st, agent).iter().any(|e| matches!(&e.body,
            EventBody::MessageQueued { text, .. } if text.contains("Team architecture (operator configuration):")
                && text.contains("hands work to") && text.contains("Your role:"))), "briefing captured for {agent}");
        let consumed = count(&s, agent, |b| {
            matches!(b, EventBody::MessageConsumed { .. })
        });
        assert_eq!(consumed, 1, "{agent}: one message, one turn");
        assert_eq!(stuck_reason(&s, agent), None, "{agent}");
    }
}

/// A Codex turn that fails (the CLI's model request is refused: no such model) leaves the work
/// unfinished, and nothing is handed over.
#[tokio::test]
#[ignore = "live: real codex CLI; run with --ignored"]
async fn live_a_failed_codex_turn_leaves_the_work_unfinished() {
    let Some(()) = require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _host = host(&s).await;
    let id = draft_team(
        &s,
        "Codex fail",
        &[
            ("lead", "Lead", "codex", REPLY),
            ("builder", "Builder", "codex", REPLY),
        ],
        &[("lead", "builder")],
    )
    .await;
    // The lead alone runs a model that does not exist, so its CLI's request fails.
    let (_, rec) =
        crate::common::m2::call(&s.router, "GET", &format!("/api/teams/{id}"), json!(null)).await;
    let mut draft = rec["draft"].clone();
    for a in draft["agents"].as_array_mut().unwrap() {
        if a["id"] == "lead" {
            a["advanced"] = json!({ "model": "no-such-model-xyz" });
        }
    }
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": rec["draft"]["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({ "request_id": rid() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (lead, builder) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
    );
    common::live::wait_event(&s.st, &lead, "the failed turn", |b| {
        matches!(
            b,
            EventBody::TurnEnded {
                status: TurnStatus::Failed,
                ..
            }
        )
    })
    .await;
    common::live::wait_for("Lead stuck", || {
        stuck_reason(&s, &lead).as_deref() == Some("Lead's work did not finish (failed)")
    })
    .await;
    common::live::wait_for("Builder blocked by Lead", || {
        stuck_reason(&s, &builder).as_deref() == Some("waiting on Lead (stuck)")
    })
    .await;
    assert!(
        handoffs(&s, &builder).is_empty(),
        "nothing is handed over from a failed turn"
    );
    crate::common::m2::stop_team(&s, &id).await;
}

/// Events written straight into the store as the runner and controld record them (nothing is
/// published, so the dispatcher does not react); `controld::handoffs::process_rig` plans from them.
struct Script<'a> {
    s: &'a Served,
    host_seq: u64,
}

impl Script<'_> {
    fn host(&mut self, seat: &str, body: EventBody) -> i64 {
        self.host_seq += 1;
        let ev = host_proto::HostEvent {
            host_seq: self.host_seq,
            at_ms: model::now_ms(),
            actor: Actor::System("hostd".into()),
            subject: Subject::Seat(seat.to_string()),
            provenance: Provenance::ProtocolObserved,
            body,
        };
        let mut store = self.s.st.store.lock().unwrap();
        store.ingest("local", ev).unwrap().last().unwrap().event.seq
    }

    fn local(&mut self, actor: Actor, subject: Subject, body: EventBody) -> i64 {
        let mut store = self.s.st.store.lock().unwrap();
        let rec = store.append_local(actor, subject, Provenance::Operator, body);
        rec.unwrap().event.seq
    }

    /// A message to `seat` from `actor`, read in turn `turn`, which ends `status` after writing `text`.
    fn turn(&mut self, seat: &str, actor: &str, message: &str, turn: &str, status: TurnStatus) {
        let queued = EventBody::MessageQueued {
            message_id: message.into(),
            intent: model::MessageIntent::Inform,
            text: "work".into(),
        };
        let subject = Subject::Seat(seat.to_string());
        let actor = if actor == "operator" {
            Actor::Human("local".into())
        } else {
            Actor::System(actor.into())
        };
        self.local(actor, subject, queued);
        let (message, turn) = (message.to_string(), turn.to_string());
        for body in [
            EventBody::MessageDelivered {
                message_id: message.clone(),
            },
            EventBody::TurnStarted {
                turn_id: turn.clone(),
            },
            EventBody::MessageConsumed {
                message_id: message,
                turn_id: turn.clone(),
            },
            EventBody::TurnEnded {
                turn_id: turn,
                status,
            },
        ] {
            self.host(seat, body);
        }
    }
}

/// A failed turn leaves the work it read unfinished: the agent is stuck, so is the agent waiting on
/// it, and nothing is owed downstream. A later turn the operator asks for finishes it.
#[tokio::test]
async fn a_failed_codex_turn_leaves_the_work_unfinished_until_a_later_turn_finishes_it() {
    let s = crate::common::m2::serve().await;
    let id = draft_team(
        &s,
        "Codex fail",
        &[
            ("lead", "Lead", "codex", REPLY),
            ("builder", "Builder", "codex", REPLY),
        ],
        &[("lead", "builder")],
    )
    .await;
    let (lead, builder) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
    );
    {
        let mut store = s.st.store.lock().unwrap();
        let draft = store.draft(&id).unwrap().unwrap();
        let rig = rigspec::from_draft(&draft).unwrap().rig;
        let source = RigSource::Team {
            id: id.clone(),
            revision: draft.revision,
        };
        store
            .upsert_rig(&rig, &source, model::Desired::Running)
            .unwrap();
    }
    let mut script = Script { s: &s, host_seq: 0 };
    script.local(
        Actor::Human("local".into()),
        Subject::Rig(id.clone()),
        EventBody::RigBooted {
            spec_hash: "x".into(),
            seat_count: 2,
        },
    );
    for seat in [&lead, &builder] {
        script.host(
            seat,
            EventBody::SeatLifecycleChanged {
                lifecycle: SeatLifecycle::Running,
                reason: None,
            },
        );
    }
    script.turn(&lead, TEAM_START_ACTOR, "job", "t1", TurnStatus::Failed);
    controld::handoffs::process_rig(&s.st, &id, false).await;
    assert_eq!(
        stuck_reason(&s, &lead).as_deref(),
        Some("Lead's work did not finish (failed)")
    );
    assert_eq!(
        stuck_reason(&s, &builder).as_deref(),
        Some("waiting on Lead (stuck)")
    );

    script.turn(&lead, "operator", "go-on", "t2", TurnStatus::Completed);
    controld::handoffs::process_rig(&s.st, &id, false).await;
    assert_eq!(stuck_reason(&s, &lead), None, "the later turn finished it");
    assert_eq!(stuck_reason(&s, &builder), None);
}

#[tokio::test]
#[ignore = "live: real codex CLI on the cheapest model; run with --ignored"]
async fn live_a_crashed_codex_agent_restarts_and_continues_its_work() {
    let Some(()) = require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let runner = host(&s).await;
    let lead_job = hang_once("lead.flag");
    let id = codex_team(
        &s,
        "Codex crash",
        &[("lead", "Lead", &lead_job), ("builder", "Builder", REPLY)],
        &[("lead", "builder")],
    )
    .await;
    let (lead, builder) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
    );
    common::live::wait_event(&s.st, &lead, "the lead is in its long command", |b| {
        matches!(b, EventBody::ToolCalled { .. })
    })
    .await;
    crash::kill_agent(&runner, &lead);
    common::live::wait_for("Lead needs attention", || {
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
    common::live::wait_for("the builder's handoff", || {
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
        format!("{CONTINUE_PREFIX}\n\n{lead_job}"),
        "a fresh process gets the continue message with the work"
    );
    assert!(
        handoffs(&s, &builder)[0].contains("--- Lead's result ---\n"),
        "{:?}",
        handoffs(&s, &builder)
    );
    crate::common::m2::stop_team(&s, &id).await;
}

fn tenure_driver(s: &Served, seat: &str) -> Option<String> {
    crate::common::m2::events(&s.st, seat)
        .into_iter()
        .find_map(|e| match e.body {
            EventBody::TenureStarted { driver, .. } => Some(driver),
            _ => None,
        })
}

/// A Claude lead, a Codex builder and a Claude reviewer: work crosses from Claude to Codex and back,
/// and each agent does exactly one work turn.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku and codex CLI on the cheapest model; run with --ignored"]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn live_claude_and_codex_agents_hand_work_to_each_other() {
    let Some(_) = common::live::require() else {
        return;
    };
    let Some(()) = require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _host = host(&s).await;
    let id = team(
        &s,
        "Mixed chain",
        &[
            ("lead", "Lead", "claude", REPLY),
            ("builder", "Builder", "codex", REPLY),
            ("reviewer", "Reviewer", "claude", REPLY),
        ],
        &[("lead", "builder"), ("builder", "reviewer")],
    )
    .await;
    let (lead, builder, reviewer) = (
        crate::common::m2::seat(&id, "lead"),
        crate::common::m2::seat(&id, "builder"),
        crate::common::m2::seat(&id, "reviewer"),
    );
    common::live::wait_event(&s.st, &reviewer, "the reviewer's work turn", completed).await;
    assert_eq!(tenure_driver(&s, &lead).as_deref(), Some("claude-stream"));
    assert_eq!(
        tenure_driver(&s, &builder).as_deref(),
        Some("codex-app-server")
    );
    assert_eq!(
        tenure_driver(&s, &reviewer).as_deref(),
        Some("claude-stream")
    );
    let to_builder = handoffs(&s, &builder);
    assert_eq!(to_builder.len(), 1, "{to_builder:?}");
    assert!(
        to_builder[0].starts_with(&format!(
            "Handed over by Lead\n\n{REPLY}\n\n--- Lead's result ---\n"
        )),
        "Claude's result reaches Codex: {}",
        to_builder[0]
    );
    let to_reviewer = handoffs(&s, &reviewer);
    assert_eq!(to_reviewer.len(), 1, "{to_reviewer:?}");
    assert!(
        to_reviewer[0].starts_with(&format!(
            "Handed over by Builder\n\n{REPLY}\n\n--- Builder's result ---\n"
        )),
        "Codex's result reaches Claude: {}",
        to_reviewer[0]
    );
    assert!(
        !crate::common::m2::agent_texts(&s.st, &reviewer).is_empty(),
        "the reviewer answered"
    );
    for agent in [&lead, &builder, &reviewer] {
        let consumed = count(&s, agent, |b| {
            matches!(b, EventBody::MessageConsumed { .. })
        });
        let work = count(&s, agent, completed);
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
#[ignore = "live: real codex CLI on the cheapest model; run with --ignored"]
async fn live_a_message_held_in_the_driver_fails_when_codex_crashes() {
    let Some(()) = require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let runner = host(&s).await;
    let job = hang_once("held.flag");
    let id = codex_team(&s, "Codex held", &[("lead", "Lead", &job)], &[]).await;
    let lead = crate::common::m2::seat(&id, "lead");
    common::live::wait_event(&s.st, &lead, "the turn is running", |b| {
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
    common::live::wait_event(
        &s.st,
        &lead,
        "the driver took the message",
        |b| matches!(b, EventBody::MessageDelivered { message_id } if *message_id == held),
    )
    .await;
    crash::kill_agent(&runner, &lead);
    common::live::wait_event(&s.st, &lead, "the crash", |b| {
        matches!(b, EventBody::TenureEnded { .. })
    })
    .await;
    common::live::wait_for("the held message failed", || {
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
#[ignore = "live: real codex CLI on the cheapest model; run with --ignored"]
async fn live_a_codex_turn_on_a_listed_model_reports_its_cost() {
    let Some(()) = require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _host = host(&s).await;
    let id = codex_team(&s, "Priced Codex", &[("solo", "Solo", REPLY)], &[]).await;
    let solo = crate::common::m2::seat(&id, "solo");
    common::live::wait_event(&s.st, &solo, "a usage report", |b| {
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
    crate::common::m2::stop_team(&s, &id).await;
}
