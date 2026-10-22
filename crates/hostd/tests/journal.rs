use hostd::Journal;

use model::{Actor, EventBody, Provenance, Subject};

fn append(j: &mut Journal, n: u32) -> u64 {
    j.append(
        n as i64,
        Actor::System("hostd".into()),
        Subject::Host("local".into()),
        Provenance::ProtocolObserved,
        EventBody::RigBooted {
            spec_hash: format!("h{n}"),
            seat_count: n,
        },
    )
    .unwrap()
    .host_seq
}

fn seqs(j: &Journal, after: u64) -> Vec<u64> {
    j.unacked_after(after)
        .unwrap()
        .iter()
        .map(|e| e.host_seq)
        .collect()
}

#[test]
fn assigns_increasing_seq_and_acks() {
    let mut j = Journal::open_in_memory(100).unwrap();
    assert_eq!(
        [append(&mut j, 1), append(&mut j, 2), append(&mut j, 3)],
        [1, 2, 3]
    );
    j.ack(2).unwrap();
    assert_eq!(seqs(&j, 0), [3]);
    assert_eq!(j.last_seq(), 3);
}

#[test]
fn unacked_after_filters_and_preserves_bodies() {
    let mut j = Journal::open_in_memory(100).unwrap();
    for n in 1..=3 {
        append(&mut j, n);
    }
    let evs = j.unacked_after(1).unwrap();
    assert_eq!(evs.iter().map(|e| e.host_seq).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(
        evs[0].body,
        EventBody::RigBooted {
            spec_hash: "h2".into(),
            seat_count: 2
        }
    );
}

#[test]
fn corrupt_event_is_returned_as_a_database_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hostd.sqlite");
    let mut journal = Journal::open(&path, 100).unwrap();
    append(&mut journal, 1);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE events SET json = 'invalid json'", [])
        .unwrap();

    assert!(matches!(
        journal.unacked_after(0),
        Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            _
        ))
    ));
}

#[test]
fn bounded_journal_keeps_only_the_newest() {
    let mut j = Journal::open_in_memory(2).unwrap();
    for n in 1..=5 {
        append(&mut j, n);
    }
    assert_eq!(seqs(&j, 0), [4, 5]);
    assert_eq!(j.last_seq(), 5);
}

#[test]
fn next_seq_and_seats_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hostd.sqlite");
    {
        let mut j = Journal::open(&path, 100).unwrap();
        append(&mut j, 1);
        append(&mut j, 2);
        j.ack(2).unwrap();
        j.record_seat("dev.a@r", "stub", Some("native-1"), 2)
            .unwrap();
    }
    let mut j = Journal::open(&path, 100).unwrap();
    assert_eq!(
        append(&mut j, 3),
        3,
        "seq must not restart after acks and reopen"
    );
    assert_eq!(j.seat_tenure("dev.a@r").unwrap(), 2);
    assert_eq!(j.seat_tenure("dev.unknown@r").unwrap(), 0);
}

#[test]
fn fast_forward_only_moves_next_seq_forward() {
    let mut j = Journal::open_in_memory(100).unwrap();
    append(&mut j, 1);
    j.fast_forward(51).unwrap();
    assert_eq!(append(&mut j, 2), 51);
    j.fast_forward(10).unwrap();
    assert_eq!(append(&mut j, 3), 52, "never moves backward");
}
