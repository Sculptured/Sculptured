mod common;

use common::m2::rid;

use model::{Actor, EventBody, SeatLifecycle};
use serde_json::json;

#[tokio::test]
async fn managed_children_messages_and_hard_stop_are_observed_and_idempotent() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let parent = crate::common::m2::seat(&id, "claude");
    let args = json!({"request_id":"child1","name":"Geometry helper","task":"Check sampled curve endpoints"});
    let (ok, result, error) =
        crate::common::seat_tools::direct(&s, &parent, "spawn_agent", args.clone()).await;
    assert!(ok, "{error:?}");
    let child = result["child"].as_str().unwrap().to_string();
    let (ok, replayed, error) =
        crate::common::seat_tools::direct(&s, &parent, "spawn_agent", args).await;
    assert!(ok, "{error:?}");
    assert_eq!(result, replayed);
    let snap =
        s.st.store
            .lock()
            .unwrap()
            .rig_snapshot(&id)
            .unwrap()
            .unwrap();
    assert_eq!(snap.rig.seats.len(), 2);
    assert!(
        snap.rig
            .edges
            .iter()
            .any(|e| e.from == child && e.to == parent)
    );
    assert_eq!(snap.rig.seats[0].cwd, snap.rig.seats[1].cwd);
    assert_eq!(
        snap.rig.seats[0].permission_policy,
        snap.rig.seats[1].permission_policy
    );
    let args =
        json!({"request_id":"msg1","to":parent,"text":"Endpoint check: complete, see result."});
    let (ok, result, error) =
        crate::common::seat_tools::direct(&s, &child, "send_agent_message", args.clone()).await;
    assert!(ok, "{error:?}");
    let (ok, replay, _) =
        crate::common::seat_tools::direct(&s, &child, "send_agent_message", args).await;
    assert!(ok);
    assert_eq!(replay, result);
    assert_eq!(
        crate::common::m2::events(&s.st, &parent)
            .iter()
            .filter(|e| e.actor == Actor::Seat(child.clone())
                && e.provenance == model::Provenance::AgentReported
                && matches!(e.body, EventBody::MessageQueued { .. }))
            .count(),
        1
    );
    let (ok, _, _) = crate::common::seat_tools::direct(
        &s,
        &child,
        "send_agent_message",
        json!({"request_id":"bad","to":"other.project@elsewhere","text":"No"}),
    )
    .await;
    assert!(!ok);
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{parent}/control"),
        json!({"request_id":rid(),"mode":"hard"}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    crate::common::wait_for("parent and child stopped", || {
        crate::common::lifecycle(&s.st, &parent) == Some(SeatLifecycle::Stopped)
            && crate::common::lifecycle(&s.st, &child) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let (ok, _, _) = crate::common::seat_tools::direct(
        &s,
        &parent,
        "spawn_agent",
        json!({"request_id":"no-after-stop","name":"No","task":"No"}),
    )
    .await;
    assert!(!ok);
    controld::reconciler::restart_seat(&s.st, &parent)
        .await
        .unwrap();
    crate::common::wait_for("parent restarted", || {
        crate::common::lifecycle(&s.st, &parent) == Some(SeatLifecycle::Running)
    })
    .await;
    assert!(
        s.st.store
            .lock()
            .unwrap()
            .control(&parent)
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn soft_stop_wraps_up_then_stops_and_archive_is_recoverable() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    crate::common::m2::say(&s, &agent, "hang").await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{agent}/control"),
        json!({"request_id":rid(),"mode":"soft"}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    crate::common::wait_for("wrap-up stopped", || {
        crate::common::lifecycle(&s.st, &agent) == Some(SeatLifecycle::Stopped)
    })
    .await;
    assert!(
        crate::common::m2::events(&s.st, &agent)
            .iter()
            .any(|e| matches!(&e.body,EventBody::MessageQueued{text,..} if text.contains("5,000")))
    );
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/archive"),
        json!({"request_id":rid()}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    assert!(
        crate::common::m2::get(&s.router, "/api/teams")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::common::m2::get(&s.router, "/api/rigs")
            .await
            .1
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!crate::common::m2::events(&s.st, &agent).is_empty());
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({"request_id":rid()}),
    )
    .await;
    assert!(!status.is_success());
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/archive"),
        json!({"request_id":rid(),"archived":false}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    assert_eq!(
        crate::common::m2::get(&s.router, "/api/teams")
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn nested_children_inherit_budget_and_exhaustion_cannot_be_resumed() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let parent = crate::common::m2::seat(&id, "claude");
    // Public budget changes are refused while live.
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/budget"),
        json!({"request_id":rid(),"output_limit":5000}),
    )
    .await;
    assert_eq!(status.as_u16(), 409);
    controld::reconciler::stop_rig(s.st.clone(), id.clone())
        .await
        .unwrap();
    crate::common::wait_for("parent stopped", || {
        crate::common::lifecycle(&s.st, &parent) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/budget"),
        json!({"request_id":rid(),"output_limit":5000}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({"request_id":rid()}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    crate::common::wait_for("parent ready", || {
        crate::common::lifecycle(&s.st, &parent) == Some(SeatLifecycle::Running)
            && !s.st.is_booting(&id)
    })
    .await;
    let (ok, child, error) = crate::common::seat_tools::direct(
        &s,
        &parent,
        "spawn_agent",
        json!({"request_id":"nested-child","name":"Branch","task":"Bounded branch"}),
    )
    .await;
    assert!(ok, "{error:?}");
    let child = child["child"].as_str().unwrap().to_string();
    let (ok, leaf, error) = crate::common::seat_tools::direct(
        &s,
        &child,
        "spawn_agent",
        json!({"request_id":"nested-leaf","name":"Leaf","task":"Bounded leaf"}),
    )
    .await;
    assert!(ok, "{error:?}");
    let leaf = leaf["child"].as_str().unwrap().to_string();
    assert_eq!(
        s.st.store.lock().unwrap().parent(&leaf).unwrap(),
        Some(child.clone())
    );
    let budget = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/budget"))
        .await
        .1;
    assert_eq!(budget["output_limit"], 5000);
    assert_eq!(budget["agents"].as_array().unwrap().len(), 3);
    // Captured usage is cumulative across turns. Only this leaf reaches its budget.
    let used =
        s.st.store
            .lock()
            .unwrap()
            .budget_used(&leaf)
            .unwrap()
            .unwrap_or(0);
    s.st.local_event(
        Actor::System("fixture".into()),
        model::Subject::Seat(leaf.clone()),
        model::Provenance::ProtocolObserved,
        EventBody::UsageReported {
            in_turn: false,
            turn_id: None,
            input_tokens: 0,
            output_tokens: 5000 - used as u64,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: None,
        },
    )
    .unwrap();
    crate::common::wait_for("leaf budget stop", || {
        crate::common::lifecycle(&s.st, &leaf) == Some(SeatLifecycle::Stopped)
    })
    .await;
    assert_eq!(
        crate::common::lifecycle(&s.st, &parent),
        Some(SeatLifecycle::Running)
    );
    assert_eq!(
        crate::common::lifecycle(&s.st, &child),
        Some(SeatLifecycle::Running)
    );
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{leaf}/control"),
        json!({"request_id":rid(),"mode":"resume"}),
    )
    .await;
    assert_eq!(status.as_u16(), 409);
    let (ok, _, _) = crate::common::seat_tools::direct(
        &s,
        &leaf,
        "spawn_agent",
        json!({"request_id":"bypass","name":"No","task":"No"}),
    )
    .await;
    assert!(!ok);
    controld::reconciler::stop_rig(s.st.clone(), id)
        .await
        .unwrap();
}

/// No limits unless the team sets them: one agent spawns more than 64 agents, and a chain goes deeper than 4.
#[tokio::test]
async fn without_team_limits_agents_spawn_past_the_old_caps_and_retries_do_not_count() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let root = crate::common::m2::seat(&id, "claude");
    for i in 0..64 {
        let args = json!({"request_id":format!("wide-{i}"),"name":format!("Child {i}"),"task":"Small bounded task"});
        let (ok, result, error) =
            crate::common::seat_tools::direct(&s, &root, "spawn_agent", args.clone()).await;
        assert!(ok, "child {i}: {error:?}");
        if i == 0 {
            let (ok, replay, error) =
                crate::common::seat_tools::direct(&s, &root, "spawn_agent", args).await;
            assert!(ok, "{error:?}");
            assert_eq!(result, replay);
        }
    }
    assert_eq!(
        s.st.store.lock().unwrap().children(&root).unwrap().len(),
        64
    );
    let mut parent = s.st.store.lock().unwrap().children(&root).unwrap()[0].clone();
    for level in 2..=6 {
        let (ok,child,error)=crate::common::seat_tools::direct(&s,&parent,"spawn_agent",json!({"request_id":format!("deep-{level}"),"name":format!("Level {level}"),"task":"Small bounded task"})).await;
        assert!(ok, "level {level}: {error:?}");
        parent = child["child"].as_str().unwrap().to_string();
    }
    controld::reconciler::stop_rig(s.st.clone(), id)
        .await
        .unwrap();
}

/// A solo Practice team with the team limits `limits`, started.
async fn limited_team(s: &common::m2::Served, limits: serde_json::Value) -> String {
    let (status, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({"request_id":rid(),"template":"solo","practice":true}),
    )
    .await;
    assert!(status.is_success(), "{rec}");
    let mut draft = rec["draft"].clone();
    draft["limits"] = limits;
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({"request_id":rid(),"revision":draft["revision"].clone(),"draft":draft}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    crate::common::m2::start_team(s, &id).await;
    id
}

#[tokio::test]
async fn a_team_agent_limit_refuses_the_spawn_and_names_the_setting() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = limited_team(&s, json!({"max_agents":3})).await;
    let root = crate::common::m2::seat(&id, "claude");
    for i in 0..2 {
        let (ok, _, error) = crate::common::seat_tools::direct(
            &s,
            &root,
            "spawn_agent",
            json!({"request_id":format!("c{i}"),"name":"Helper","task":"Help"}),
        )
        .await;
        assert!(ok, "{error:?}");
    }
    let (ok, _, error) = crate::common::seat_tools::direct(
        &s,
        &root,
        "spawn_agent",
        json!({"request_id":"c2","name":"Helper","task":"Help"}),
    )
    .await;
    assert!(!ok);
    assert_eq!(
        error.as_deref(),
        Some("Team limit you set: 3 agents, children included")
    );
    controld::reconciler::stop_rig(s.st.clone(), id)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_team_level_limit_refuses_a_deeper_child_and_names_the_setting() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = limited_team(&s, json!({"max_child_levels":2})).await;
    let mut parent = crate::common::m2::seat(&id, "claude");
    for level in 1..=2 {
        let (ok, child, error) = crate::common::seat_tools::direct(
            &s,
            &parent,
            "spawn_agent",
            json!({"request_id":format!("l{level}"),"name":"Helper","task":"Help"}),
        )
        .await;
        assert!(ok, "{error:?}");
        parent = child["child"].as_str().unwrap().to_string();
    }
    let (ok, _, error) = crate::common::seat_tools::direct(
        &s,
        &parent,
        "spawn_agent",
        json!({"request_id":"l3","name":"Helper","task":"Help"}),
    )
    .await;
    assert!(!ok);
    assert_eq!(
        error.as_deref(),
        Some("Team limit you set: 2 levels of children")
    );
    controld::reconciler::stop_rig(s.st.clone(), id)
        .await
        .unwrap();
}

#[tokio::test]
async fn deployment_history_keeps_removed_children_and_isolates_run_evidence() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let root = crate::common::m2::seat(&id, "claude");
    let runs = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/deployments"))
        .await
        .1;
    let first = runs[0]["start_seq"].as_i64().unwrap();
    let (ok, result, error) = crate::common::seat_tools::direct(
        &s,
        &root,
        "spawn_agent",
        json!({"request_id":"historical-child","name":"Historical child","task":"old run task"}),
    )
    .await;
    assert!(ok, "{error:?}");
    let child = result["child"].as_str().unwrap().to_string();
    crate::common::m2::say(&s, &root, "first run evidence").await;
    controld::reconciler::stop_rig(s.st.clone(), id.clone())
        .await
        .unwrap();
    crate::common::wait_for("all stopped", || {
        crate::common::lifecycle(&s.st, &root) == Some(SeatLifecycle::Stopped)
            && crate::common::lifecycle(&s.st, &child) == Some(SeatLifecycle::Stopped)
    })
    .await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({"request_id":rid()}),
    )
    .await;
    assert!(status.is_success(), "{body}");
    crate::common::wait_for("restarted", || {
        crate::common::lifecycle(&s.st, &root) == Some(SeatLifecycle::Running)
            && !s.st.is_booting(&id)
    })
    .await;
    assert!(s.st.store.lock().unwrap().seat(&child).unwrap().is_none());
    let runs = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/deployments"))
        .await
        .1;
    assert_eq!(runs.as_array().unwrap().len(), 2);
    let next = runs[0]["start_seq"].as_i64().unwrap();
    let path = format!("/api/teams/{id}/deployments/{first}");
    let (status, history) = crate::common::m2::get(&s.router, &path).await;
    assert!(status.is_success(), "{history}");
    assert_eq!(history["has_snapshot"], true);
    let old = history["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == child)
        .unwrap();
    assert_eq!(old["name"], "Historical child");
    assert_eq!(old["parent"], root);
    assert_eq!(old["lifecycle"], "stopped");
    let evidence = crate::common::m2::get(&s.router, &format!("{path}/events?seat={child}"))
        .await
        .1;
    assert!(!evidence["events"].as_array().unwrap().is_empty());
    assert!(
        evidence["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["seq"].as_i64().unwrap() >= first && e["seq"].as_i64().unwrap() < next)
    );
    assert_eq!(
        crate::common::m2::get(&s.router, &format!("/api/teams/other/deployments/{first}"))
            .await
            .0
            .as_u16(),
        404
    );
    s.st.store
        .lock()
        .unwrap()
        .connection()
        .execute(
            "DELETE FROM deployment_specs WHERE rig=?1 AND start_seq=?2",
            rusqlite::params![id, first],
        )
        .unwrap();
    let recovered = crate::common::m2::get(&s.router, &path).await.1;
    assert_eq!(recovered["has_snapshot"], false);
    assert!(
        recovered["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == child && a["name"] == "Historical child")
    );
    let older = crate::common::m2::get(
        &s.router,
        &format!("/api/teams/{id}/deployments?before={next}"),
    )
    .await
    .1;
    assert_eq!(older.as_array().unwrap().len(), 1);
    controld::reconciler::stop_rig(s.st.clone(), id)
        .await
        .unwrap();
}

/// A per-agent token limit counts the agent and, when it says so, the agents it spawned; reaching it
/// soft-stops the agent (wrap-up, then stop with its subagents) and a stopped agent cannot be resumed.
#[tokio::test]
async fn a_token_limit_including_subagents_soft_stops_the_agent_with_its_children() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let parent = crate::common::m2::seat(&id, "claude");
    crate::common::wait_for("parent ready", || {
        crate::common::lifecycle(&s.st, &parent) == Some(SeatLifecycle::Running)
            && !s.st.is_booting(&id)
    })
    .await;
    let (ok, child, error) = crate::common::seat_tools::direct(
        &s,
        &parent,
        "spawn_agent",
        json!({"request_id":"c","name":"Helper","task":"Help"}),
    )
    .await;
    assert!(ok, "{error:?}");
    let child = child["child"].as_str().unwrap().to_string();
    {
        let mut store = s.st.store.lock().unwrap();
        let mut d = store.draft(&id).unwrap().unwrap();
        let limit = model::TokenLimit {
            soft: None,
            tokens: 1_000_000,
            counts: model::TokenCount::All,
            include_subagents: true,
        };
        d.agents[0].advanced.token_limit = Some(limit);
        let rev = d.revision;
        store.save_draft(&d, rev).unwrap();
    }
    // Only the child's usage crosses the parent's limit (cache reads count with "all").
    s.st.local_event(
        Actor::System("fixture".into()),
        model::Subject::Seat(child.clone()),
        model::Provenance::ProtocolObserved,
        EventBody::UsageReported {
            in_turn: false,
            turn_id: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 1_000_000,
            cache_write_tokens: 0,
            cost: None,
        },
    )
    .unwrap();
    let budget = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/budget"))
        .await
        .1;
    let mine = budget["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["seat"] == parent.as_str())
        .cloned()
        .unwrap();
    assert!(
        mine["token_used"].as_u64().unwrap() >= 1_000_000,
        "{budget}"
    );
    assert_eq!(mine["exhausted"], true, "{budget}");
    crate::common::wait_for("parent and child stopped", || {
        [&parent, &child]
            .iter()
            .all(|x| crate::common::lifecycle(&s.st, x) == Some(SeatLifecycle::Stopped))
    })
    .await;
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{parent}/control"),
        json!({"request_id":rid(),"mode":"resume"}),
    )
    .await;
    assert_eq!(status.as_u16(), 409);
}

#[tokio::test]
async fn team_usage_reports_running_totals_and_the_latest_context_fill() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let seat =
        s.st.store
            .lock()
            .unwrap()
            .rig_snapshot(&id)
            .unwrap()
            .unwrap()
            .seats[0]
            .id
            .clone();
    let put = |body: EventBody| {
        s.st.local_event(
            Actor::System("fixture".into()),
            model::Subject::Seat(seat.clone()),
            model::Provenance::ProtocolObserved,
            body,
        )
        .unwrap();
    };
    put(EventBody::UsageReported {
        in_turn: false,
        turn_id: None,
        input_tokens: 100,
        output_tokens: 10,
        cache_read_tokens: 7,
        cache_write_tokens: 0,
        cost: None,
    });
    put(EventBody::ContextReported {
        turn_id: None,
        used_tokens: 900,
        window_tokens: Some(200_000),
        basis: model::ContextBasis::Reported,
    });
    put(EventBody::UsageReported {
        in_turn: false,
        turn_id: None,
        input_tokens: 50,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cost: None,
    });
    let (status, usage) =
        crate::common::m2::get(&s.router, &format!("/api/teams/{id}/usage")).await;
    assert_eq!(status.as_u16(), 200, "{usage}");
    let a = usage["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["seat"] == seat.as_str())
        .expect("agent")
        .clone();
    let last = a["points"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(
        (
            last["input"].as_u64(),
            last["output"].as_u64(),
            last["cache_read"].as_u64()
        ),
        (Some(150), Some(15), Some(7))
    );
    assert!(a["reports"].as_u64().unwrap() >= 2);
    assert_eq!(
        (
            a["context"]["used_tokens"].as_u64(),
            a["context"]["estimated"].as_bool()
        ),
        (Some(900), Some(false))
    );
    controld::reconciler::stop_rig(s.st.clone(), id)
        .await
        .unwrap();
}

/// The account page's summary: every team's tokens and costs together, by day, by CLI and by team,
/// with costs only where the CLI reported one or a list price applies (the rest counted, not guessed).
#[tokio::test]
async fn account_usage_sums_every_teams_tokens_costs_and_activity() {
    use model::pricing::{Cost, CostSource};
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let (a, b) = (
        crate::common::m2::practice_team(&s, "solo").await,
        crate::common::m2::practice_team(&s, "builder-reviewer").await,
    );
    let seat_of = |id: &str| {
        s.st.store
            .lock()
            .unwrap()
            .rig_snapshot(id)
            .unwrap()
            .unwrap()
            .seats[0]
            .id
            .clone()
    };
    let (sa, sb) = (seat_of(&a), seat_of(&b));
    // The Practice agents' own first turns report tokens too: settle them, then measure what we add.
    let usage = || async {
        crate::common::m2::get(&s.router, "/api/account/usage")
            .await
            .1
    };
    let mut before = usage().await;
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let now = usage().await;
        if now == before {
            break;
        }
        before = now;
    }
    let n = |v: &serde_json::Value, p: &str| v.pointer(p).and_then(|x| x.as_u64()).unwrap_or(0);
    let team_tokens = |v: &serde_json::Value, id: &str| {
        v["top_teams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["team"] == id)
            .map(|t| t["tokens"].as_u64().unwrap())
            .unwrap_or(0)
    };
    let put = |seat: &str, body: EventBody| {
        s.st.local_event(
            Actor::System("fixture".into()),
            model::Subject::Seat(seat.to_string()),
            model::Provenance::ProtocolObserved,
            body,
        )
        .unwrap();
    };
    put(
        &sa,
        EventBody::UsageReported {
            in_turn: false,
            turn_id: None,
            input_tokens: 100,
            output_tokens: 10,
            cache_read_tokens: 1000,
            cache_write_tokens: 50,
            cost: Some(Cost {
                usd: 0.25,
                source: CostSource::ClaudeCode,
            }),
        },
    );
    put(
        &sa,
        EventBody::UsageReported {
            in_turn: false,
            turn_id: None,
            input_tokens: 20,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: None,
        },
    );
    put(
        &sb,
        EventBody::UsageReported {
            in_turn: false,
            turn_id: None,
            input_tokens: 300,
            output_tokens: 30,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost: Some(Cost {
                usd: 1.0,
                source: CostSource::OpenaiListPrices,
            }),
        },
    );
    put(
        &sb,
        EventBody::TurnEnded {
            turn_id: "t1".into(),
            status: model::TurnStatus::Completed,
        },
    );

    let (status, u) = crate::common::m2::get(&s.router, "/api/account/usage").await;
    assert_eq!(status.as_u16(), 200, "{u}");
    for (p, added) in [
        ("/totals/input", 420),
        ("/totals/output", 45),
        ("/totals/cache_read", 1000),
        ("/totals/cache_write", 50),
        ("/totals/reports", 3),
        ("/cost/reports_without_cost", 1),
        ("/activity/turns_completed", 1),
    ] {
        assert_eq!(n(&u, p) - n(&before, p), added, "{p}: {u}");
    }
    let usd = |v: &serde_json::Value| v["cost"]["usd"].as_f64().unwrap();
    assert!((usd(&u) - usd(&before) - 1.25).abs() < 1e-9, "{u}");
    let by_source: std::collections::BTreeMap<String, f64> = u["cost"]["by_source"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["source"].as_str().unwrap().to_string(),
                x["usd"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        by_source,
        [
            ("claude-code".to_string(), 0.25),
            ("openai-list-prices".to_string(), 1.0)
        ]
        .into(),
        "Practice agents report no cost"
    );
    let days = u["by_day"].as_array().unwrap();
    assert_eq!(days.len(), 30, "the last 30 days, empty ones included");
    assert_eq!(
        days[29]["tokens"].as_u64().unwrap() - before["by_day"][29]["tokens"].as_u64().unwrap(),
        1515,
        "today: {}",
        days[29]
    );
    assert_eq!(days[0]["tokens"], 0);
    assert_eq!(team_tokens(&u, &a) - team_tokens(&before, &a), 1185);
    assert_eq!(team_tokens(&u, &b) - team_tokens(&before, &b), 330);
    assert_eq!(u["top_teams"][0]["team"], a.as_str(), "most tokens first");
    assert!(
        u["top_teams"][0]["name"]
            .as_str()
            .is_some_and(|n| !n.is_empty())
    );
    assert!(n(&u, "/activity/teams") >= 2);
}
