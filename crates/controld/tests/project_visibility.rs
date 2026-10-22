mod common;

use common::m2::{Served, rid};

use serde_json::json;
#[tokio::test]
async fn hide_trash_restore_are_persistent_and_do_not_remove_drafts() {
    let s = crate::common::m2::serve().await;
    let (status, created) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({"request_id":rid(),"template":"blank","name":"Visibility test"}),
    )
    .await;
    assert_eq!(status, 200);
    let id = created["draft"]["id"].as_str().unwrap();
    let endpoint = format!("/api/project-visibility/{id}");
    for state in ["hidden", "trashed"] {
        let (status, result) =
            crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":state})).await;
        assert_eq!(status, 200, "{result}");
        let (_, rows) =
            crate::common::m2::call(&s.router, "GET", "/api/project-visibility", json!({})).await;
        assert_eq!(rows[0]["status"], state);
        assert!(s.st.store.lock().unwrap().draft(id).unwrap().is_some());
    }
    let (status,result)=crate::common::m2::call(&s.router,"PUT",&format!("/api/teams/{id}"),json!({"request_id":rid(),"revision":created["draft"]["revision"],"draft":created["draft"]})).await;
    assert_eq!(status, 409);
    assert_eq!(result["error"]["code"], "project_trashed");
    let (status, result) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({"request_id":rid(),"revision":created["draft"]["revision"]}),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(result["error"]["code"], "project_trashed");
    assert_eq!(
        crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":"visible"}))
            .await
            .0,
        200
    );
    assert_eq!(
        crate::common::m2::call(&s.router, "GET", "/api/project-visibility", json!({}))
            .await
            .1,
        json!([])
    );
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "PUT",
            "/api/project-visibility/missing",
            json!({"status":"hidden"})
        )
        .await
        .0,
        404
    );
    assert_eq!(
        crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":"invalid"}))
            .await
            .0,
        422
    );
}
#[tokio::test]
async fn running_agents_can_be_hidden_but_cannot_be_trashed_or_stopped_implicitly() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let endpoint = format!("/api/project-visibility/{id}");
    assert_eq!(
        crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":"hidden"}))
            .await
            .0,
        200
    );
    let (status, result) =
        crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":"trashed"})).await;
    assert_eq!(status, 409, "{result}");
    assert_eq!(result["error"]["code"], "project_running");
    assert!(
        s.st.store
            .lock()
            .unwrap()
            .rig_snapshot(&id)
            .unwrap()
            .unwrap()
            .seats
            .iter()
            .any(|a| controld::store::is_live(a.lifecycle))
    );
    let claim = s.st.try_begin_boot(&id).unwrap();
    assert_eq!(
        crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":"visible"}))
            .await
            .0,
        409
    );
    drop(claim);
    assert_eq!(
        crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":"visible"}))
            .await
            .0,
        200
    );
}

/// Rows `team` still has in the tables that hold a team's records.
fn rows_of(s: &Served, team: &str) -> Vec<(&'static str, i64)> {
    let store = s.st.store.lock().unwrap();
    let suffix = format!("@{team}");
    let count = |sql: &str| -> i64 {
        store
            .connection()
            .query_row(sql, rusqlite::params![team, suffix], |r| r.get(0))
            .unwrap()
    };
    vec![
        (
            "drafts",
            count("SELECT COUNT(*) FROM drafts WHERE id=?1 AND ?2 IS NOT NULL"),
        ),
        (
            "rigs",
            count("SELECT COUNT(*) FROM rigs WHERE name=?1 AND ?2 IS NOT NULL"),
        ),
        (
            "seats",
            count("SELECT COUNT(*) FROM seats WHERE rig=?1 AND ?2 IS NOT NULL"),
        ),
        (
            "events",
            count(
                "SELECT COUNT(*) FROM events WHERE subject='rig:'||?1 OR (substr(subject,1,5)='seat:' AND substr(subject,-length(?2))=?2)",
            ),
        ),
        (
            "messages",
            count(
                "SELECT COUNT(*) FROM messages WHERE substr(seat,-length(?2))=?2 AND ?1 IS NOT NULL",
            ),
        ),
        (
            "deployment_drafts",
            count("SELECT COUNT(*) FROM deployment_drafts WHERE rig=?1 AND ?2 IS NOT NULL"),
        ),
        (
            "project_visibility",
            count("SELECT COUNT(*) FROM project_visibility WHERE id=?1 AND ?2 IS NOT NULL"),
        ),
    ]
}

#[tokio::test]
async fn a_trashed_project_can_be_deleted_forever_and_nothing_else_is_touched() {
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let other = crate::common::m2::practice_team(&s, "solo").await;
    crate::common::m2::stop_team(&s, &id).await;
    let endpoint = format!("/api/project-visibility/{id}");
    let (status, result) = crate::common::m2::call(&s.router, "DELETE", &endpoint, json!({})).await;
    assert_eq!(
        (status.as_u16(), result["error"]["code"].as_str()),
        (409, Some("not_in_trash")),
        "only from the Trash: {result}"
    );
    assert_eq!(
        crate::common::m2::call(&s.router, "PUT", &endpoint, json!({"status":"trashed"}))
            .await
            .0,
        200
    );
    assert!(rows_of(&s, &id).iter().any(|(_, n)| *n > 0));
    let before_other = rows_of(&s, &other);
    let (status, result) = crate::common::m2::call(&s.router, "DELETE", &endpoint, json!({})).await;
    assert_eq!(status, 200, "{result}");
    assert!(
        rows_of(&s, &id).iter().all(|(_, n)| *n == 0),
        "{:?}",
        rows_of(&s, &id)
    );
    assert_eq!(
        rows_of(&s, &other),
        before_other,
        "another project is untouched"
    );
    assert_eq!(
        crate::common::m2::call(&s.router, "GET", &format!("/api/teams/{id}"), json!({}))
            .await
            .0,
        404
    );
}
