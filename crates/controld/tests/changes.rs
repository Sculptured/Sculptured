//! Proposed changes: an agent calls `propose_change` before changing a business system. Sculptured checks
//! the team's rules itself, approves a change that passes them when the team allows that, and otherwise asks
//! a person; nobody answering in time means not approved. Practice agents through the real seat-tools socket.
mod common;
use axum::http::StatusCode;
use common::m2::{call, get, rid, say, seat, start_team, wait_event};
use model::{EventBody, QuestionSource};
use serde_json::{Value, json};

/// A Practice buyer on a team whose rules allow purchase orders up to $5,000 from two suppliers.
async fn buying_team(s: &common::m2::Served, approve_within: bool) -> (String, String) {
    let (status, rec) = call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "blank", "practice": true, "name": "Buying" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([{ "id": "buyer", "group_id": null, "name": "Buyer", "kind": "practice", "job": "", "folder": null, "x": 40, "y": 40 }]);
    draft["mandate"] = json!({ "approve_within": approve_within, "rules": [
        { "id": "limit", "action": "create_purchase_order", "field": "amount", "test": { "kind": "at-most", "value": 5000 } },
        { "id": "vendor", "action": "create_purchase_order", "field": "supplier", "test": { "kind": "one-of", "values": ["Acme Hydraulics", "Bolt & Co"] } },
    ] });
    let id = draft["id"].as_str().unwrap().to_string();
    let put = json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft });
    let (status, saved) = call(&s.router, "PUT", &format!("/api/teams/{id}"), put).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    start_team(s, &id).await;
    let buyer = seat(&id, "buyer");
    (id, buyer)
}

fn order(amount: u32, supplier: &str) -> String {
    let p = json!({ "action": "create_purchase_order", "system": "ERP", "summary": format!("Order 2 SP-118 bearings from {supplier}, ${amount}"),
        "payload": { "amount": amount, "supplier": supplier, "item": "SP-118", "qty": 2 }, "reason": "the press is down" });
    format!("propose_change:{p}")
}

async fn reply(s: &common::m2::Served, buyer: &str, words: &str) -> String {
    let e = wait_event(
        &s.st,
        buyer,
        words,
        |b| matches!(b, EventBody::AgentText { text, .. } if text.contains(words)),
    )
    .await;
    let EventBody::AgentText { text, .. } = e.body else {
        unreachable!()
    };
    text
}

async fn changes(s: &common::m2::Served, id: &str) -> Value {
    let (status, v) = get(&s.router, &format!("/api/teams/{id}/changes")).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_within_the_rules_is_approved_by_them_without_asking_anyone() {
    let (s, _sup, _dir) = common::seat_tools::with_tools().await;
    let (id, buyer) = buying_team(&s, true).await;
    say(&s, &buyer, &order(4200, "Acme Hydraulics")).await;
    let text = reply(&s, &buyer, "Approved by the team's rules").await;
    assert!(text.contains("Carry out exactly this change"), "{text}");
    let v = changes(&s, &id).await;
    let c = &v["changes"][0];
    assert_eq!(
        c["summary"],
        "Order 2 SP-118 bearings from Acme Hydraulics, $4200"
    );
    assert_eq!(c["agent_name"], "Buyer");
    assert_eq!(c["payload"]["item"], "SP-118");
    assert_eq!(c["decision"]["by"], "mandate");
    assert_eq!(c["decision"]["approved"], true);
    assert_eq!(c["checks"].as_array().unwrap().len(), 2);
    assert!(
        c["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["passed"] == true)
    );
    let (_, inbox) = get(&s.router, "/api/inbox").await;
    assert!(
        inbox.as_array().unwrap().is_empty(),
        "nobody was asked: {inbox}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_over_the_limit_waits_for_a_person_who_can_refuse_it() {
    let (s, _sup, _dir) = common::seat_tools::with_tools().await;
    let (id, buyer) = buying_team(&s, true).await;
    say(&s, &buyer, &order(7300, "Acme Hydraulics")).await;
    let e = wait_event(&s.st, &buyer, "an approval", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    })
    .await;
    let EventBody::QuestionRaised {
        question_id,
        source,
        prompt,
        ..
    } = e.body
    else {
        unreachable!()
    };
    assert_eq!(source, QuestionSource::Approval);
    assert_eq!(
        prompt.text,
        "Order 2 SP-118 bearings from Acme Hydraulics, $7300"
    );
    let input = prompt.tool_input.unwrap();
    assert_eq!(input["checks"][0]["words"], "amount is 7300, at most 5000");
    assert_eq!(input["checks"][0]["passed"], false);
    let (_, inbox) = get(&s.router, "/api/inbox").await;
    assert_eq!(inbox[0]["kind"], "approval", "{inbox}");
    assert!(
        changes(&s, &id).await["changes"][0]
            .get("decision")
            .is_none(),
        "still waiting"
    );

    let answer = json!({ "request_id": rid(), "answer": { "kind": "deny", "reason": "too expensive, find another supplier" } });
    let (status, v) = call(
        &s.router,
        "POST",
        &format!("/api/questions/{question_id}/answer"),
        answer,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let text = reply(&s, &buyer, "Not approved").await;
    assert!(
        text.contains("Not approved: too expensive, find another supplier"),
        "{text}"
    );
    assert!(text.contains("Do not make this change"), "{text}");
    let c = &changes(&s, &id).await["changes"][0];
    assert_eq!(
        c["decision"],
        json!({ "approved": false, "by": "person", "note": "too expensive, find another supplier", "at_ms": c["decision"]["at_ms"] })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_can_approve_and_a_team_without_approve_within_always_asks() {
    let (s, _sup, _dir) = common::seat_tools::with_tools().await;
    let (id, buyer) = buying_team(&s, false).await;
    say(&s, &buyer, &order(100, "Bolt & Co")).await;
    let e = wait_event(&s.st, &buyer, "an approval", |b| {
        matches!(b, EventBody::QuestionRaised { .. })
    })
    .await;
    let EventBody::QuestionRaised { question_id, .. } = e.body else {
        unreachable!()
    };
    let answer = json!({ "request_id": rid(), "answer": { "kind": "allow" } });
    let (status, v) = call(
        &s.router,
        "POST",
        &format!("/api/questions/{question_id}/answer"),
        answer,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    reply(&s, &buyer, "Approved by a person").await;
    assert_eq!(
        changes(&s, &id).await["changes"][0]["decision"]["by"],
        "person"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_approval_nobody_gives_in_time_is_not_approved() {
    let (s, _sup, _dir) = common::seat_tools::with_tools().await;
    s.st.store
        .lock()
        .unwrap()
        .set_question_timeout_override(Some(300))
        .unwrap();
    let (id, buyer) = buying_team(&s, true).await;
    say(&s, &buyer, &order(9000, "Cheap Parts Ltd")).await;
    let text = reply(&s, &buyer, "Not approved: nobody answered in time").await;
    assert!(
        text.contains(
            "✗ supplier is \\\"Cheap Parts Ltd\\\", not one of Acme Hydraulics, Bolt & Co"
        ),
        "{text}"
    );
    assert_eq!(
        changes(&s, &id).await["changes"][0]["decision"]["by"],
        "nobody-in-time"
    );
    let (_, qs) = get(&s.router, &format!("/api/seats/{buyer}/questions")).await;
    assert_eq!(qs[0]["decision"]["source"], "rejected-on-timeout", "{qs}");
}
