use controld::store::{LOCAL_EVENT_HOST, RigSource, Store};
use controld::views::Receipt;
use host_proto::HostEvent;

use model::{
    Actor, Caps, Desired, EventBody, MessageIntent, MessageState, Provenance, SeatActivity,
    SeatLifecycle, Subject, TenureCause, TurnStatus,
};
use serde_json::json;

const SPEC: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: stub, cwd: "." }
    edges: [{ kind: delegates_to, from: a, to: b }]
"#;

const SPEC_A_ONLY: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
"#;

const A: &str = "dev.a@r";
const B: &str = "dev.b@r";

fn store() -> Store {
    let mut s = Store::open_in_memory().unwrap();
    s.upsert_rig(
        &rigspec::compile(SPEC).unwrap(),
        &RigSource::File {
            path: "/specs/r.yaml".into(),
        },
        Desired::Running,
    )
    .unwrap();
    s
}

fn ev(seq: u64, seat: &str, body: EventBody) -> HostEvent {
    HostEvent {
        host_seq: seq,
        at_ms: 1000 + seq as i64,
        actor: Actor::System("hostd".into()),
        subject: Subject::Seat(seat.into()),
        provenance: Provenance::ProtocolObserved,
        body,
    }
}

fn running() -> EventBody {
    EventBody::SeatLifecycleChanged {
        lifecycle: SeatLifecycle::Running,
        reason: None,
    }
}

#[test]
fn new_seats_start_planned_in_declaration_order() {
    let s = store();
    let snap = s.rig_snapshot("r").unwrap().unwrap();
    let ids: Vec<_> = snap.seats.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, [A, B]);
    assert!(
        snap.seats
            .iter()
            .all(|x| x.lifecycle == SeatLifecycle::Planned && x.activity == SeatActivity::Unknown)
    );
    assert!(
        snap.seats
            .iter()
            .all(|x| x.tokens_in.is_none() && x.tokens_out.is_none()),
        "usage is unknown, not 0"
    );
    assert_eq!(snap.desired, Desired::Running);
    assert_eq!(snap.last_seq, 0);
    assert_eq!(snap.rig.name, "r");
    assert!(s.rig_snapshot("nope").unwrap().is_none());
}

