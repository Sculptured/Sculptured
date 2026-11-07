mod common;

use common::m2::rid;

use model::EventBody;
use serde_json::json;

/// A solo team whose agent has `check`, saved with every agent live (`live::make_live`); returns its id.
async fn solo_with_check(s: &common::m2::Served, check: &str) -> String {
    let (_, rec) = crate::common::m2::call(
        &s.router,
        "POST",
        "/api/teams",
        json!({ "request_id": rid(), "template": "solo" }),
    )
    .await;
    let id = rec["draft"]["id"].as_str().unwrap().to_string();
    let mut draft = rec["draft"].clone();
    crate::common::live::make_live(s, &mut draft);
    draft["agents"][0]["advanced"]["check"] = json!(check);
    let (status, body) = crate::common::m2::call(
        &s.router,
        "PUT",
        &format!("/api/teams/{id}"),
        json!({ "request_id": rid(), "revision": draft["revision"], "draft": draft }),
    )
    .await;
    assert_eq!(status.as_u16(), 200, "{body}");
    id
}

fn check_of(s: &common::m2::Served, seat: &str) -> Option<controld::views::SeatCheck> {
    s.st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .unwrap()
        .check
}

#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_finished_turn_runs_the_agents_check_and_the_seat_shows_its_result() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let (s, _sup, _dir) = crate::common::seat_tools::with_tools().await;
    let id = solo_with_check(&s, "test -f marker && echo found marker").await;
    crate::common::m2::start_team(&s, &id).await;
    let seat = crate::common::m2::seat(&id, "claude");
    crate::common::m2::say(&s, &seat, "Reply with the single word OK and nothing else.").await;
    crate::common::live::wait_event(&s.st, &seat, "a finished turn's check", |b| {
        matches!(b, EventBody::CheckRan { .. })
    })
    .await;
    crate::common::live::wait_for("the check's result on the seat", || {
        check_of(&s, &seat).is_some_and(|c| c.passed.is_some())
    })
    .await;
    let check = check_of(&s, &seat).unwrap();
    assert_eq!(check.command, "test -f marker && echo found marker");
    crate::common::m2::stop_team(&s, &id).await;
}

/// "Run check" runs the agent's check in the folder it started in and records the command's own result.
#[tokio::test]
async fn run_check_runs_the_agents_check_in_its_folder_and_the_seat_shows_its_result() {
    let s = crate::common::m2::serve().await;
    let id = solo_with_check(&s, "test -f marker && echo found marker").await;
    // The agent as recorded when it started in its folder (no model runs here).
    let saved = s.st.store.lock().unwrap().draft(&id).unwrap().unwrap();
    let rig = rigspec::from_draft(&saved).unwrap().rig;
    let seat = rig.seats[0].id.clone();
    let cwd = crate::common::live::work_folder(&s);
    {
        let mut store = s.st.store.lock().unwrap();
        store
            .upsert_rig(
                &rig,
                &controld::store::RigSource::Team {
                    id: id.clone(),
                    revision: saved.revision,
                },
                model::Desired::Running,
            )
            .unwrap();
        store.set_seat_cwd(&seat, &cwd).unwrap();
    }
    let run = || async {
        let (status, _) = crate::common::m2::call(
            &s.router,
            "POST",
            &format!("/api/seats/{seat}/check"),
            json!({}),
        )
        .await;
        assert_eq!(status.as_u16(), 202);
    };
    run().await;
    crate::common::wait_for("a failed check", || {
        check_of(&s, &seat).is_some_and(|c| c.passed == Some(false))
    })
    .await;
    let check = check_of(&s, &seat).unwrap();
    assert_eq!(
        (check.passed, check.exit_code, check.command.as_str()),
        (Some(false), Some(1), "test -f marker && echo found marker")
    );

    std::fs::write(std::path::Path::new(&cwd).join("marker"), "").unwrap();
    run().await;
    crate::common::wait_for("a passing check", || {
        check_of(&s, &seat).is_some_and(|c| c.passed == Some(true))
    })
    .await;
    assert_eq!(check_of(&s, &seat).unwrap().output, "found marker");

    let other = format!("{seat}x");
    let (status, _) = crate::common::m2::call(
        &s.router,
        "POST",
        &format!("/api/seats/{other}/check"),
        json!({}),
    )
    .await;
    assert_eq!(status.as_u16(), 409, "an agent without a check is refused");
}
