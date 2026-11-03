//! Safety stops never wait for a team action: Stop everything now, and an agent's hard stop, go ahead
//! while Resume all holds the team's claim, and Resume all leaves the stopped agents stopped
//! (Practice agents whose session resume waits at a gate the test opens; no model is called).
mod common;
use axum::http::StatusCode;
use common::m2::{Served, call, connect, rid, seat, start_team};
use common::pausing::*;
use controld::store::ControlMode;
use model::{Caps, SeatLifecycle};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// The Practice driver, except that resuming a session waits for a permit of `gate`.
struct Gated {
    gate: Arc<Semaphore>,
}

#[async_trait::async_trait]
impl drivers::Driver for Gated {
    fn name(&self) -> &'static str {
        "stub"
    }
    fn capabilities(&self) -> Caps {
        drivers::stub::StubDriver.capabilities()
    }
    async fn launch(
        &self,
        spec: drivers::LaunchSpec,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        drivers::stub::StubDriver.launch(spec).await
    }
    async fn resume(
        &self,
        spec: drivers::LaunchSpec,
        native_id: &str,
    ) -> Result<Box<dyn drivers::Session>, drivers::DriverError> {
        self.gate
            .acquire()
            .await
            .expect("the gate stays open")
            .forget();
        drivers::stub::StubDriver.resume(spec, native_id).await
    }
}

/// A started team of `agents`, all paused, with a runner whose session resumes wait at the gate.
async fn paused_team(name: &str, agents: &[&str]) -> (Served, Paused) {
    let s = common::m2::serve().await;
    let gate = Arc::new(Semaphore::new(0));
    let driver = Gated { gate: gate.clone() };
    let runner = connect(&s, vec![Arc::new(driver)], None).await;
    let id = team(&s, name, agents).await;
    start_team(&s, &id).await;
    let seats: Vec<String> = agents.iter().map(|a| seat(&id, a)).collect();
    ok(team_action(&s, &id, "pause", &rid()).await);
    wait_paused(&s, &seats.iter().collect::<Vec<_>>()).await;
    let paused = Paused {
        _runner: runner,
        gate,
        id,
        seats,
    };
    (s, paused)
}

struct Paused {
    _runner: hostd::supervisor::Supervisor,
    gate: Arc<Semaphore>,
    id: String,
    seats: Vec<String>,
}

/// Starts Resume all in the background, and waits until it holds the claim with an agent starting.
async fn resume_all_in_flight(
    s: &Served,
    id: &str,
) -> tokio::task::JoinHandle<(StatusCode, Value)> {
    let (router, uri) = (s.router.clone(), format!("/api/teams/{id}/resume"));
    let resume =
        tokio::spawn(
            async move { call(&router, "POST", &uri, json!({ "request_id": rid() })).await },
        );
    common::wait_for("an agent resuming", || {
        s.st.team_action_running(id)
            && s.st
                .store
                .lock()
                .unwrap()
                .rig_snapshot(id)
                .unwrap()
                .is_some_and(|snap| {
                    snap.seats
                        .iter()
                        .any(|v| v.lifecycle == SeatLifecycle::Launching)
                })
    })
    .await;
    resume
}

async fn stop_now(s: &Served, id: &str) -> (StatusCode, Value) {
    let uri = format!("/api/teams/{id}/stop-now");
    let call = call(&s.router, "POST", &uri, json!({ "request_id": rid() }));
    tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("a safety stop does not wait for Resume all")
}

fn control_of(s: &Served, seat: &str) -> Option<ControlMode> {
    s.st.store.lock().unwrap().control(seat).unwrap()
}

/// No agent of `seats` runs, now or a moment later (nothing started after the stop).
async fn stays_stopped(s: &Served, seats: &[String]) {
    wait_lifecycle(s, &seats.iter().collect::<Vec<_>>(), SeatLifecycle::Stopped).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    for x in seats {
        assert_eq!(
            common::lifecycle(&s.st, x),
            Some(SeatLifecycle::Stopped),
            "{x}"
        );
    }
}

#[tokio::test]
async fn stop_everything_now_goes_ahead_during_resume_all_and_nothing_resumes_after_it() {
    let (s, p) = paused_team("preempt-team", &["a", "b"]).await;
    let (gate, id, seats) = (&p.gate, &p.id, &p.seats);
    let resume = resume_all_in_flight(&s, id).await;

    ok(stop_now(&s, id).await);
    gate.add_permits(10);
    let body = ok(resume.await.unwrap());

    assert_eq!(
        affected(&body),
        Vec::<String>::new(),
        "nothing resumed: {body}"
    );
    let skipped = skipped(&body);
    assert_eq!(skipped.len(), 2, "{body}");
    for (seat, code, reason) in skipped {
        assert_eq!(code, "already_stopping", "{seat}: {reason}");
        assert!(reason.contains("stopped meanwhile"), "{reason}");
    }
    stays_stopped(&s, seats).await;
    // The one that was starting is stopped for good; the other is still paused, its session kept.
    let modes: Vec<_> = seats.iter().map(|x| control_of(&s, x)).collect();
    assert!(modes.contains(&Some(ControlMode::Hard)), "{modes:?}");
    assert!(
        modes
            .iter()
            .all(|m| matches!(m, Some(ControlMode::Hard | ControlMode::Paused))),
        "{modes:?}"
    );
    assert!(
        !s.st.team_action_running(id),
        "Resume all let go of the team"
    );
}

#[tokio::test]
async fn an_agents_hard_stop_goes_ahead_during_resume_all_and_only_it_stays_stopped() {
    let (s, p) = paused_team("preempt-agent", &["a", "b"]).await;
    let (gate, id, seats) = (&p.gate, &p.id, &p.seats);
    let resume = resume_all_in_flight(&s, id).await;
    let starting = seats
        .iter()
        .find(|x| common::lifecycle(&s.st, x) == Some(SeatLifecycle::Launching))
        .unwrap()
        .clone();
    let other = seats.iter().find(|x| **x != starting).unwrap().clone();

    let stop = control(&s, &starting, "hard");
    let (status, body) = tokio::time::timeout(Duration::from_secs(5), stop)
        .await
        .expect("an agent's hard stop does not wait for Resume all");
    assert_eq!(status, StatusCode::OK, "{body}");
    gate.add_permits(10);
    let body = ok(resume.await.unwrap());

    assert_eq!(
        affected(&body),
        vec![other.clone()],
        "the other agent resumes: {body}"
    );
    stays_stopped(&s, std::slice::from_ref(&starting)).await;
    assert_eq!(control_of(&s, &starting), Some(ControlMode::Hard));
    wait_lifecycle(&s, &[&other], SeatLifecycle::Running).await;
}

#[tokio::test]
async fn another_team_action_is_refused_in_plain_words_while_one_runs() {
    let (s, p) = paused_team("preempt-busy", &["a", "b"]).await;
    let (gate, id, seats) = (&p.gate, &p.id, &p.seats);
    let resume = resume_all_in_flight(&s, id).await;
    let (status, body) = team_action(&s, id, "pause", &rid()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "team_busy", "{body}");
    gate.add_permits(10);
    ok(resume.await.unwrap());
    wait_lifecycle(
        &s,
        &seats.iter().collect::<Vec<_>>(),
        SeatLifecycle::Running,
    )
    .await;
}
