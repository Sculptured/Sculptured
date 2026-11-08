//! Changes proposed by an outside system (`POST /api/teams/{id}/changes`): the same rules, Needs-you
//! question and decisions as an agent's `propose_change`, with the proposer at `external.<name>@<team>`.
//! No agent runs: the outside system is the test calling Sculptured's own API.
mod common;
use axum::http::StatusCode;
use common::m2::{Served, call, get, rid};
use serde_json::{Value, json};

/// A team with no agents; with rules allowing purchase orders up to $5,000 when `rules` is given.
async fn team(s: &Served, rules: Option<bool>) -> String {
    let (status, rec) = call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "name": "Plant" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    let id = draft["id"].as_str().unwrap().to_string();
    if let Some(approve_within) = rules {
        draft["mandate"] = json!({ "approve_within": approve_within, "rules": [
            { "id": "limit", "action": "create_purchase_order", "field": "amount", "test": { "kind": "at-most", "value": 5000 } },
        ] });
        let put = json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft });
        let (status, saved) = call(&s.router, "PUT", &format!("/api/teams/{id}"), put).await;
        assert_eq!(status, StatusCode::OK, "{saved}");
    }
    id
}

fn order(amount: u32) -> Value {
    json!({ "request_id": rid(), "proposer": "arena", "action": "create_purchase_order", "system": "ERP",
        "summary": format!("Order 2 SP-118 bearings, ${amount}"), "payload": { "amount": amount, "item": "SP-118" },
        "reason": "the press is down" })
}

async fn propose(s: &Served, team: &str, body: Value) -> (StatusCode, Value) {
    call(
        &s.router,
        "POST",
        &format!("/api/teams/{team}/changes"),
        body,
    )
    .await
}

async fn change(s: &Served, team: &str, id: &str) -> Value {
    let (status, v) = get(&s.router, &format!("/api/teams/{team}/changes")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("{id} is not listed: {v}"))
}

