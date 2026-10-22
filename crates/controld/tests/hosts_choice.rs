mod common;

use common::m2::{TOKEN, rid, seat};
use model::SeatLifecycle;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn a_team_runs_on_its_chosen_computer() {
    let s = crate::common::m2::serve().await;
    let _local = crate::common::m2::connect_stub(&s).await;
    let _laptop =
        crate::common::m2::connect_as(&s, "dev-laptop", vec![Arc::new(drivers::stub::StubDriver)])
            .await;
    let id = crate::common::m2::practice_chain(&s, "chain", &[("a", "A", "Job")], &[]).await;
    let (status, _) = crate::common::m2::call(&s.router, "PUT", &format!("/api/teams/{id}/settings"), json!({"request_id": rid(), "settings": {"posture": null, "question_timeout_min": null, "own_copies": false, "host": "dev-laptop"}})).await;
    assert_eq!(status.as_u16(), 200);
    crate::common::m2::start_team(&s, &id).await;
    let seat = seat(&id, "a");
    crate::common::wait_for("running", || {
        crate::common::lifecycle(&s.st, &seat) == Some(SeatLifecycle::Running)
    })
    .await;
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&seat)
            .unwrap()
            .unwrap()
            .host
            .as_deref(),
        Some("dev-laptop")
    );
    let (_, hosts) = crate::common::m2::get(&s.router, "/api/hosts").await;
    let ids: Vec<&str> = hosts
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["dev-laptop", "local"]);
}

#[tokio::test]
async fn a_team_whose_computer_is_offline_does_not_start_and_says_which() {
    let s = crate::common::m2::serve().await;
    let _local = crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_chain(&s, "chain", &[("a", "A", "Job")], &[]).await;
    crate::common::m2::call(&s.router, "PUT", &format!("/api/teams/{id}/settings"), json!({"request_id": rid(), "settings": {"posture": null, "question_timeout_min": null, "own_copies": false, "host": "dev-gone"}})).await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({"request_id": rid()}),
    )
    .await;
    assert_eq!(status.as_u16(), 409, "{body}");
    assert_eq!(body["error"]["code"], "needs_computer");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("dev-gone")
    );
}

#[test]
fn hosted_with_one_computer_uses_it_and_passes_folders_through_unresolved() {
    use controld::AppState;
    use controld::auth::AuthPolicy;
    use controld::store::Store;
    let st = AppState::new(Store::open_in_memory().unwrap(), "t".into(), vec![]).with_auth(
        AuthPolicy::Gateway {
            secret: Arc::new(b"s".to_vec()),
        },
    );
    assert_eq!(
        controld::launch::team_host(&st, "team"),
        "local",
        "no computer connected yet"
    );
    st.hosts.test_mark_connected("dev-1");
    assert_eq!(controld::launch::team_host(&st, "team"), "dev-1");
    assert_eq!(
        controld::launch::cwd_for(&st, std::path::Path::new("/srv/home"), "."),
        ".",
        "hosted: the runner resolves it on the user's computer"
    );
    let local = AppState::new(Store::open_in_memory().unwrap(), "t".into(), vec![]);
    assert_eq!(
        controld::launch::cwd_for(&local, std::path::Path::new("/srv/home"), "."),
        "/srv/home/."
    );
}

#[tokio::test]
async fn each_computer_reports_its_folders_access_and_capabilities() {
    let s = crate::common::m2::serve().await;
    let home = tempfile::tempdir().unwrap();
    let proj = home.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let mut pol = hostd::policy::RunnerPolicy::default();
    let allowed = pol.allow(&proj).unwrap();
    let cx = localops::Context {
        home: home.path().into(),
        copies_root: home.path().join("copies"),
        claude: "/no/claude".into(),
        codex: "/no/codex".into(),
        codex_home: home.path().join(".codex"),
    };
    let sup = hostd::Supervisor::new(
        "dev-1".into(),
        vec![Arc::new(drivers::stub::StubDriver)],
        hostd::Journal::open_in_memory(1000).unwrap(),
    )
    .restricted(pol)
    .with_ops(hostd::ops::OpsHost { context: cx });
    let (url, link) = (s.url.clone(), sup.clone());
    tokio::spawn(async move {
        let _ = hostd::run_link(&url, TOKEN, &link).await;
    });
    crate::common::wait_for("connected", || s.st.hosts.is_connected("dev-1")).await;
    let (_, hosts) = crate::common::m2::get(&s.router, "/api/hosts").await;
    let p = &hosts[0]["policy"];
    assert_eq!(p["allowed_folders"], json!([allowed.display().to_string()]));
    assert_eq!(p["access"], "full");
    assert!(
        p["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c == "git")
    );
}

fn settings_with(agent_hosts: serde_json::Value) -> serde_json::Value {
    json!({"request_id": rid(), "settings": {"posture": null, "question_timeout_min": null, "own_copies": false, "host": null, "agent_hosts": agent_hosts}})
}

#[tokio::test]
async fn an_agent_runs_on_its_own_computer_and_the_rest_on_the_teams() {
    let s = crate::common::m2::serve().await;
    let _local = crate::common::m2::connect_stub(&s).await;
    let _cluster =
        crate::common::m2::connect_as(&s, "hpc-login", vec![Arc::new(drivers::stub::StubDriver)])
            .await;
    let id = crate::common::m2::practice_chain(
        &s,
        "split",
        &[("a", "A", "Plan"), ("b", "B", "Run the jobs")],
        &[],
    )
    .await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}/settings"),
        settings_with(json!({"b": "hpc-login"})),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{body}");
    assert_eq!(
        crate::common::m2::get(&s.router, &format!("/api/teams/{id}/settings"))
            .await
            .1["agent_hosts"],
        json!({"b": "hpc-login"})
    );
    crate::common::m2::start_team(&s, &id).await;
    let (a, b) = (seat(&id, "a"), seat(&id, "b"));
    crate::common::wait_for("both running", || {
        crate::common::lifecycle(&s.st, &a) == Some(SeatLifecycle::Running)
            && crate::common::lifecycle(&s.st, &b) == Some(SeatLifecycle::Running)
    })
    .await;
    let host = |seat: &str| s.st.store.lock().unwrap().seat(seat).unwrap().unwrap().host;
    assert_eq!(
        (host(&a).as_deref(), host(&b).as_deref()),
        (Some("local"), Some("hpc-login"))
    );
    assert_eq!(
        controld::launch::seat_host(&s.st, &b).unwrap(),
        "hpc-login",
        "messages and stops for b go to its computer"
    );
}

