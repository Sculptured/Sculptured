mod common;

use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn an_op_sent_to_a_computer_runs_there_and_returns_its_value() {
    let s = crate::common::m2::serve().await;
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join("proj")).unwrap();
    let cx = localops::Context {
        home: home.path().into(),
        copies_root: home.path().join("copies"),
        claude: "/no/claude".into(),
        codex: "/no/codex".into(),
        codex_home: home.path().join(".codex"),
    };
    let _laptop = crate::common::m2::connect_with_ops(
        &s,
        "dev-1",
        vec![Arc::new(drivers::stub::StubDriver)],
        hostd::ops::OpsHost { context: cx },
    )
    .await;
    let v =
        s.st.hosts
            .op(
                "dev-1",
                serde_json::to_value(localops::LocalOp::ListFolder {
                    path: None,
                    files: false,
                })
                .unwrap(),
                Duration::from_secs(5),
            )
            .await
            .unwrap();
    assert_eq!(v["entries"][0]["name"], "proj");
    let e =
        s.st.hosts
            .op(
                "dev-1",
                serde_json::to_value(localops::LocalOp::ListFolder {
                    path: Some("/".into()),
                    files: false,
                })
                .unwrap(),
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
    assert!(
        matches!(e, controld::hosts::OpFailure::Failed(ref f) if f.status == 403 || f.status == 400),
        "{e:?}"
    );
    let e =
        s.st.hosts
            .op(
                "dev-gone",
                serde_json::json!({"op":"clis"}),
                Duration::from_secs(1),
            )
            .await
            .unwrap_err();
    assert!(matches!(e, controld::hosts::OpFailure::NotConnected));
}

#[tokio::test]
async fn an_op_that_takes_too_long_times_out_instead_of_hanging() {
    let s = crate::common::m2::serve().await;
    let home = tempfile::tempdir().unwrap();
    let slow = home.path().join("claude");
    std::fs::write(&slow, "#!/bin/sh\nsleep 30\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cx = localops::Context {
        home: home.path().into(),
        copies_root: home.path().join("copies"),
        claude: slow,
        codex: "/no/codex".into(),
        codex_home: home.path().join(".codex"),
    };
    let _c = crate::common::m2::connect_with_ops(
        &s,
        "dev-1",
        vec![],
        hostd::ops::OpsHost { context: cx },
    )
    .await;
    let started = std::time::Instant::now();
    let e =
        s.st.hosts
            .op(
                "dev-1",
                serde_json::json!({"op":"clis"}),
                Duration::from_millis(500),
            )
            .await
            .unwrap_err();
    assert!(matches!(e, controld::hosts::OpFailure::Timeout));
    assert!(started.elapsed() < Duration::from_secs(3));
}
