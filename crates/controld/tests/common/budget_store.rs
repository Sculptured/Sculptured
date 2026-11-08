//! Budget tests on Sculptured's own records: a team marked started with its agents running, and the
//! usage, plan readings and turns their CLIs would report, written straight into the store. No agent
//! runs and no background task acts by itself: each test runs the checks it means to test.
//! Included with `#[path = "common/budget_store.rs"] mod budget_store;` by the budget test files.
#![allow(dead_code)]
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use controld::AppState;
use controld::budget::{BudgetState, Record};
use controld::store::{RigSource, Store};
use http_body_util::BodyExt;
use model::{
    Actor, ContextBasis, EventBody, PlanUsage, PlanWindow, Provenance, SeatActivity, SeatLifecycle,
    Subject,
};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;
use tower::ServiceExt;

/// A controld whose background tasks (budget loop, handoffs, limits) never start.
pub struct Quiet {
    pub st: AppState,
    pub router: Router,
    pub home: tempfile::TempDir,
    host_seq: u64,
}

pub fn quiet() -> Quiet {
    let home = tempfile::tempdir().unwrap();
    let st = AppState::new(
        Store::open_in_memory().unwrap(),
        "test-token".into(),
        vec!["test".into()],
    )
    .with_home(home.path());
    st.background_started.store(true, Ordering::SeqCst);
    st.handoffs_started.store(true, Ordering::SeqCst);
    let router = controld::app(st.clone(), None);
    Quiet {
        st,
        router,
        home,
        host_seq: 0,
    }
}

pub async fn call(router: &Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "test")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// The seat id of `agent` in team `team`.
pub fn seat(team: &str, agent: &str) -> String {
    format!("team.{agent}@{team}")
}

impl Quiet {
    /// A saved team of independent Claude agents (on haiku, in `<home>/work`) with `budget`; `edit`
    /// changes the draft further before it is saved.
    pub async fn team(
        &self,
        agents: &[&str],
        budget: Value,
        edit: impl FnOnce(&mut Value),
    ) -> String {
        let work = self.home.path().join("work");
        std::fs::create_dir_all(&work).unwrap();
        let (status, rec) = call(
            &self.router,
            "POST",
            "/api/teams",
            json!({ "request_id": rid(), "template": "blank", "name": "Spend" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{rec}");
        let mut draft = rec["draft"].clone();
        draft["agents"] = agents
            .iter()
            .enumerate()
            .map(|(i, id)| {
                json!({ "id": id, "group_id": null, "name": id, "kind": "claude", "job": "Reply with OK.",
                        "folder": work.display().to_string(), "x": 40 + 260 * i as i32, "y": 40 })
            })
            .collect();
        draft["connections"] = json!([]);
        draft["models"] = json!({ "claude": "haiku" });
        draft["budget"] = budget;
        edit(&mut draft);
        let id = draft["id"].as_str().unwrap().to_string();
        let (status, body) = call(
            &self.router,
            "PUT",
            &format!("/api/teams/{id}"),
            json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        id
    }

    /// Marks team `id` started as a Start does (its budget recorded, then booted) with every agent
    /// running; returns the start's seq.
    pub fn start(&mut self, id: &str) -> i64 {
        let (draft, start) = {
            let mut store = self.st.store.lock().unwrap();
            let draft = store.draft(id).unwrap().unwrap();
            let rig = rigspec::from_draft(&draft).unwrap().rig;
            store
                .upsert_rig(
                    &rig,
                    &RigSource::Team {
                        id: id.to_string(),
                        revision: draft.revision,
                    },
                    model::Desired::Running,
                )
                .unwrap();
            (draft, store.last_seq().unwrap())
        };
        controld::budget::begin(&self.st, id, &draft.budget, start).unwrap();
        self.st
            .store
            .lock()
            .unwrap()
            .append_local(
                Actor::Human("local".into()),
                Subject::Rig(id.to_string()),
                Provenance::Operator,
                EventBody::RigBooted {
                    spec_hash: "x".into(),
                    seat_count: u32::try_from(draft.agents.len()).unwrap(),
                },
            )
            .unwrap();
        for a in &draft.agents {
            self.host(
                &seat(id, &a.id),
                EventBody::SeatLifecycleChanged {
                    lifecycle: SeatLifecycle::Running,
                    reason: None,
                },
            );
        }
        start
    }

    /// An event from the agent's runner, observed at `at_ms` (now when None).
    pub fn host_at(&mut self, seat: &str, body: EventBody, at_ms: Option<i64>) -> i64 {
        self.host_seq += 1;
        let ev = host_proto::HostEvent {
            host_seq: self.host_seq,
            at_ms: at_ms.unwrap_or_else(model::now_ms),
            actor: Actor::System("hostd".into()),
            subject: Subject::Seat(seat.to_string()),
            provenance: Provenance::ProtocolObserved,
            body,
        };
        self.st
            .store
            .lock()
            .unwrap()
            .ingest("local", ev)
            .unwrap()
            .last()
            .unwrap()
            .event
            .seq
    }

    pub fn host(&mut self, seat: &str, body: EventBody) -> i64 {
        self.host_at(seat, body, None)
    }

    /// One model call's usage, reported while the turn runs.
    pub fn usage(&mut self, seat: &str, output: u64, cache_read: u64) {
        self.host(
            seat,
            EventBody::UsageReported {
                turn_id: Some("t1".into()),
                input_tokens: 0,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_write_tokens: 0,
                cost: None,
                in_turn: true,
            },
        );
    }

    /// The account's Claude 5-hour use, as a CLI reports it.
    pub fn plan(&mut self, seat: &str, percent: f64, resets_at: i64, at_ms: i64) {
        let usage = PlanUsage {
            provider: "claude".into(),
            windows: vec![PlanWindow {
                name: "five_hour".into(),
                used_percent: Some(percent),
                resets_at: Some(resets_at),
            }],
            status: Some("allowed".into()),
            credits: None,
        };
        self.host_at(seat, EventBody::PlanUsageReported { usage }, Some(at_ms));
    }

    /// A turn starts: the agent is working.
    pub fn turn_started(&mut self, seat: &str) {
        self.host(
            seat,
            EventBody::TurnStarted {
                turn_id: "t1".into(),
            },
        );
        self.activity(seat, SeatActivity::Working);
    }

    pub fn activity(&mut self, seat: &str, activity: SeatActivity) {
        self.host(seat, EventBody::SeatActivityChanged { activity });
    }

    pub fn context(&mut self, seat: &str, used: u64) {
        self.host(
            seat,
            EventBody::ContextReported {
                turn_id: Some("t1".into()),
                used_tokens: used,
                window_tokens: Some(200_000),
                basis: ContextBasis::Reported,
            },
        );
    }

    pub fn record(&self, id: &str) -> Record {
        self.st
            .store
            .lock()
            .unwrap()
            .run_budget(id)
            .unwrap()
            .unwrap()
    }

    pub fn state(&self, id: &str) -> BudgetState {
        self.record(id).state
    }

    /// The run's budget changes in its log, oldest first.
    pub fn changes(&self, id: &str) -> Vec<String> {
        let changes = self
            .st
            .store
            .lock()
            .unwrap()
            .latest_of_types(&Subject::Rig(id.to_string()), &["budget-state-changed"], 10)
            .unwrap();
        changes
            .iter()
            .rev()
            .map(|e| match &e.body {
                EventBody::BudgetStateChanged { state, reason } => {
                    assert!(!reason.is_empty());
                    state.clone()
                }
                other => panic!("{other:?}"),
            })
            .collect()
    }
}
