//! Result checks as a gate between agents: a buyer's result must carry a JSON block whose price is at
//! most 5000 and whose part number is filled in before its work reaches the approver. A failing result is
//! not handed on; the failures go back to the buyer (up to three times), then it is stuck until a
//! passing result comes. Practice agents echo what they are sent, so the job's JSON is the result's.
mod common;
use axum::http::StatusCode;
use common::m2::{
    call, connect_stub, get, messages_by, practice_chain, rid, say, seat, start_team,
};
use model::{EventBody, EventRecord};
use serde_json::json;

fn job(price: u32) -> String {
    format!("Find the bearing.\n```json\n{{\"part_number\": \"SP-118\", \"price\": {price}}}\n```")
}

/// Buyer hands work to Approver; the buyer's result is checked. Started.
async fn buying(s: &common::m2::Served, price: u32) -> String {
    let id = practice_chain(
        s,
        "Buying",
        &[
            ("buyer", "Buyer", &job(price)),
            ("approver", "Approver", "Approve the purchase."),
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
async fn a_failing_result_goes_back_three_times_then_waits_and_a_passing_one_is_handed_on() {
    let s = common::m2::serve().await;
    let _sup = connect_stub(&s).await;
    let id = buying(&s, 7300).await;
    let (buyer, approver) = (seat(&id, "buyer"), seat(&id, "approver"));
    common::wait_for("four failed checks", || checks(&s, &buyer).len() == 4).await;
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
    assert_eq!(results[1].words, "price is 7300, at most 5000");
    let bounced: Vec<bool> = all
        .iter()
        .map(|e| matches!(e.body, EventBody::ResultChecked { bounced: true, .. }))
        .collect();
    assert_eq!(bounced, [true, true, true, false], "sent back three times");
    let back = messages_by(&s.st, &buyer, "result-check");
    assert_eq!(back.len(), 3);
    assert!(
        back[0].2.contains("✗ price is 7300, at most 5000")
            && back[0].2.contains("`price`: a number at most 5000"),
        "{}",
        back[0].2
    );
    common::wait_for("stuck", || stuck(&s, &buyer).is_some()).await;
    assert_eq!(
        stuck(&s, &buyer).unwrap(),
        "Buyer's work did not finish (its result did not pass its checks)"
    );
    assert!(
        messages_by(&s.st, &approver, "handoff").is_empty(),
        "nothing reached the approver"
    );

    say(
        &s,
        &buyer,
        "Found a cheaper one.\n```json\n{\"part_number\": \"SP-118\", \"price\": \"$4,200\"}\n```",
    )
    .await;
    common::wait_for("handed on", || {
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
            .contains("$4,200")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_passing_result_is_recorded_and_handed_on() {
    let s = common::m2::serve().await;
    let _sup = connect_stub(&s).await;
    let id = buying(&s, 4200).await;
    let (buyer, approver) = (seat(&id, "buyer"), seat(&id, "approver"));
    common::wait_for("handed on", || {
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