/// The change once it shows a decision.
async fn decided(s: &Served, team: &str, id: &str) -> Value {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let c = change(s, team, id).await;
        if !c["decision"].is_null() {
            return c;
        }
        assert!(tokio::time::Instant::now() < deadline, "no decision: {c}");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

async fn answer(s: &Served, question: &str, answer: Value) {
    let body = json!({ "request_id": rid(), "answer": answer });
    let (status, v) = call(
        &s.router,
        "POST",
        &format!("/api/questions/{question}/answer"),
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn an_outside_proposal_waits_in_needs_you_and_a_persons_allow_approves_it() {
    let s = common::m2::serve().await;
    let t = team(&s, None).await;
    let (status, v) = propose(&s, &t, order(1200)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let (id, q) = (
        v["proposal_id"].as_str().unwrap(),
        v["question_id"].as_str().unwrap(),
    );
    assert!(v["approved"].is_null(), "no rule decided it: {v}");
    let c = change(&s, &t, id).await;
    assert_eq!(c["agent"], format!("external.arena@{t}"));
    assert_eq!(c["agent_name"], "arena");
    assert_eq!(
        (c["action"].as_str(), c["payload"]["amount"].as_u64()),
        (Some("create_purchase_order"), Some(1200))
    );
    assert!(c["decision"].is_null());
    let (_, qs) = get(&s.router, &format!("/api/teams/{t}/questions")).await;
    let asked = qs
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == q)
        .cloned()
        .unwrap_or_else(|| panic!("{q} is not among the team's questions: {qs}"));
    assert_eq!(
        (asked["source"].as_str(), asked["state"].as_str()),
        (Some("approval"), Some("open"))
    );
    assert_eq!(asked["prompt"]["tool_input"]["proposal_id"], id);
    let (_, inbox) = get(&s.router, "/api/inbox").await;
    assert!(
        inbox.to_string().contains(q),
        "the question is in Needs you: {inbox}"
    );
    answer(&s, q, json!({ "kind": "allow" })).await;
    let c = decided(&s, &t, id).await;
    assert_eq!(
        (
            c["decision"]["approved"].as_bool(),
            c["decision"]["by"].as_str()
        ),
        (Some(true), Some("person"))
    );
}

#[tokio::test]
async fn a_persons_deny_is_not_approved_with_their_note() {
    let s = common::m2::serve().await;
    let t = team(&s, None).await;
    let (_, v) = propose(&s, &t, order(9000)).await;
    let (id, q) = (
        v["proposal_id"].as_str().unwrap(),
        v["question_id"].as_str().unwrap(),
    );
    answer(&s, q, json!({ "kind": "deny", "reason": "too dear" })).await;
    let c = decided(&s, &t, id).await;
    assert_eq!(c["decision"]["approved"], false);
    assert_eq!(c["decision"]["by"], "person");
    assert_eq!(c["decision"]["note"], "too dear");
}

#[tokio::test]
async fn the_teams_rules_approve_a_change_within_them_without_asking() {
    let s = common::m2::serve().await;
    let t = team(&s, Some(true)).await;
    let (status, v) = propose(&s, &t, order(1200)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["approved"], true);
    assert!(v["question_id"].is_null(), "nobody is asked: {v}");
    assert_eq!(v["checks"][0]["passed"], true);
    let c = change(&s, &t, v["proposal_id"].as_str().unwrap()).await;
    assert_eq!(c["decision"]["by"], "mandate");
    // Over the limit: the rules fail, so a person is asked.
    let (_, over) = propose(&s, &t, order(9000)).await;
    assert!(
        over["approved"].is_null() && over["question_id"].is_string(),
        "{over}"
    );
    assert_eq!(over["checks"][0]["passed"], false);
}

#[tokio::test]
async fn an_approval_nobody_gives_in_time_is_not_approved() {
    let s = common::m2::serve().await;
    let t = team(&s, None).await;
    let (_, v) = propose(&s, &t, order(1200)).await;
    let (id, q) = (
        v["proposal_id"].as_str().unwrap(),
        v["question_id"].as_str().unwrap(),
    );
    // The question's deadline passes now: the sweeper expires it as it would after the team's timeout.
    s.st.store
        .lock()
        .unwrap()
        .connection()
        .execute("UPDATE questions SET deadline_ms = 0 WHERE id = ?1", [q])
        .unwrap();
    let c = decided(&s, &t, id).await;
    assert_eq!(c["decision"]["approved"], false);
    assert_eq!(c["decision"]["by"], "nobody-in-time");
}

#[tokio::test]
async fn bad_proposals_are_refused_and_record_nothing() {
    let s = common::m2::serve().await;
    let t = team(&s, None).await;
    let with = |k: &str, v: Value| {
        let mut b = order(1);
        b[k] = v;
        b
    };
    for (body, words) in [
        (with("payload", json!("x")), "payload must be an object"),
        (with("action", json!("a".repeat(81))), "at most 80"),
        (with("proposer", json!("arena:planner")), "proposer must be"),
        (with("proposer", Value::Null), "proposer must be"),
    ] {
        let (status, v) = propose(&s, &t, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["error"]["code"], "bad_proposal");
        assert!(v.to_string().contains(words), "{words}: {v}");
    }
    let (status, v) = propose(&s, &t, with("request_id", Value::Null)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    let (status, v) = propose(&s, "no-such-team", order(1)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{v}");
    let (_, list) = get(&s.router, &format!("/api/teams/{t}/changes")).await;
    assert_eq!(list["changes"], json!([]), "nothing was recorded");
}

#[tokio::test]
async fn the_same_request_proposes_once() {
    let s = common::m2::serve().await;
    let t = team(&s, None).await;
    let body = order(1200);
    let (_, first) = propose(&s, &t, body.clone()).await;
    let (_, again) = propose(&s, &t, body).await;
    assert_eq!(first["proposal_id"], again["proposal_id"]);
    let (_, list) = get(&s.router, &format!("/api/teams/{t}/changes")).await;
    assert_eq!(list["changes"].as_array().unwrap().len(), 1);
}
