mod common;

use model::{
    ActionClass, Actor, Caps, EventBody, EventRecord, Provenance, RiskFlag, Subject, TenureCause,
};

/// Live: a real agent's shell call is interpreted by Sculptured once its result is in, as a heuristic.
#[tokio::test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_shell_call_is_interpreted_after_its_result() {
    let Some(_) = crate::common::live::require() else {
        return;
    };
    let s = crate::common::m2::serve().await;
    let _host = crate::common::live::connect(&s).await;
    let id = crate::common::m2::chain(
        &s,
        "Interpreted",
        &[(
            "a",
            "A",
            "Use your Bash tool to run exactly: echo sculptured-ok. Then reply with the single word OK.",
        )],
        &[],
    )
    .await;
    crate::common::m2::start_team(&s, &id).await;
    let agent = crate::common::m2::seat(&id, "a");
    let e = crate::common::live::wait_event(&s.st, &agent, "the interpretation", |b| {
        matches!(b, EventBody::ActionInterpreted { .. })
    })
    .await;
    assert_eq!(
        (e.actor.clone(), e.provenance),
        (Actor::System("interpreter".into()), Provenance::Heuristic)
    );
    let EventBody::ActionInterpreted { call_id, .. } = &e.body else {
        unreachable!()
    };
    let result = crate::common::m2::events(&s.st, &agent)
        .into_iter()
        .find(|r| matches!(&r.body, EventBody::ToolResulted { call_id: c, .. } if c == call_id))
        .expect("the call's result");
    assert!(result.seq < e.seq, "interpreted after its result");
    let (tool, ok): (String, Option<i64>) =
        s.st.store
            .lock()
            .unwrap()
            .connection()
            .query_row(
                "SELECT tool, ok FROM actions WHERE seat = ?1 AND call_id = ?2",
                [&agent, call_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_eq!(tool, "Bash");
    assert!(ok.is_some(), "the captured result is on the action");
    crate::common::m2::stop_team(&s, &id).await;
}

/// The interpreter reads a call once its result is in: its class and risks, as a heuristic.
#[test]
fn tool_calls_are_interpreted_after_their_results() {
    let mut store = store_with(vec![
        tenure(1),
        bash("toolu_1", "git push --force origin main"),
    ]);
    assert!(interpretations(&store).is_empty(), "not before its result");
    feed(&mut store, 3, result("toolu_1"));
    let e = last_interpretation(&store);
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
    assert_eq!(store.seat("dev.a@r").unwrap().unwrap().risk_count, 1);

    feed(&mut store, 4, bash("toolu_2", "cargo test"));
    feed(&mut store, 5, result("toolu_2"));
    let e = last_interpretation(&store);
    assert!(
        matches!(&e.body, EventBody::ActionInterpreted { class: ActionClass::ShellTestRun, risks, .. } if risks.is_empty())
    );
    let (ok, class): (i64, String) = store
        .connection()
        .query_row(
            "SELECT ok, class_json FROM actions WHERE seat = 'dev.a@r' AND call_id = 'toolu_2'",
            [],
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

    feed(&mut store, 6, bash("toolu_3", "rm -rf /tmp/elsewhere"));
    feed(&mut store, 7, result("toolu_3"));
    let e = last_interpretation(&store);
    assert!(
        matches!(&e.body, EventBody::ActionInterpreted { risks, .. } if risks[0].code == "outside-folder-delete"),
        "{:?}",
        e.body
    );
    assert_eq!(store.seat("dev.a@r").unwrap().unwrap().risk_count, 2);
}

#[test]
fn a_permission_prompts_consequence_uses_the_agents_folder() {
    let store = store_with(vec![]);
    let cwd = store.seat("dev.a@r").unwrap().unwrap().cwd.unwrap();
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

fn last_interpretation(store: &controld::store::Store) -> EventRecord {
    store
        .events_for_seat("dev.a@r", 0, 5000)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.body, EventBody::ActionInterpreted { .. }))
        .last()
        .expect("an interpretation")
}

/// Feeds `body` to `store` as host `h1`'s record number `host_seq` about seat `dev.a@r`.
fn feed(store: &mut controld::store::Store, host_seq: u64, body: EventBody) {
    let ev = host_proto::HostEvent {
        host_seq,
        at_ms: 1_000 + host_seq as i64,
        actor: Actor::Seat("dev.a@r".into()),
        subject: Subject::Seat("dev.a@r".into()),
        provenance: Provenance::ProtocolObserved,
        body,
    };
    store.ingest("h1", ev).unwrap();
}

/// Feeds `bodies` to a store as host `h1` for seat `dev.a@r`, whose folder is `/home/u/project`.
fn store_with(bodies: Vec<EventBody>) -> controld::store::Store {
    let mut store = controld::store::Store::open_in_memory().unwrap();
    store
        .connection()
        .execute(
            "INSERT INTO seats (id, rig, ord, pod, member, runtime, lifecycle, activity)
             VALUES ('dev.a@r', 'r', 0, 'dev', 'a', 'claude-code', 'running', 'idle')",
            [],
        )
        .unwrap();
    store.set_seat_cwd("dev.a@r", "/home/u/project").unwrap();
    for (i, body) in bodies.into_iter().enumerate() {
        feed(&mut store, i as u64 + 1, body);
    }
    store
}

fn tenure(n: u32) -> EventBody {
    EventBody::TenureStarted {
        tenure: n,
        cause: TenureCause::Fresh,
        driver: "claude-stream".into(),
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
