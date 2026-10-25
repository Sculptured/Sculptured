//! The team's status page: only its owner writes it with write_status, each write is kept as a
//! version, and the Results view reads the newest page and the earlier ones.
mod common;
use axum::http::StatusCode;
use common::m2::{call, rid, seat};
use serde_json::json;

#[tokio::test]
async fn only_the_owner_writes_the_page_and_every_version_is_kept() {
    let s = common::m2::serve().await;
    let (_, rec) = call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "builder-reviewer", "name": "Status" }),
    )
    .await;
    let id = rec["draft"]["id"].as_str().unwrap().to_string();
    let (status, empty) = call(
        &s.router,
        "GET",
        &format!("/api/teams/{id}/status"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{empty}");
    assert_eq!(empty["page"], json!(null));
    assert_eq!(
        empty["owner"],
        json!({ "id": "reviewer", "name": "Reviewer", "chosen": false })
    );

    let write = |who: &str, args: serde_json::Value| {
        controld::status_page::write_status(&s.st, &seat(&id, who), &args)
    };
    let refused = write("builder", json!({ "markdown": "# Mine" })).unwrap_err();
    assert!(refused.contains("Only Reviewer writes"), "{refused}");
    write(
        "reviewer",
        json!({ "markdown": "# Building\n\nHalf done." }),
    )
    .unwrap();
    let done = write(
        "reviewer",
        json!({ "markdown": "# Done\n\nAll merged.", "done": true }),
    )
    .unwrap();
    assert!(done["text"].as_str().unwrap().contains("final"));
    assert!(write("reviewer", json!({ "markdown": "  " })).is_err());
    let big = "x".repeat(controld::status_page::MAX_BYTES + 1);
    assert!(
        write("reviewer", json!({ "markdown": big }))
            .unwrap_err()
            .contains("at most 64 KB")
    );

    let (_, v) = call(
        &s.router,
        "GET",
        &format!("/api/teams/{id}/status"),
        json!(null),
    )
    .await;
    assert_eq!(v["page"]["markdown"], "# Done\n\nAll merged.");
    assert_eq!(v["page"]["done"], true);
    assert_eq!(v["page"]["author_name"], "Reviewer");
    let versions = v["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert!(
        versions[0].get("markdown").is_none(),
        "the list carries no pages"
    );
    let first = versions[1]["seq"].as_i64().unwrap();
    let (status, old) = call(
        &s.router,
        "GET",
        &format!("/api/teams/{id}/status/{first}"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{old}");
    assert_eq!(old["markdown"], "# Building\n\nHalf done.");
    assert_eq!(old["done"], false);
    let (status, _) = call(
        &s.router,
        "GET",
        &format!("/api/teams/{id}/status/999999"),
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
