//! The computer running a team's agents drops off mid-run (Wi-Fi change, sleep, a server deploy) and
//! comes back. Nothing the agents were doing is lost: their calls wait, answers wait, and Stop is
//! carried out on reconnect. Live: the agents run on the real Claude Code CLI on haiku.
mod common;
#[path = "common/live_controls.rs"]
mod live_controls;

use common::m2::Served;
use live_controls::Runner;

use model::{Answer, EventBody, SeatLifecycle};
use serde_json::json;
use std::time::Duration;

/// A started team of one live agent with no job; returns (team, its seat).
async fn one_agent(s: &Served) -> (String, String) {
    let team = common::pausing::team(s, "solo", &["lead"]).await;
    crate::common::m2::start_team(s, &team).await;
    let agent = crate::common::m2::seat(&team, "lead");
    (team, agent)
}

fn questions_raised(s: &Served, seat: &str) -> usize {
    crate::common::m2::events(&s.st, seat)
        .iter()
        .filter(|e| matches!(e.body, EventBody::QuestionRaised { .. }))
        .count()
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_question_answered_while_the_computer_is_offline_reaches_the_agent_when_it_returns()
{
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let mut runner = Runner::with_tools(&s).await;
    let (_, agent) = one_agent(&s).await;
    crate::common::m2::say(
        &s,
        &agent,
        "Use your request_input tool (Sculptured's) once, with the question exactly: Which colour? Then reply with the answer you got and nothing else.",
    )
    .await;
    let raised = crate::common::live::wait_event(&s.st, &agent, "a question", |b| {
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
    let answered = |b: &EventBody| matches!(b, EventBody::ToolResulted { output, .. } if output.contains("Green"));
    assert!(
        !crate::common::m2::events(&s.st, &agent)
            .iter()
            .any(|e| answered(&e.body)),
        "the agent is still waiting"
    );

    runner.reconnect(&s).await;
    crate::common::live::wait_event(&s.st, &agent, "the answer reaches the agent", answered).await;
    assert_eq!(
        questions_raised(&s, &agent),
        1,
        "the call sent again is answered once, not asked twice"
    );
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_stopping_a_team_while_its_computer_is_offline_works_and_happens_when_it_returns() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let mut runner = Runner::connect(&s).await;
    let (team, agent) = one_agent(&s).await;
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
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_team_started_during_a_brief_drop_launches_once_the_computer_is_back() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let mut runner = Runner::connect(&s).await;
    let team = common::pausing::team(&s, "Pair", &["a", "b"]).await;
    runner.drop_link(&s).await;
    let back = {
        let s_url = s.url.clone();
        let sup = runner.sup.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let _ = hostd::run_link(&s_url, crate::common::m2::TOKEN, &sup).await;
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
