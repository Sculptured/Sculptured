mod common;

use common::m2::Served;

use model::{
    ActionClass, Actor, Caps, EventBody, EventRecord, Provenance, RiskFlag, Subject, TenureCause,
};

async fn interpreted(s: &Served, agent: &str, n: usize) -> EventRecord {
    crate::common::wait_for("the interpretation", || {
        crate::common::m2::events(&s.st, agent)
            .iter()
            .filter(|e| matches!(e.body, EventBody::ActionInterpreted { .. }))
            .count()
            >= n
    })
    .await;
    crate::common::m2::events(&s.st, agent)
        .into_iter()
        .filter(|e| matches!(e.body, EventBody::ActionInterpreted { .. }))
        .nth(n - 1)
        .unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
async fn tool_calls_are_interpreted_after_their_results() {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");

    crate::common::m2::say(&s, &agent, "tool:git push --force origin main").await;
    let e = interpreted(&s, &agent, 1).await;
    assert_eq!(
        (e.actor.clone(), e.provenance),
        (Actor::System("interpreter".into()), Provenance::Heuristic)
    );
    match &e.body {
        EventBody::ActionInterpreted { class, risks, .. } => {
            assert_eq!(*class, ActionClass::ShellGit);
            assert_eq!(
                risks.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(),
                ["force-push"]
            );
            assert_eq!(risks[0].text, "force-pushed to origin main");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .risk_count,
        1
    );

    crate::common::m2::say(&s, &agent, "tool:cargo test").await;
    let e = interpreted(&s, &agent, 2).await;
    assert!(
        matches!(&e.body, EventBody::ActionInterpreted { class: ActionClass::ShellTestRun, risks, .. } if risks.is_empty())
    );
    let (ok, class): (i64, String) = s
        .st
        .store
        .lock()
        .unwrap()
        .connection()
        .query_row(
            "SELECT ok, class_json FROM actions WHERE seat = ?1 AND tool = 'Bash' ORDER BY called_seq DESC LIMIT 1",
            [&agent],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        ok, 1,
        "a captured test run that exited 0 (never 'verified')"
    );
    assert_eq!(
        serde_json::from_str::<ActionClass>(&class).unwrap(),
        ActionClass::ShellTestRun
    );

    crate::common::m2::say(&s, &agent, "tool:rm -rf /tmp/elsewhere").await;
    let e = interpreted(&s, &agent, 3).await;
    assert!(
        matches!(&e.body, EventBody::ActionInterpreted { risks, .. } if risks[0].code == "outside-folder-delete"),
        "{:?}",
        e.body
    );
    assert_eq!(
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .risk_count,
        2
    );
}

#[tokio::test]
async fn a_permission_prompts_consequence_uses_the_agents_folder() {
    let s = crate::common::m2::serve().await;
    crate::common::m2::connect_stub(&s).await;
    let id = crate::common::m2::practice_team(&s, "solo").await;
    let agent = crate::common::m2::seat(&id, "claude");
    let cwd =
        s.st.store
            .lock()
            .unwrap()
            .seat(&agent)
            .unwrap()
            .unwrap()
            .cwd
            .unwrap();
    let flags = controld::interpret::consequence(
        "Bash",
        &serde_json::json!({ "command": "rm -rf ../x" }),
        Some(std::path::Path::new(&cwd)),
    );
    assert_eq!(
        flags.iter().map(|f| f.code.as_str()).collect::<Vec<_>>(),
        ["outside-folder-delete"]
    );
}

/// Feeds `bodies` to a store as host `h1` for seat `dev.a@r`, whose folder is `/home/u/project`.
fn store_with(bodies: Vec<EventBody>) -> controld::store::Store {
    let mut store = controld::store::Store::open_in_memory().unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO seats (id, rig, ord, pod, member, runtime, lifecycle, activity)
             VALUES ('dev.a@r', 'r', 0, 'dev', 'a', 'stub', 'running', 'idle')",
            [],
        )
        .unwrap();
    store.set_seat_cwd("dev.a@r", "/home/u/project").unwrap();
    let indexed_bodies = bodies.into_iter().enumerate();
    for (i, body) in indexed_bodies {
        let ev = host_proto::HostEvent {
            host_seq: i as u64 + 1,
            at_ms: 1_000 + i as i64,
            actor: Actor::Seat("dev.a@r".into()),
            subject: Subject::Seat("dev.a@r".into()),
            provenance: Provenance::ProtocolObserved,
            body,
        };
        store.ingest("h1", ev).unwrap();
    }
    store
}

fn tenure(n: u32) -> EventBody {
    EventBody::TenureStarted {
        tenure: n,
        cause: TenureCause::Fresh,
        driver: "stub".into(),
        native_session_id: None,
        caps: Caps::default(),
    }
}

fn bash(call: &str, cmd: &str) -> EventBody {
    EventBody::ToolCalled {
        turn_id: Some("turn-1".into()),
        call_id: call.into(),
        name: "Bash".into(),
        input: serde_json::json!({ "command": cmd }),
        parent_call_id: None,
    }
}

fn result(call: &str) -> EventBody {
    EventBody::ToolResulted {
        turn_id: Some("turn-1".into()),
        call_id: call.into(),
        output: "done".into(),
        is_error: false,
    }
}

fn interpretations(store: &controld::store::Store) -> Vec<(String, Vec<String>)> {
    store
        .events_for_seat("dev.a@r", 0, 5000)
        .unwrap()
        .into_iter()
        .filter_map(|e| match e.body {
            EventBody::ActionInterpreted { call_id, risks, .. } => {
                Some((call_id, risks.into_iter().map(|r| r.code).collect()))
            }
            _ => None,
        })
        .collect()
}

#[test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
fn a_call_id_used_again_in_a_later_session_is_a_separate_action() {
    let store = store_with(vec![
        tenure(1),
        bash("toolu_1", "git push --force origin main"),
        result("toolu_1"),
        bash("toolu_9", "ls"),
        tenure(2),
        bash("toolu_1", "rm -rf /tmp/x"),
        result("toolu_1"),
        // toolu_9 was called in session 1 only; a result for it in session 2 belongs to no call of this session.
        result("toolu_9"),
    ]);
    let rows: Vec<(i64, Option<i64>, String)> = store
        .connection()
        .prepare("SELECT tenure, ok, risks_json FROM actions WHERE seat = 'dev.a@r' AND call_id = ?1 ORDER BY tenure")
        .unwrap()
        .query_map(["toolu_1"], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let codes = |json: &str| {
        serde_json::from_str::<Vec<RiskFlag>>(json)
            .unwrap()
            .into_iter()
            .map(|r| r.code)
            .collect::<Vec<_>>()
    };
    assert_eq!(rows.len(), 2, "both sessions keep their own row");
    assert_eq!(
        (rows[0].0, rows[0].1, codes(&rows[0].2)),
        (1, Some(1), vec!["force-push".to_string()])
    );
    assert_eq!(
        (rows[1].0, rows[1].1, codes(&rows[1].2)),
        (2, Some(1), vec!["outside-folder-delete".to_string()])
    );
    let (ok, result_seq): (Option<i64>, Option<i64>) = store
        .connection()
        .query_row(
            "SELECT ok, result_seq FROM actions WHERE call_id = 'toolu_9'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (ok, result_seq),
        (None, None),
        "session 1's unfinished call is not touched by session 2"
    );
    assert_eq!(
        interpretations(&store),
        [
            ("toolu_1".to_string(), vec!["force-push".to_string()]),
            (
                "toolu_1".to_string(),
                vec!["outside-folder-delete".to_string()]
            )
        ]
    );
    assert_eq!(store.seat("dev.a@r").unwrap().unwrap().risk_count, 2);
}

#[test]
fn a_repeated_result_is_interpreted_once() {
    let store = store_with(vec![
        tenure(1),
        bash("toolu_1", "sudo ls"),
        result("toolu_1"),
        result("toolu_1"),
    ]);
    assert_eq!(
        interpretations(&store),
        [("toolu_1".to_string(), vec!["sudo".to_string()])]
    );
    assert_eq!(store.seat("dev.a@r").unwrap().unwrap().risk_count, 1);
}