#[test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
fn ingest_projects_the_seat_view() {
    let mut s = store();
    s.ingest("local", ev(1, A, running())).unwrap();
    s.ingest(
        "local",
        ev(
            2,
            A,
            EventBody::TenureStarted {
                tenure: 1,
                cause: TenureCause::Fresh,
                driver: "stub".into(),
                native_session_id: Some("n1".into()),
                caps: Caps {
                    interrupt: true,
                    ..Caps::default()
                },
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            3,
            A,
            EventBody::TurnStarted {
                turn_id: "turn-1".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            4,
            A,
            EventBody::ToolCalled {
                turn_id: Some("turn-1".into()),
                call_id: "c1".into(),
                name: "Bash".into(),
                input: json!({ "command": "ls" }),
                parent_call_id: None,
            },
        ),
    )
    .unwrap();
    let items = s
        .ingest(
            "local",
            ev(
                5,
                A,
                EventBody::UsageReported {
                    in_turn: false,
                    turn_id: Some("turn-1".into()),
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    cost: None,
                },
            ),
        )
        .unwrap();
    assert_eq!(items.len(), 1);
    let seat = items[0].seat.clone().expect("seat state after the event");
    assert_eq!(seat.lifecycle, SeatLifecycle::Running);
    assert_eq!(seat.host.as_deref(), Some("local"));
    assert_eq!(seat.tenure, 1);
    assert_eq!(seat.native_session_id.as_deref(), Some("n1"));
    assert!(seat.caps.unwrap().interrupt);
    assert_eq!(seat.current_turn.as_deref(), Some("turn-1"));
    assert_eq!(seat.current_tool.as_deref(), Some("Bash"));
    assert_eq!((seat.tokens_in, seat.tokens_out), (Some(10), Some(5)));
    assert_eq!(seat.last_event_ms, Some(1005));
    assert_eq!(items[0].event.seq, 5);

    s.ingest(
        "local",
        ev(
            6,
            A,
            EventBody::TurnEnded {
                turn_id: "turn-1".into(),
                status: TurnStatus::Completed,
            },
        ),
    )
    .unwrap();
    let seat = s.seat(A).unwrap().unwrap();
    assert_eq!(seat.current_turn, None);
    assert_eq!(seat.current_tool, None);
}

#[test]
fn subagent_tool_calls_do_not_replace_current_tool() {
    let mut s = store();
    let call = |seq, name: &str, parent: Option<&str>| {
        ev(
            seq,
            A,
            EventBody::ToolCalled {
                turn_id: None,
                call_id: format!("c{seq}"),
                name: name.into(),
                input: json!({}),
                parent_call_id: parent.map(str::to_string),
            },
        )
    };
    // The subagent tool is `Agent` on the wire (M0: claude-subagent@3265).
    s.ingest("local", call(1, "Agent", None)).unwrap();
    s.ingest("local", call(2, "Bash", Some("c1"))).unwrap();
    assert_eq!(
        s.seat(A).unwrap().unwrap().current_tool.as_deref(),
        Some("Agent")
    );
}

#[test]
fn duplicate_ingest_is_ignored() {
    let mut s = store();
    assert_eq!(s.ingest("local", ev(1, A, running())).unwrap().len(), 1);
    assert!(s.ingest("local", ev(1, A, running())).unwrap().is_empty());
    assert_eq!(s.host_cursor("local").unwrap(), 1);
}

#[test]
fn gap_records_events_lost_first() {
    let mut s = store();
    s.ingest("local", ev(1, A, running())).unwrap();
    let items = s.ingest("local", ev(5, A, running())).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].event.body, EventBody::EventsLost { count: 3 });
    assert_eq!(items[0].event.host, LOCAL_EVENT_HOST);
    assert_eq!(items[0].event.subject, Subject::Host("local".into()));
    assert_eq!(items[0].event.actor, Actor::System("controld".into()));
    assert!(items[0].seat.is_none());
    assert_eq!(items[1].event.host_seq, 5);
    assert!(items[0].event.seq < items[1].event.seq);
    assert_eq!(s.host_cursor("local").unwrap(), 5);
}

