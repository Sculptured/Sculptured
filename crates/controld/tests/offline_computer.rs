//! The computer running a team's agents drops off mid-run (Wi-Fi change, sleep, a server deploy) and
//! comes back. Nothing the agents were doing is lost: their calls wait, answers wait, and Stop is
//! carried out on reconnect.
mod common;

use common::m2::{Served, TOKEN};
use drivers::stub::StubDriver;
use hostd::journal::Journal;
use hostd::supervisor::Supervisor;

use model::{Answer, EventBody, SeatLifecycle};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

/// This machine's runner, with a link that the test can cut and bring back.
struct Runner {
    sup: Supervisor,
    link: JoinHandle<()>,
}

impl Runner {
    async fn connect(s: &Served) -> Runner {
        let sup = Supervisor::new(
            "local".into(),
            vec![Arc::new(StubDriver)],
            Journal::open_in_memory(10_000).unwrap(),
        );
        let link = Runner::link(s, &sup).await;
        Runner { sup, link }
    }

    async fn link(s: &Served, sup: &Supervisor) -> JoinHandle<()> {
        let (url, sup) = (s.url.clone(), sup.clone());
        let link = tokio::spawn(async move {
            let _ = hostd::run_link(&url, TOKEN, &sup).await;
        });
        crate::common::wait_for("computer connected", || s.st.hosts.is_connected("local")).await;
        link
    }

    async fn drop_link(&self, s: &Served) {
        self.link.abort();
        crate::common::wait_for("computer gone", || !s.st.hosts.is_connected("local")).await;
    }

    async fn reconnect(&mut self, s: &Served) {
        self.link = Runner::link(s, &self.sup).await;
    }
}

fn questions_raised(s: &Served, seat: &str) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| matches!(e.body, EventBody::QuestionRaised { .. }))
        .count()
}

#[tokio::test]
async fn a_question_answered_while_the_computer_is_offline_reaches_the_agent_when_it_returns() {
    let s = crate::common::m2::serve().await;
    let mut runner = Runner::connect(&s).await;
    let agent = crate::common::m2::seat(
        &crate::common::m2::practice_team(&s, "solo").await,
        "claude",
    );
    let sup = runner.sup.clone();
    let a = agent.clone();
    let call = tokio::spawn(async move {
        sup.tool_call(&a, "request_input", json!({ "question": "Which colour?" }))
            .await
    });
    let raised = crate::common::m2::wait_event(&s.st, &agent, "a question", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    })
    .await;
    let EventBody::QuestionRaised { question_id: q, .. } = raised.body else {
        unreachable!()
    };

    runner.drop_link(&s).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        crate::common::seat_tools::question_view(&s, &agent, &q).await["state"],
        "open",
        "the question stays open while the computer is away"
    );
    controld::questions::answer(
        &s.st,
        &q,
        Answer::Text {
            text: "Green".into(),
        },
    )
    .await
    .expect("answering never needs the computer");
    assert!(!call.is_finished(), "the agent is still waiting");

    runner.reconnect(&s).await;
    let got = tokio::time::timeout(Duration::from_secs(10), call)
        .await
        .expect("the answer arrives")
        .unwrap()
        .unwrap();
    assert!(got.to_string().contains("Green"), "{got}");
    assert_eq!(
        questions_raised(&s, &agent),
        1,
        "the call sent again is answered once, not asked twice"
    );
}

#[tokio::test]
async fn stopping_a_team_while_its_computer_is_offline_works_and_happens_when_it_returns() {
    let s = crate::common::m2::serve().await;
    let mut runner = Runner::connect(&s).await;
    let team = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&team, "claude");
    runner.drop_link(&s).await;
    crate::common::m2::stop_team(&s, &team).await;
    // A computer that may never come back cannot hold the stop open: it is done here at once.
    let v = s.st.store.lock().unwrap().seat(&agent).unwrap().unwrap();
    assert_eq!(v.lifecycle, SeatLifecycle::Stopped);
    assert!(
        v.lifecycle_reason
            .as_deref()
            .unwrap_or("")
            .contains("its computer is offline"),
        "{:?}",
        v.lifecycle_reason
    );
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "PUT",
            &format!("/api/project-visibility/{team}"),
            json!({ "status": "trashed" })
        )
        .await
        .0,
        200,
        "nothing counts as running"
    );
    assert!(
        !runner.sup.live_seats().await.is_empty(),
        "the agent still runs on the offline computer"
    );

    runner.reconnect(&s).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !runner.sup.live_seats().await.is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the computer ends the agent when it returns"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        crate::common::lifecycle(&s.st, &agent),
        Some(SeatLifecycle::Stopped)
    );
}

#[tokio::test]
async fn a_team_started_during_a_brief_drop_launches_once_the_computer_is_back() {
    let s = crate::common::m2::serve().await;
    let mut runner = Runner::connect(&s).await;
    let team = crate::common::m2::practice_chain(
        &s,
        "Pair",
        &[("a", "A", "first"), ("b", "B", "second")],
        &[],
    )
    .await;
    runner.drop_link(&s).await;
    let back = {
        let s_url = s.url.clone();
        let sup = runner.sup.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let _ = hostd::run_link(&s_url, TOKEN, &sup).await;
        })
    };
    crate::common::m2::start_team(&s, &team).await;
    runner.link = back;
    for a in ["a", "b"] {
        assert_eq!(
            crate::common::lifecycle(&s.st, &crate::common::m2::seat(&team, a)),
            Some(SeatLifecycle::Running)
        );
    }
}