#[tokio::test]
async fn an_agent_whose_computer_is_offline_stops_the_start_and_is_named() {
    let s = crate::common::m2::serve().await;
    let _local = crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_chain(
        &s,
        "split",
        &[("a", "A", "Plan"), ("b", "Cluster runner", "Run the jobs")],
        &[],
    )
    .await;
    crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}/settings"),
        settings_with(json!({"b": "hpc-gone"})),
    )
    .await;
    let (status, body) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        json!({"request_id": rid()}),
    )
    .await;
    assert_eq!(status.as_u16(), 409, "{body}");
    assert_eq!(body["error"]["code"], "needs_computer");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("hpc-gone") && message.contains("Cluster runner"),
        "{message}"
    );
}

#[tokio::test]
async fn hosted_an_agent_without_a_folder_works_in_its_computers_own_folder() {
    let s = crate::common::m2::serve_hosted().await;
    let (status, rec) = crate::common::m2::signed(
        &s.router,
        "POST",
        "/api/teams",
        Some(json!({"request_id": rid(), "template": "blank", "name": "sites"})),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{rec}");
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([{"id": "a", "group_id": null, "name": "Delta", "kind": "claude", "job": "Plan", "folder": null, "x": 40, "y": 40}]);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, saved) = crate::common::m2::signed(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        Some(json!({"request_id": rid(), "revision": draft["revision"], "draft": draft})),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{saved}");
    let codes: Vec<&str> = saved["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["code"].as_str())
        .collect();
    assert!(
        !codes.contains(&"folder-missing"),
        "hosted: the runner uses its computer's first allowed folder: {codes:?}"
    );
}

#[tokio::test]
async fn hosted_an_agent_on_its_own_computer_starts_in_its_own_folder_not_the_teams() {
    let s = crate::common::m2::serve_hosted().await;
    let _laptop =
        crate::common::m2::connect_as(&s, "laptop", vec![Arc::new(drivers::stub::StubDriver)])
            .await;
    let _hpc =
        crate::common::m2::connect_as(&s, "hpc-login", vec![Arc::new(drivers::stub::StubDriver)])
            .await;
    let (_, rec) = crate::common::m2::signed(
        &s.router,
        "POST",
        "/api/teams",
        Some(json!({"request_id": rid(), "template": "blank", "practice": true, "name": "sites"})),
    )
    .await;
    let mut draft = rec["draft"].clone();
    draft["folder"] = json!("/home/me/project");
    let agent = |id: &str, folder: Option<&str>| json!({"id": id, "group_id": null, "name": id, "kind": "practice", "job": "Work", "folder": folder, "x": 40, "y": 40});
    draft["agents"] = json!([
        agent("a", None),
        agent("b", None),
        agent("c", Some("/scratch/me/run"))
    ]);
    let id = draft["id"].as_str().unwrap().to_string();
    let (status, saved) = crate::common::m2::signed(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        Some(json!({"request_id": rid(), "revision": draft["revision"], "draft": draft})),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{saved}");
    let settings = json!({"request_id": rid(), "settings": {"posture": null, "question_timeout_min": null, "own_copies": false, "host": "laptop", "agent_hosts": {"b": "hpc-login", "c": "hpc-login"}}});
    assert_eq!(
        crate::common::m2::signed(
            &s.router,
            "PUT",
            &format!("/api/teams/{id}/settings"),
            Some(settings)
        )
        .await
        .0
        .as_u16(),
        200
    );
    let (status, body) = crate::common::m2::signed(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        Some(json!({"request_id": rid()})),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{body}");
    let snapshot = || {
        s.st.store
            .lock()
            .unwrap()
            .rig_snapshot(&id)
            .unwrap()
            .unwrap()
            .seats
    };
    crate::common::wait_for("all three launched", || {
        snapshot().iter().all(|x| x.cwd.is_some())
    })
    .await;
    let seats = snapshot();
    let cwd = |m: &str| {
        seats
            .iter()
            .find(|x| x.member == m)
            .and_then(|x| x.cwd.clone())
            .unwrap_or_default()
    };
    assert_eq!(
        cwd("a"),
        "/home/me/project",
        "the team's computer uses the project folder"
    );
    assert_eq!(
        cwd("b"),
        ".",
        "another computer: its own folder there, not the project's path"
    );
    assert_eq!(
        cwd("c"),
        "/scratch/me/run",
        "a folder chosen for the agent is kept"
    );
}

#[tokio::test]
async fn hosted_a_team_whose_agents_all_have_their_own_computers_needs_no_team_computer() {
    let s = crate::common::m2::serve_hosted().await;
    let _desk =
        crate::common::m2::connect_as(&s, "desk", vec![Arc::new(drivers::stub::StubDriver)]).await;
    let _hpc =
        crate::common::m2::connect_as(&s, "hpc-login", vec![Arc::new(drivers::stub::StubDriver)])
            .await;
    let (_, rec) = crate::common::m2::signed(
        &s.router,
        "POST",
        "/api/teams",
        Some(json!({"request_id": rid(), "template": "blank", "practice": true, "name": "spread"})),
    )
    .await;
    let mut draft = rec["draft"].clone();
    draft["agents"] = json!([{"id": "a", "group_id": null, "name": "A", "kind": "practice", "job": "Work", "folder": null, "x": 40, "y": 40},
                             {"id": "b", "group_id": null, "name": "B", "kind": "practice", "job": "Work", "folder": null, "x": 300, "y": 40}]);
    let id = draft["id"].as_str().unwrap().to_string();
    crate::common::m2::signed(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        Some(json!({"request_id": rid(), "revision": draft["revision"], "draft": draft})),
    )
    .await;
    let settings = json!({"request_id": rid(), "settings": {"posture": null, "question_timeout_min": null, "own_copies": false, "host": null, "agent_hosts": {"a": "desk", "b": "hpc-login"}}});
    crate::common::m2::signed(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}/settings"),
        Some(settings),
    )
    .await;
    let (_, rec) =
        crate::common::m2::signed(&s.router, "GET", &format!("/api/teams/{id}"), None).await;
    let codes: Vec<&str> = rec["issues"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["code"].as_str())
        .collect();
    assert!(!codes.contains(&"computer-not-connected"), "{codes:?}");
    let (status, body) = crate::common::m2::signed(
        &s.router,
        "POST",
        &format!("/api/teams/{id}/start"),
        Some(json!({"request_id": rid()})),
    )
    .await;
    assert_eq!(
        status.as_u16(),
        200,
        "every agent has its own computer, so no team computer is needed: {body}"
    );
}

#[tokio::test]
async fn a_new_team_runs_on_the_computer_you_chose_last_until_you_choose_otherwise() {
    let s = crate::common::m2::serve().await;
    let _local = crate::common::m2::connect_stub(&s).await;
    let _desk =
        crate::common::m2::connect_as(&s, "desk", vec![Arc::new(drivers::stub::StubDriver)]).await;
    let first = crate::common::m2::practice_chain(&s, "first", &[("a", "A", "Job")], &[]).await;
    let chosen = |host: serde_json::Value| json!({"request_id": rid(), "settings": {"posture": null, "question_timeout_min": null, "own_copies": false, "host": host}});
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "PUT",
            &format!("/api/teams/{first}/settings"),
            chosen(json!("desk"))
        )
        .await
        .0
        .as_u16(),
        200
    );
    let second = crate::common::m2::practice_chain(&s, "second", &[("a", "A", "Job")], &[]).await;
    assert_eq!(
        crate::common::m2::get(&s.router, &format!("/api/teams/{second}/settings"))
            .await
            .1["host"],
        "desk",
        "remembered for a new team"
    );
    assert_eq!(
        crate::common::m2::call(
            &s.router,
            "PUT",
            &format!("/api/teams/{second}/settings"),
            chosen(json!(null))
        )
        .await
        .0
        .as_u16(),
        200
    );
    assert_eq!(
        crate::common::m2::get(&s.router, &format!("/api/teams/{second}/settings"))
            .await
            .1["host"],
        serde_json::Value::Null,
        "Automatic, once chosen, stays"
    );
    assert_eq!(
        crate::common::m2::get(&s.router, &format!("/api/teams/{first}/settings"))
            .await
            .1["host"],
        "desk"
    );
}