#[test]
fn message_state_only_moves_forward() {
    let mut s = store();
    s.append_local(
        Actor::Human("local".into()),
        Subject::Seat(A.into()),
        Provenance::Operator,
        EventBody::MessageQueued {
            message_id: "m1".into(),
            intent: MessageIntent::Inform,
            text: "hi".into(),
        },
    )
    .unwrap();
    assert_eq!(s.message_state("m1").unwrap(), Some(MessageState::Queued));
    // Consumed can be observed before Delivered (the driver races the command reply).
    s.ingest(
        "local",
        ev(
            1,
            A,
            EventBody::MessageConsumed {
                message_id: "m1".into(),
                turn_id: "turn-1".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            2,
            A,
            EventBody::MessageDelivered {
                message_id: "m1".into(),
            },
        ),
    )
    .unwrap();
    assert_eq!(s.message_state("m1").unwrap(), Some(MessageState::Consumed));
    s.ingest(
        "local",
        ev(
            3,
            A,
            EventBody::MessageFailed {
                message_id: "m1".into(),
                reason: "late".into(),
            },
        ),
    )
    .unwrap();
    assert_eq!(s.message_state("m1").unwrap(), Some(MessageState::Consumed));
    assert_eq!(s.message_state("unknown").unwrap(), None);
}

#[test]
fn local_events_get_their_own_sequence() {
    let mut s = store();
    let a = s
        .append_local(
            Actor::Human("local".into()),
            Subject::Rig("r".into()),
            Provenance::Operator,
            EventBody::RigDesiredChanged {
                desired: Desired::Stopped,
            },
        )
        .unwrap();
    let b = s
        .append_local(
            Actor::Human("local".into()),
            Subject::Rig("r".into()),
            Provenance::Operator,
            EventBody::RigDesiredChanged {
                desired: Desired::Running,
            },
        )
        .unwrap();
    assert_eq!(
        (a.event.host.as_str(), a.event.host_seq),
        (LOCAL_EVENT_HOST, 1)
    );
    assert_eq!(b.event.host_seq, 2);
    assert!(a.seat.is_none());
    let rig_events = s
        .events_for_subject(&Subject::Rig("r".into()), 0, 10)
        .unwrap();
    assert_eq!(rig_events.len(), 2);
}

#[test]
fn events_for_seat_filters_and_pages() {
    let mut s = store();
    for seq in 1..=3 {
        s.ingest("local", ev(seq, A, running())).unwrap();
    }
    s.ingest("local", ev(4, B, running())).unwrap();
    let all = s.events_for_seat(A, 0, 10).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(s.events_for_seat(A, all[0].seq, 10).unwrap().len(), 2);
    assert_eq!(s.events_for_seat(A, 0, 1).unwrap().len(), 1);
    let tail = s.latest_events_for_seat(A, 2).unwrap();
    assert_eq!(
        tail.iter().map(|e| e.host_seq).collect::<Vec<_>>(),
        [2, 3],
        "newest two, oldest first"
    );
}

#[test]
fn upsert_keeps_state_and_drops_removed_seats() {
    let mut s = store();
    s.ingest("local", ev(1, A, running())).unwrap();
    s.upsert_rig(
        &rigspec::compile(SPEC_A_ONLY).unwrap(),
        &RigSource::File {
            path: "/specs/r.yaml".into(),
        },
        Desired::Running,
    )
    .unwrap();
    let snap = s.rig_snapshot("r").unwrap().unwrap();
    assert_eq!(snap.seats.len(), 1);
    assert_eq!(snap.seats[0].lifecycle, SeatLifecycle::Running);
    assert!(s.seat(B).unwrap().is_none());
}

#[test]
fn list_rigs_counts_running_and_attention() {
    let mut s = store();
    s.ingest("local", ev(1, A, running())).unwrap();
    s.ingest(
        "local",
        ev(
            2,
            B,
            EventBody::SeatLifecycleChanged {
                lifecycle: SeatLifecycle::Attention,
                reason: Some("x".into()),
            },
        ),
    )
    .unwrap();
    s.set_desired("r", Desired::Stopped).unwrap();
    let rigs = s.list_rigs().unwrap();
    assert_eq!(rigs.len(), 1);
    assert_eq!(
        (
            rigs[0].seat_count,
            rigs[0].running_count,
            rigs[0].attention_count
        ),
        (2, 1, 1)
    );
    assert_eq!(rigs[0].desired, Desired::Stopped);
    assert_eq!(s.seats_on_host("local").unwrap().len(), 2);
}

#[test]
fn receipts_round_trip() {
    let mut s = store();
    assert!(s.receipt("req-1").unwrap().is_none());
    let r = Receipt {
        fingerprint: "send:{}".into(),
        status: 502,
        body: "{\"error\":{}}".into(),
    };
    s.save_receipt("req-1", &r).unwrap();
    assert_eq!(s.receipt("req-1").unwrap(), Some(r));
}

#[test]
fn store_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    {
        let mut s = Store::open(&path).unwrap();
        s.upsert_rig(
            &rigspec::compile(SPEC).unwrap(),
            &RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
        s.ingest("local", ev(1, A, running())).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.host_cursor("local").unwrap(), 1);
    assert_eq!(
        s.seat(A).unwrap().unwrap().lifecycle,
        SeatLifecycle::Running
    );
}

#[test]
fn tenure_end_and_start_clear_turn_and_tool() {
    let mut s = store();
    s.ingest(
        "local",
        ev(
            1,
            A,
            EventBody::TurnStarted {
                turn_id: "t1".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            2,
            A,
            EventBody::ToolCalled {
                turn_id: Some("t1".into()),
                call_id: "c1".into(),
                name: "Bash".into(),
                input: json!({}),
                parent_call_id: None,
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            3,
            A,
            EventBody::TenureEnded {
                tenure: 1,
                exit_code: None,
                signal: Some(9),
                stderr_tail: String::new(),
            },
        ),
    )
    .unwrap();
    let v = s.seat(A).unwrap().unwrap();
    assert_eq!((v.current_turn, v.current_tool), (None, None));

    s.ingest(
        "local",
        ev(
            4,
            A,
            EventBody::TurnStarted {
                turn_id: "t2".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            5,
            A,
            EventBody::ToolCalled {
                turn_id: Some("t2".into()),
                call_id: "c2".into(),
                name: "Bash".into(),
                input: json!({}),
                parent_call_id: None,
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            6,
            A,
            EventBody::TurnStarted {
                turn_id: "t3".into(),
            },
        ),
    )
    .unwrap();
    let v = s.seat(A).unwrap().unwrap();
    assert_eq!(
        (v.current_turn.as_deref(), v.current_tool),
        (Some("t3"), None),
        "new turn clears the old tool"
    );
    s.ingest(
        "local",
        ev(
            7,
            A,
            EventBody::TenureStarted {
                tenure: 2,
                cause: TenureCause::Fresh,
                driver: "stub".into(),
                native_session_id: None,
                caps: Caps::default(),
            },
        ),
    )
    .unwrap();
    assert_eq!(s.seat(A).unwrap().unwrap().current_turn, None);
}

#[test]
fn bad_stored_enum_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    {
        let mut s = Store::open(&path).unwrap();
        s.upsert_rig(
            &rigspec::compile(SPEC).unwrap(),
            &RigSource::File {
                path: "/specs/r.yaml".into(),
            },
            Desired::Running,
        )
        .unwrap();
    }
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute("UPDATE seats SET lifecycle = 'bogus' WHERE id = ?1", [A])
        .unwrap();
    drop(raw);
    let s = Store::open(&path).unwrap();
    assert!(s.seat(A).is_err());
    assert!(s.rig_snapshot("r").is_err());
}

#[test]
fn usage_sums_across_reports() {
    let mut s = store();
    for (seq, i, o) in [(1, 10, 5), (2, 7, 3)] {
        s.ingest(
            "local",
            ev(
                seq,
                A,
                EventBody::UsageReported {
                    in_turn: false,
                    turn_id: None,
                    input_tokens: i,
                    output_tokens: o,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    cost: None,
                },
            ),
        )
        .unwrap();
    }
    let v = s.seat(A).unwrap().unwrap();
    assert_eq!((v.tokens_in, v.tokens_out), (Some(17), Some(8)));
}

#[test]
fn failed_message_stays_failed() {
    let mut s = store();
    s.append_local(
        Actor::Human("local".into()),
        Subject::Seat(A.into()),
        Provenance::Operator,
        EventBody::MessageQueued {
            message_id: "m1".into(),
            intent: MessageIntent::Inform,
            text: "hi".into(),
        },
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            1,
            A,
            EventBody::MessageFailed {
                message_id: "m1".into(),
                reason: "x".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            2,
            A,
            EventBody::MessageDelivered {
                message_id: "m1".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            3,
            A,
            EventBody::MessageConsumed {
                message_id: "m1".into(),
                turn_id: "t".into(),
            },
        ),
    )
    .unwrap();
    assert_eq!(s.message_state("m1").unwrap(), Some(MessageState::Failed));
}

#[test]
fn activity_projects() {
    let mut s = store();
    s.ingest(
        "local",
        ev(
            1,
            A,
            EventBody::SeatActivityChanged {
                activity: SeatActivity::Working,
            },
        ),
    )
    .unwrap();
    assert_eq!(s.seat(A).unwrap().unwrap().activity, SeatActivity::Working);
}

#[test]
fn local_lifecycle_does_not_overwrite_host() {
    let mut s = store();
    s.ingest("local", ev(1, A, running())).unwrap();
    s.append_local(
        Actor::Human("local".into()),
        Subject::Seat(A.into()),
        Provenance::Operator,
        EventBody::SeatLifecycleChanged {
            lifecycle: SeatLifecycle::Stopping,
            reason: None,
        },
    )
    .unwrap();
    let v = s.seat(A).unwrap().unwrap();
    assert_eq!(v.lifecycle, SeatLifecycle::Stopping);
    assert_eq!(v.host.as_deref(), Some("local"));
}

#[test]
fn ingest_rejects_the_reserved_local_host() {
    let mut s = store();
    assert!(s.ingest(LOCAL_EVENT_HOST, ev(1, A, running())).is_err());
    assert_eq!(s.host_cursor(LOCAL_EVENT_HOST).unwrap(), 0);
}

// ---- final fix wave ----

fn local_lifecycle(s: &mut Store, seat: &str, provenance: Provenance, lifecycle: SeatLifecycle) {
    s.append_local(
        Actor::System("controld".into()),
        Subject::Seat(seat.into()),
        provenance,
        EventBody::SeatLifecycleChanged {
            lifecycle,
            reason: Some("test".into()),
        },
    )
    .unwrap();
}

fn mid_turn(s: &mut Store, seat: &str) {
    s.ingest("local", ev(1, seat, running())).unwrap();
    s.ingest(
        "local",
        ev(
            2,
            seat,
            EventBody::SeatActivityChanged {
                activity: SeatActivity::Working,
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            3,
            seat,
            EventBody::TurnStarted {
                turn_id: "t1".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            4,
            seat,
            EventBody::ToolCalled {
                turn_id: Some("t1".into()),
                call_id: "c1".into(),
                name: "Bash".into(),
                input: json!({}),
                parent_call_id: None,
            },
        ),
    )
    .unwrap();
}

/// I5: an inferred Attention (no TenureEnded) must not leave the old turn, tool and
/// activity looking current.
#[test]
fn attention_clears_turn_tool_and_activity() {
    for l in [
        SeatLifecycle::Attention,
        SeatLifecycle::Failed,
        SeatLifecycle::Stopped,
    ] {
        let mut s = store();
        mid_turn(&mut s, A);
        local_lifecycle(&mut s, A, Provenance::Heuristic, l);
        let v = s.seat(A).unwrap().unwrap();
        assert_eq!(
            (v.current_turn, v.current_tool, v.activity),
            (None, None, SeatActivity::Unknown),
            "{l:?}"
        );
    }
}

/// I5: stale means the host is unreachable and the process may still be alive; the UI
/// shows its turn, tool and activity as "last known", so they are kept.
#[test]
fn stale_keeps_last_known_turn_tool_and_activity() {
    let mut s = store();
    mid_turn(&mut s, A);
    local_lifecycle(&mut s, A, Provenance::Heuristic, SeatLifecycle::Stale);
    let v = s.seat(A).unwrap().unwrap();
    assert_eq!(
        (
            v.current_turn.as_deref(),
            v.current_tool.as_deref(),
            v.activity
        ),
        (Some("t1"), Some("Bash"), SeatActivity::Working)
    );
}

fn queue(s: &mut Store, seat: &str, id: &str) {
    s.append_local(
        Actor::Human("local".into()),
        Subject::Seat(seat.into()),
        Provenance::Operator,
        EventBody::MessageQueued {
            message_id: id.into(),
            intent: MessageIntent::Inform,
            text: "hi".into(),
        },
    )
    .unwrap();
}

/// I6: messages the process never read are failed with a reason when its tenure ends.
#[test]
fn tenure_end_fails_unread_messages_with_a_reason() {
    let mut s = store();
    for id in ["queued", "delivered", "consumed"] {
        queue(&mut s, A, id);
    }
    queue(&mut s, B, "other-seat");
    s.ingest(
        "local",
        ev(
            1,
            A,
            EventBody::MessageDelivered {
                message_id: "delivered".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            2,
            A,
            EventBody::MessageDelivered {
                message_id: "consumed".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            3,
            A,
            EventBody::MessageConsumed {
                message_id: "consumed".into(),
                turn_id: "t".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            4,
            A,
            EventBody::TenureEnded {
                tenure: 1,
                exit_code: Some(1),
                signal: None,
                stderr_tail: String::new(),
            },
        ),
    )
    .unwrap();
    for id in ["queued", "delivered"] {
        assert_eq!(
            s.message_state(id).unwrap(),
            Some(MessageState::Failed),
            "{id}"
        );
        assert_eq!(
            s.message_reason(id).unwrap().as_deref(),
            Some("the agent stopped before reading this message"),
            "{id}"
        );
    }
    assert_eq!(
        s.message_state("consumed").unwrap(),
        Some(MessageState::Consumed)
    );
    assert_eq!(s.message_reason("consumed").unwrap(), None);
    assert_eq!(
        s.message_state("other-seat").unwrap(),
        Some(MessageState::Queued),
        "only that seat's messages"
    );
}

#[test]
fn message_failed_stores_its_reason() {
    let mut s = store();
    queue(&mut s, A, "m1");
    s.ingest(
        "local",
        ev(
            1,
            A,
            EventBody::MessageFailed {
                message_id: "m1".into(),
                reason: "host gone".into(),
            },
        ),
    )
    .unwrap();
    assert_eq!(
        s.message_reason("m1").unwrap().as_deref(),
        Some("host gone")
    );
}

/// M1: the seat view carries the provenance of its latest lifecycle event.
#[test]
fn lifecycle_provenance_follows_the_latest_lifecycle_event() {
    let mut s = store();
    assert_eq!(
        s.seat(A).unwrap().unwrap().lifecycle_provenance,
        None,
        "planned: no lifecycle event yet"
    );
    s.ingest("local", ev(1, A, running())).unwrap();
    assert_eq!(
        s.seat(A).unwrap().unwrap().lifecycle_provenance,
        Some(Provenance::ProtocolObserved)
    );
    local_lifecycle(&mut s, A, Provenance::Heuristic, SeatLifecycle::Attention);
    assert_eq!(
        s.seat(A).unwrap().unwrap().lifecycle_provenance,
        Some(Provenance::Heuristic)
    );
    s.ingest(
        "local",
        ev(
            2,
            A,
            EventBody::SeatActivityChanged {
                activity: SeatActivity::Idle,
            },
        ),
    )
    .unwrap();
    assert_eq!(
        s.seat(A).unwrap().unwrap().lifecycle_provenance,
        Some(Provenance::Heuristic),
        "only lifecycle events set it"
    );
}

/// A store written by the previous schema (no lifecycle_provenance, no message reason)
/// opens and gains the columns.
#[test]
#[allow(clippy::too_many_lines)] // one scenario: its steps and checks read in order
fn opens_a_store_created_before_the_added_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    {
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.execute_batch(
            "CREATE TABLE events (seq INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT NOT NULL, host_seq INTEGER NOT NULL,
                at_ms INTEGER NOT NULL, subject TEXT NOT NULL, json TEXT NOT NULL, UNIQUE (host, host_seq));
             CREATE INDEX events_by_subject ON events (subject, seq);
             CREATE TABLE host_cursor (host TEXT PRIMARY KEY, last_seq INTEGER NOT NULL);
             CREATE TABLE rigs (name TEXT PRIMARY KEY, spec_path TEXT NOT NULL, spec_hash TEXT NOT NULL,
                compiled_json TEXT NOT NULL, desired TEXT NOT NULL);
             CREATE TABLE seats (id TEXT PRIMARY KEY, rig TEXT NOT NULL, ord INTEGER NOT NULL, pod TEXT NOT NULL,
                member TEXT NOT NULL, runtime TEXT NOT NULL, label TEXT, host TEXT, lifecycle TEXT NOT NULL,
                lifecycle_reason TEXT, activity TEXT NOT NULL, tenure INTEGER NOT NULL DEFAULT 0, native_session_id TEXT,
                caps_json TEXT, current_turn TEXT, current_tool TEXT, last_event_ms INTEGER, tokens_in INTEGER, tokens_out INTEGER);
             CREATE TABLE messages (id TEXT PRIMARY KEY, seat TEXT NOT NULL, intent TEXT NOT NULL, text TEXT NOT NULL,
                state TEXT NOT NULL, turn_id TEXT, at_ms INTEGER NOT NULL);
             CREATE TABLE receipts (request_id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, status INTEGER NOT NULL, body TEXT NOT NULL);
             INSERT INTO seats (id, rig, ord, pod, member, runtime, lifecycle, activity)
                VALUES ('dev.a@r', 'r', 0, 'dev', 'a', 'stub', 'running', 'idle');
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.seat(A).unwrap().unwrap().lifecycle_provenance, None);
    queue(&mut s, A, "m1");
    s.ingest(
        "local",
        ev(
            1,
            A,
            EventBody::MessageFailed {
                message_id: "m1".into(),
                reason: "x".into(),
            },
        ),
    )
    .unwrap();
    assert_eq!(s.message_reason("m1").unwrap().as_deref(), Some("x"));
    drop(s);
    Store::open(&path).expect("reopening a migrated store is a no-op");
}

/// Parked from M1: a seat recorded as stopped or failed without a tenure end (the
/// attention "not running" path) fails its unread messages too.
#[test]
fn stopped_or_failed_without_a_tenure_end_fails_unread_messages() {
    let mut s = store();
    queue(&mut s, A, "a1");
    queue(&mut s, B, "b1");
    local_lifecycle(&mut s, A, Provenance::Heuristic, SeatLifecycle::Stopped);
    local_lifecycle(&mut s, B, Provenance::Heuristic, SeatLifecycle::Failed);
    for id in ["a1", "b1"] {
        assert_eq!(
            s.message_state(id).unwrap(),
            Some(MessageState::Failed),
            "{id}"
        );
        assert_eq!(
            s.message_reason(id).unwrap().as_deref(),
            Some("the agent stopped before reading this message"),
            "{id}"
        );
    }
}

/// G21's other side: a seat that may still have a process (stale: host unreachable;
/// stopping: not yet exited) keeps its unread messages queued.
#[test]
fn stale_or_stopping_keeps_unread_messages_queued() {
    let mut s = store();
    queue(&mut s, A, "a1");
    queue(&mut s, B, "b1");
    local_lifecycle(&mut s, A, Provenance::Heuristic, SeatLifecycle::Stale);
    local_lifecycle(
        &mut s,
        B,
        Provenance::ProtocolObserved,
        SeatLifecycle::Stopping,
    );
    for id in ["a1", "b1"] {
        assert_eq!(
            s.message_state(id).unwrap(),
            Some(MessageState::Queued),
            "{id}"
        );
        assert_eq!(s.message_reason(id).unwrap(), None, "{id}");
    }
}

/// G21, Attention: a seat that needs attention may still have its process (a launch
/// that timed out, a host that reported a problem), so its unread messages stay queued.
#[test]
fn attention_keeps_unread_messages_queued() {
    let mut s = store();
    queue(&mut s, A, "a1");
    queue(&mut s, B, "b1");
    s.ingest("local", ev(1, B, running())).unwrap();
    s.ingest(
        "local",
        ev(
            2,
            B,
            EventBody::MessageDelivered {
                message_id: "b1".into(),
            },
        ),
    )
    .unwrap();
    local_lifecycle(&mut s, A, Provenance::Heuristic, SeatLifecycle::Attention);
    local_lifecycle(
        &mut s,
        B,
        Provenance::ProtocolObserved,
        SeatLifecycle::Attention,
    );
    assert_eq!(s.message_state("a1").unwrap(), Some(MessageState::Queued));
    assert_eq!(
        s.message_state("b1").unwrap(),
        Some(MessageState::Delivered)
    );
    for id in ["a1", "b1"] {
        assert_eq!(s.message_reason(id).unwrap(), None, "{id}");
    }
}

/// Silence (an idea from Gas Town's patrol): an agent in a turn with no output for a long time is found,
/// flagged once by a heuristic event, and cleared by its next output; one waiting for a person is not.
#[test]
fn an_agent_silent_in_its_turn_is_flagged_once_and_its_next_output_clears_it() {
    let mut s = store();
    s.ingest("local", ev(1, A, running())).unwrap();
    s.ingest("local", ev(2, B, running())).unwrap();
    s.ingest(
        "local",
        ev(
            3,
            A,
            EventBody::TurnStarted {
                turn_id: "t1".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            4,
            A,
            EventBody::ToolCalled {
                turn_id: None,
                call_id: "c1".into(),
                name: "Bash".into(),
                input: json!({}),
                parent_call_id: None,
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            5,
            B,
            EventBody::TurnStarted {
                turn_id: "t2".into(),
            },
        ),
    )
    .unwrap();
    s.ingest(
        "local",
        ev(
            6,
            B,
            EventBody::ToolCalled {
                turn_id: None,
                call_id: "c2".into(),
                name: "mcp__rig__request_input".into(),
                input: json!({}),
                parent_call_id: None,
            },
        ),
    )
    .unwrap();
    let quiet = 30 * 60_000;
    assert!(
        s.silent_seats(1004 + quiet - 1, quiet).unwrap().is_empty(),
        "not yet"
    );
    let found = s.silent_seats(1006 + 31 * 60_000, quiet).unwrap();
    assert_eq!(
        found.len(),
        1,
        "an agent waiting for a person's answer is not silent: {found:?}"
    );
    assert_eq!(found[0].seat, A);
    assert!(
        found[0].reason().contains("31 minutes") && found[0].reason().contains("Bash"),
        "{}",
        found[0].reason()
    );

    s.append_local(
        Actor::System("controld".into()),
        Subject::Seat(A.into()),
        Provenance::Heuristic,
        EventBody::StallSuspected {
            reason: found[0].reason(),
        },
    )
    .unwrap();
    let stall = s.seat(A).unwrap().unwrap().stalled.expect("flagged");
    assert!(stall.silent, "{stall:?}");
    assert!(
        s.silent_seats(1006 + 90 * 60_000, quiet)
            .unwrap()
            .is_empty(),
        "flagged once"
    );
    s.ingest(
        "local",
        ev(
            7,
            A,
            EventBody::AgentText {
                turn_id: None,
                text: "still building".into(),
            },
        ),
    )
    .unwrap();
    assert!(
        s.seat(A).unwrap().unwrap().stalled.is_none(),
        "its next output clears it"
    );
}

/// Stall flags: the same tool error three times in a row flags the agent (heuristic); a success clears it.
#[test]
fn the_same_tool_error_three_times_in_a_row_flags_a_stall_and_a_success_clears_it() {
    let mut s = store();
    s.ingest("local", ev(1, A, running())).unwrap();
    let result = |seq, is_error, output: &str| {
        ev(
            seq,
            A,
            EventBody::ToolResulted {
                turn_id: None,
                call_id: format!("c{seq}"),
                output: output.into(),
                is_error,
            },
        )
    };
    let stalled = |s: &Store| s.seat(A).unwrap().unwrap().stalled;
    s.ingest(
        "local",
        result(2, true, "error: cannot find module 'x'\nstack"),
    )
    .unwrap();
    s.ingest("local", result(3, true, "error: cannot find module 'x'"))
        .unwrap();
    assert!(stalled(&s).is_none(), "two repeats are not a stall yet");
    s.ingest("local", result(4, true, "error: cannot find module 'x'"))
        .unwrap();
    let stall = stalled(&s).expect("stalled");
    assert!(
        stall
            .reason
            .ends_with("3 times in a row: error: cannot find module 'x'"),
        "{}",
        stall.reason
    );
    s.ingest("local", result(5, false, "ok")).unwrap();
    assert!(stalled(&s).is_none());
}
