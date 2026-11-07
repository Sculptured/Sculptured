//! Result checks as a gate between agents: a buyer's result must carry a JSON block whose price is at
//! most 5000 and whose part number is filled in before its work reaches the approver. A failing result is
//! not handed on; the failures go back to the buyer (up to three times), then it is stuck until a
//! passing result comes. Live: the agents run on the real Claude Code CLI, told exactly what to reply.
mod common;
use axum::http::StatusCode;
use common::m2::{call, chain, get, messages_by, rid, say, seat, start_team};
use model::{EventBody, EventRecord};
use serde_json::json;

/// A reply the buyer is told to give, word for word, to every message.
fn block(price: &str) -> String {
    format!("```json\n{{\"part_number\": \"SP-118\", \"price\": {price}}}\n```")
}

/// The buyer's job: always reply with `block(price)`, whatever it is told.
fn job(price: u32) -> String {
    format!(
        "To this message and to every later message, whatever it says, reply with exactly the following and nothing else:\n{}",
        block(&price.to_string())
    )
}

/// Buyer hands work to Approver; the buyer's result is checked. Started.
async fn buying(s: &common::m2::Served, price: u32) -> String {
    let id = chain(
        s,
        "Buying",
        &[
            ("buyer", "Buyer", &job(price)),
            ("approver", "Approver", "Reply with one short sentence."),
        ],
        &[("buyer", "approver")],
    )
    .await;
    let (_, rec) = get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft["agents"][0]["advanced"]["result_checks"] = json!([
        { "id": "part", "field": "part_number", "test": { "kind": "present" } },
        { "id": "price", "field": "price", "test": { "kind": "at-most", "value": 5000 } },
    ]);
    let put = json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft });
    let (status, saved) = call(&s.router, "PUT", &format!("/api/teams/{id}"), put).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    start_team(s, &id).await;
    id
}

fn checks(s: &common::m2::Served, seat: &str) -> Vec<EventRecord> {
    common::m2::events(&s.st, seat)
        .into_iter()
        .filter(|e| matches!(e.body, EventBody::ResultChecked { .. }))
        .collect()
}

fn stuck(s: &common::m2::Served, seat: &str) -> Option<String> {
    s.st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .and_then(|v| v.handoff_stuck)
        .map(|h| h.reason)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_failing_result_goes_back_three_times_then_waits_and_a_passing_one_is_handed_on() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = common::m2::serve().await;
    let _sup = common::live::connect(&s).await;
    let id = buying(&s, 7300).await;
    let (buyer, approver) = (seat(&id, "buyer"), seat(&id, "approver"));
    common::live::wait_for("four failed checks", || checks(&s, &buyer).len() == 4).await;
    let all = checks(&s, &buyer);
    let EventBody::ResultChecked {
        passed,
        results,
        bounced,
        ..
    } = &all[0].body
    else {
        unreachable!()
    };
    assert!(!passed);
    assert!(*bounced);
    assert!(
        results[1].words.ends_with("at most 5000"),
        "{}",
        results[1].words
    );
    let bounced: Vec<bool> = all
        .iter()
        .map(|e| matches!(e.body, EventBody::ResultChecked { bounced: true, .. }))
        .collect();
    assert_eq!(bounced, [true, true, true, false], "sent back three times");
    let back = messages_by(&s.st, &buyer, "result-check");
    assert_eq!(back.len(), 3);
    assert!(
        back[0].2.contains("✗ price is") && back[0].2.contains("`price`: a number at most 5000"),
        "{}",
        back[0].2
    );
    common::live::wait_for("stuck", || stuck(&s, &buyer).is_some()).await;
    assert_eq!(
        stuck(&s, &buyer).unwrap(),
        "Buyer's work did not finish (its result did not pass its checks)"
    );
    assert!(
        messages_by(&s.st, &approver, "handoff").is_empty(),
        "nothing reached the approver"
    );

    let cheaper = format!(
        "A cheaper one was found. From now on reply with exactly the following and nothing else:\n{}",
        block("\"$4,200\"")
    );
    say(&s, &buyer, &cheaper).await;
    common::live::wait_for("handed on", || {
        !messages_by(&s.st, &approver, "handoff").is_empty()
    })
    .await;
    let last = checks(&s, &buyer).pop().unwrap();
    assert!(matches!(
        last.body,
        EventBody::ResultChecked {
            passed: true,
            bounced: false,
            ..
        }
    ));
    assert!(
        messages_by(&s.st, &approver, "handoff")[0]
            .2
            .contains("--- Buyer's result ---")
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_passing_result_is_recorded_and_handed_on() {
    let Some(_) = common::live::require() else {
        return;
    };
    let s = common::m2::serve().await;
    let _sup = common::live::connect(&s).await;
    let id = buying(&s, 4200).await;
    let (buyer, approver) = (seat(&id, "buyer"), seat(&id, "approver"));
    common::live::wait_for("handed on", || {
        !messages_by(&s.st, &approver, "handoff").is_empty()
    })
    .await;
    let all = checks(&s, &buyer);
    assert_eq!(all.len(), 1);
    assert!(matches!(
        all[0].body,
        EventBody::ResultChecked { passed: true, .. }
    ));
    assert!(messages_by(&s.st, &buyer, "result-check").is_empty());
}
