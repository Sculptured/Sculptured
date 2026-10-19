mod common;
use common::{m2::*, wait_for};
use serde_json::json;
use std::sync::Arc;

/// A connected computer allowing `<tmp>/proj`, answering ops; returns (tmp, its proj folder).
async fn computer(s: &Served) -> (tempfile::TempDir, std::path::PathBuf, hostd::Supervisor) {
    let tmp = tempfile::tempdir().unwrap();
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(proj.join("src")).unwrap();
    let mut pol = hostd::policy::RunnerPolicy::default();
    let proj = pol.allow(&proj).unwrap();
    let cx = localops::Context { home: tmp.path().into(), copies_root: tmp.path().join("copies"), claude: "/no/claude".into(), codex: "/no/codex".into(), codex_home: tmp.path().join(".codex") };
    let sup = hostd::Supervisor::new("dev-1".into(), vec![Arc::new(drivers::stub::StubDriver)], hostd::Journal::open_in_memory(1000).unwrap())
        .restricted(pol).with_ops(hostd::ops::OpsHost { context: cx });
    let (url, link) = (s.url.clone(), sup.clone());
    tokio::spawn(async move { let _ = hostd::run_link(&url, TOKEN, &link).await; });
    wait_for("connected", || s.st.hosts.is_connected("dev-1")).await;
    (tmp, proj, sup)
}

#[tokio::test]
async fn hosted_folders_git_and_clis_run_on_the_teams_computer() {
    let s = serve_hosted().await;
    let (status, body) = signed(&s.router, "GET", "/api/folders", None).await;
    assert_eq!((status.as_u16(), body["error"]["code"].as_str()), (409, Some("needs_computer")), "{body}");
    let (tmp, proj, _sup) = computer(&s).await;

    let (status, top) = signed(&s.router, "GET", "/api/folders", None).await;
    assert_eq!(status.as_u16(), 200, "{top}");
    assert_eq!(top["entries"][0]["path"], json!(proj.display().to_string()), "the top is the computer's allowed folders");
    let (status, inside) = signed(&s.router, "GET", &format!("/api/folders?path={}", proj.display()), None).await;
    assert_eq!((status.as_u16(), inside["entries"][0]["name"].as_str()), (200, Some("src")), "{inside}");
    let (status, body) = signed(&s.router, "GET", &format!("/api/folders?path={}", tmp.path().display()), None).await;
    assert_eq!((status.as_u16(), body["error"]["code"].as_str()), (403, Some("not_allowed_here")), "{body}");

    let (_, rec) = signed(&s.router, "POST", "/api/teams", Some(json!({"request_id": "r1", "template": "solo", "practice": true}))).await;
    let mut draft = rec["draft"].clone();
    let id = draft["id"].as_str().unwrap().to_string();
    draft["folder"] = json!(proj.display().to_string());
    let (status, _) = signed(&s.router, "PUT", &format!("/api/teams/{id}"), Some(json!({"request_id": "r2", "revision": draft["revision"], "draft": draft}))).await;
    assert_eq!(status.as_u16(), 200);
    let (status, git) = signed(&s.router, "GET", &format!("/api/teams/{id}/git"), None).await;
    assert_eq!((status.as_u16(), git["repository"].as_bool()), (200, Some(false)), "{git}");
    let (status, git) = signed(&s.router, "POST", &format!("/api/teams/{id}/git/action"), Some(json!({"request_id": "g1", "action": "init", "fingerprint": git["fingerprint"]}))).await;
    assert_eq!((status.as_u16(), git["repository"].as_bool()), (200, Some(true)), "{git}");
    assert!(proj.join(".git").is_dir(), "git ran on the computer, in the team's folder");

    let (status, clis) = signed(&s.router, "GET", "/api/clis", None).await;
    assert_eq!((status.as_u16(), clis.as_array().map(Vec::len)), (200, Some(2)), "{clis}");
    assert_eq!(clis[0]["installed"], false);
    // The code from a sign-in page goes to that computer's sign-in, which here has not started.
    let (status, body) = signed(&s.router, "POST", "/api/clis/claude/login/code", Some(json!({"request_id": "c1", "code": "abc"}))).await;
    assert_eq!((status.as_u16(), body["error"]["code"].as_str()), (409, Some("no_sign_in")), "{body}");

    let (status, _) = signed(&s.router, "POST", "/api/rigs", Some(json!({"request_id": "x", "spec_path": "/etc/passwd"}))).await;
    assert_eq!(status.as_u16(), 409, "spec files by path stay refused in hosted mode");
}

#[tokio::test]
async fn a_hosted_check_runs_on_the_computer_and_reports_pass_or_fail() {
    let s = serve_hosted().await;
    let (_tmp, proj, _sup) = computer(&s).await;
    let (_, rec) = signed(&s.router, "POST", "/api/teams", Some(json!({"request_id": "r1", "template": "solo", "practice": true}))).await;
    let mut draft = rec["draft"].clone();
    let id = draft["id"].as_str().unwrap().to_string();
    draft["folder"] = json!(proj.display().to_string());
    draft["agents"][0]["advanced"]["check"] = json!("test -f ok && echo found");
    let (status, body) = signed(&s.router, "PUT", &format!("/api/teams/{id}"), Some(json!({"request_id": "r2", "revision": draft["revision"], "draft": draft}))).await;
    assert_eq!(status.as_u16(), 200, "{body}");
    let (status, body) = signed(&s.router, "POST", &format!("/api/teams/{id}/start"), Some(json!({"request_id": "r3"}))).await;
    assert_eq!(status.as_u16(), 200, "{body}");
    let seat = s.st.store.lock().unwrap().rig_snapshot(&id).unwrap().unwrap().seats[0].id.clone();
    wait_for("running on the computer", || s.st.store.lock().unwrap().seat(&seat).unwrap().is_some_and(|v| v.cwd.is_some())).await;
    let check = || s.st.store.lock().unwrap().seat(&seat).unwrap().unwrap().check;
    let (status, _) = signed(&s.router, "POST", &format!("/api/seats/{seat}/check"), Some(json!({}))).await;
    assert_eq!(status.as_u16(), 202);
    wait_for("a failed check", || check().is_some_and(|c| c.passed == Some(false))).await;
    std::fs::write(proj.join("ok"), "").unwrap();
    signed(&s.router, "POST", &format!("/api/seats/{seat}/check"), Some(json!({}))).await;
    wait_for("a passing check", || check().is_some_and(|c| c.passed == Some(true))).await;
    assert_eq!(check().unwrap().output, "found", "the command ran in the team's folder on the computer");
}

#[tokio::test]
async fn hosted_teams_with_own_copies_ask_the_computer_for_one_instead_of_making_it_here() {
    let s = serve_hosted().await;
    let (_, rec) = signed(&s.router, "POST", "/api/teams", Some(json!({"request_id": "r1", "template": "solo", "practice": true}))).await;
    let id = rec["draft"]["id"].as_str().unwrap().to_string();
    s.st.store.lock().unwrap().save_team_settings(&id, &controld::views::TeamSettings { own_copies: true, ..Default::default() }).unwrap();
    let draft = s.st.store.lock().unwrap().draft(&id).unwrap().unwrap();
    let seat = draft.seat_id(&draft.agents[0].id).unwrap();
    let own = controld::launch::own_copy_for(&s.st, &id, &seat, rigspec::Runtime::ClaudeCode).expect("asks for an own copy");
    assert_eq!(own.team, id);
    assert!(controld::launch::own_copy_for(&s.st, &id, &seat, rigspec::Runtime::Stub).is_none(), "Practice agents need no copy");
}
