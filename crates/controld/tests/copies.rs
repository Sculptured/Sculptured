mod common;

use common::m2::rid;

use serde_json::json;
use std::path::Path;
use std::process::Command;

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=T", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::test]
async fn an_agent_works_in_its_own_copy_that_merges_back_and_can_be_removed() {
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
    crate::common::m2::stop_team(&s, &id).await;
    let proj = s.home.path().join("proj");
    std::fs::create_dir_all(proj.join("src")).unwrap();
    sh(&proj, &["init", "-q", "-b", "main"]);
    std::fs::write(proj.join("src/a.txt"), "a").unwrap();
    sh(&proj, &["add", "-A"]);
    sh(&proj, &["commit", "-qm", "first"]);

    let none = controld::copies::prepare(&s.st, &id, &seat, s.home.path()).unwrap_err();
    assert!(none.contains("not in a git repository"), "{none}");

    let (status, saved) = crate::common::m2::call(&s.router, "PUT", &format!("/api/teams/{id}/settings"), json!({"request_id": rid(), "settings": {"posture": null, "question_timeout_min": null, "own_copies": true}})).await;
    assert_eq!(status.as_u16(), 200, "{saved}");
    let (_, rec) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}")).await;
    let mut draft = rec["draft"].clone();
    draft["folder"] = json!(proj.display().to_string());
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({"request_id": rid(), "revision": draft["revision"], "draft": draft}),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{body}");

    let at = controld::copies::prepare(&s.st, &id, &seat, &proj.join("src")).unwrap();
    assert!(
        at.ends_with("src") && at.join("a.txt").exists() && !at.starts_with(&proj),
        "{}",
        at.display()
    );
    assert_eq!(
        controld::copies::prepare(&s.st, &id, &seat, &proj.join("src")).unwrap(),
        at,
        "reused on relaunch"
    );
    std::fs::write(at.join("b.txt"), "from the agent").unwrap();
    assert!(
        !proj.join("src/b.txt").exists(),
        "the project folder is untouched"
    );

    let (_, copies) = crate::common::m2::get(&s.router, &format!("/api/teams/{id}/copies")).await;
    assert_eq!(
        (copies["enabled"].as_bool(), copies["into"].as_str()),
        (Some(true), Some("main"))
    );
    assert_eq!(copies["copies"][0]["uncommitted"], 1, "{copies}");

    let (status, merged) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/copies/merge"),
        json!({"request_id": rid(), "seat": seat}),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{merged}");
    assert_eq!(
        std::fs::read_to_string(proj.join("src/b.txt")).unwrap(),
        "from the agent"
    );
    assert_eq!(
        (
            merged["copies"][0]["ahead"].as_u64(),
            merged["copies"][0]["uncommitted"].as_u64()
        ),
        (Some(0), Some(0))
    );

    std::fs::write(at.join("c.txt"), "unsaved").unwrap();
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/copies/remove"),
        json!({"request_id": rid(), "seat": seat}),
    )
    .await;
    assert_eq!(status.as_u16(), 409, "uncommitted work is never removed");
    std::fs::remove_file(at.join("c.txt")).unwrap();
    let branch = merged["copies"][0]["branch"].as_str().unwrap().to_string();
    let (status, removed) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/copies/remove"),
        json!({"request_id": rid(), "seat": seat}),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{removed}");
    assert_eq!(removed["copies"].as_array().unwrap().len(), 0);
    assert!(!at.exists());
    assert_eq!(
        sh(&proj, &["branch", "--list", &branch]).trim_start_matches(['*', ' ']),
        branch,
        "the branch is kept"
    );
}

/// `SCULPTURED_TEST_COPIES_DIR` points this at a real drive (e.g. an exFAT SSD); by default a temp folder.
#[tokio::test]
async fn copies_go_to_the_chosen_folder_and_a_missing_drive_is_refused() {
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
    crate::common::m2::stop_team(&s, &id).await;
    let proj = s.home.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    sh(&proj, &["init", "-q", "-b", "main"]);
    std::fs::write(proj.join("a.txt"), "a").unwrap();
    std::os::unix::fs::symlink("a.txt", proj.join("link")).unwrap();
    sh(&proj, &["add", "-A"]);
    sh(&proj, &["commit", "-qm", "first"]);
    let set = |folder: &str| {
        let mut st = s.st.store.lock().unwrap();
        let mut g = st.settings().unwrap();
        g.copies_folder = Some(folder.into());
        st.save_settings(&g).unwrap();
    };

    set("/nonexistent/drive/copies");
    let why = controld::copies::prepare(&s.st, &id, &seat, &proj).unwrap_err();
    assert!(why.contains("not available"), "{why}");

    let base = std::env::var("SCULPTURED_TEST_COPIES_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| s.home.path().join("drive"));
    let chosen = base.join(format!("t{}", std::process::id()));
    std::fs::create_dir_all(&chosen).unwrap();
    set(chosen.to_str().unwrap());
    let at = controld::copies::prepare(&s.st, &id, &seat, &proj).unwrap();
    assert!(at.starts_with(&chosen), "{}", at.display());
    assert_eq!(
        sh(&at, &["status", "--porcelain"]),
        "",
        "a fresh copy reports nothing changed"
    );
    std::fs::write(at.join("a.txt"), "b").unwrap();
    assert_eq!(sh(&at, &["status", "--porcelain"]), "M a.txt");
    sh(
        &proj,
        &["worktree", "remove", "--force", at.to_str().unwrap()],
    );
    let _ = std::fs::remove_dir_all(&chosen);
}
