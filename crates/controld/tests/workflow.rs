mod common;
use axum::http::StatusCode;

use model::TaskStatus;
use serde_json::json;

/// What every live agent of the textbook team is told: a tiny, plain reply, so turns stay cheap.
const PLAIN_JOB: &str =
    "Whatever you are sent, reply with the single word OK and nothing else. Do not use any tools.";

/// The textbook team with every agent live (`live::make_live`) and told `PLAIN_JOB`, started.
async fn live_textbook(s: &crate::common::m2::Served) -> String {
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": crate::common::m2::rid(), "template": "textbook" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    crate::common::live::make_live(s, &mut draft);
    for a in draft["agents"].as_array_mut().unwrap() {
        a["job"] = json!(PLAIN_JOB);
    }
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": crate::common::m2::rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    crate::common::m2::start_team(s, &id).await;
    id
}

/// A dispatched task reaches its agent on the real CLI; the workflow records how its turn ended. When
/// the agent replied plainly (no result) the task is blocked and a retry assigns it again. After a
/// stop the workflow is over and a retry is refused.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_host_delivery_plain_reply_blocking_retry_and_stop_are_observed() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _host = crate::common::live::connect(&s).await;
    let id = live_textbook(&s).await;
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
    let (code, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/textbook/prepare"),
        json!({"revision":rec["draft"]["revision"]}),
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "team_running");
    let dispatcher = crate::common::m2::seat(&id, "dispatcher");
    let reference = format!("webgpu-engine.engine-creator@{id}");
    let args = json!({"batch_id":"one", "summary":"Offline fixture", "tasks":[{"id":"curves","title":"Curves","context":"Fixture source section 1"}]});
    let (ok, _, error) =
        crate::common::seat_tools::direct(&s, &dispatcher, "dispatch_work", args).await;
    assert!(ok, "{error:?}");
    crate::common::live::wait_for("the first assignment delivered", || {
        crate::common::m2::messages_by(&s.st, &reference, "workflow").len() == 1
    })
    .await;
    crate::common::live::wait_for("the agent's turn on the task recorded", || {
        s.st.store
            .lock()
            .unwrap()
            .latest_workflow(&id)
            .unwrap()
            .is_some_and(|r| !r.tasks[0].history.is_empty())
    })
    .await;
    let (_, view) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/workflow")).await;
    assert_eq!(view["active"], true);
    let retry = json!({"task":"curves", "start_seq":view["run"]["start_seq"]});
    let blocked =
        s.st.store
            .lock()
            .unwrap()
            .latest_workflow(&id)
            .unwrap()
            .is_some_and(|r| r.tasks[0].status == TaskStatus::Blocked);
    if blocked {
        // A plain reply blocks instead of advancing; a retry assigns the task again.
        let (code, body) = crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/teams/{id}/workflow/retry"),
            retry.clone(),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        crate::common::live::wait_for("second assignment delivered", || {
            crate::common::m2::messages_by(&s.st, &reference, "workflow").len() == 2
        })
        .await;
    } else {
        eprintln!(
            "the agent returned a result, so the task advanced; the retry path was not reached"
        );
    }
    crate::common::m2::stop_team(&s, &id).await;
    let (_, view) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/workflow")).await;
    assert_eq!(view["active"], false);
    let (code, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/workflow/retry"),
        retry,
    )
    .await;
    assert_eq!(code, StatusCode::CONFLICT);
}
