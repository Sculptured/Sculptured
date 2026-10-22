mod common;

use common::m2::rid;
use serde_json::json;
#[tokio::test]
async fn project_git_is_scoped_and_commits_only_reviewed_index() {
    let s = crate::common::m2::serve().await;
    let folder = s.home.path().join("project");
    std::fs::create_dir(&folder).unwrap();
    let mut d = rigspec::templates::template("blank", "git-test", "Git test", false).unwrap();
    d.folder = Some(folder.to_string_lossy().into());
    s.st.store.lock().unwrap().insert_draft(&d).unwrap();
    let route = "/api/teams/git-test/git";
    let action = format!("{route}/action");
    let (status, b) = crate::common::m2::call(&s.router, "GET", route, json!(null)).await;
    assert!(status.is_success());
    assert_eq!(b["repository"], false);
    let (status, b) = crate::common::m2::call(
        &s.router,
        "POST",
        &action,
        json!({"request_id":rid(),"action":"init","fingerprint":""}),
    )
    .await;
    assert!(status.is_success(), "{b}");
    assert_eq!(b["repository"], true);
    for (key, value) in [
        ("user.name", "Fixture"),
        ("user.email", "fixture@example.test"),
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&folder)
                .args(["config", key, value])
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(folder.join("notes.txt"), "first\n").unwrap();
    let (_, b) = crate::common::m2::call(&s.router, "GET", route, json!(null)).await;
    let fp = b["fingerprint"].clone();
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &action,
        json!({"request_id":rid(),"action":"stage","paths":["../outside"],"fingerprint":fp}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let (status, b) = crate::common::m2::call(
        &s.router,
        "POST",
        &action,
        json!({"request_id":rid(),"action":"stage","paths":["notes.txt"],"fingerprint":fp}),
    )
    .await;
    assert!(status.is_success(), "{b}");
    let staged = b["fingerprint"].clone();
    let (status, diff) = crate::common::m2::call(
        &s.router,
        "GET",
        "/api/teams/git-test/git/diff?path=notes.txt&staged=true",
        json!(null),
    )
    .await;
    assert!(status.is_success());
    assert!(diff["text"].as_str().unwrap().contains("+first"));
    std::fs::write(folder.join("notes.txt"), "second\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .current_dir(&folder)
            .args(["add", "notes.txt"])
            .status()
            .unwrap()
            .success()
    );
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &action,
        json!({"request_id":rid(),"action":"commit","message":"stale commit","fingerprint":staged}),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::CONFLICT);
    let (_, b) = crate::common::m2::call(&s.router, "GET", route, json!(null)).await;
    let(status,b)=crate::common::m2::call(&s.router,"POST",&action,json!({"request_id":rid(),"action":"commit","message":"Reviewed fixture","fingerprint":b["fingerprint"]})).await;
    assert!(status.is_success(), "{b}");
    assert!(!b["head"].as_str().unwrap().is_empty());
    assert!(b["log"].as_str().unwrap().contains("Reviewed fixture"));
    let(status,b)=crate::common::m2::call(&s.router,"POST",&action,json!({"request_id":rid(),"action":"branch","message":"feature/demo","fingerprint":b["fingerprint"]})).await;
    assert!(status.is_success(), "{b}");
    assert_eq!(b["branch"], "feature/demo");
    let (status, _) = crate::common::m2::call(
        &s.router,
        "GET",
        "/api/teams/git-test/git/diff?path=../outside",
        json!(null),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    let outside = tempfile::tempdir().unwrap();
    d.folder = Some(outside.path().to_string_lossy().into());
    d.revision = 1;
    s.st.store.lock().unwrap().save_draft(&d, 0).unwrap();
    assert!(
        !crate::common::m2::call(&s.router, "GET", route, json!(null))
            .await
            .0
            .is_success()
    );
}
