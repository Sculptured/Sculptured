//! C0 (priority briefs): the seat records the model its session asked for and the model its CLI
//! reports (migration 10 adds the columns; a new session clears both).
use controld::store::{RigSource, SCHEMA_VERSION, Store};
use host_proto::HostEvent;

use model::{Actor, Caps, Desired, EventBody, Provenance, Subject, TenureCause};
use rusqlite::Connection;

const SPEC: &str = r#"
version: "0.2"
name: r
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
"#;

const A: &str = "dev.a@r";

/// The migration that adds `seats.model_requested` and `seats.model_observed`.
const MODEL_MIGRATION: i64 = 10;

fn version(path: &std::path::Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn has_column(path: &std::path::Path, column: &str) -> bool {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('seats') WHERE name = ?1",
            [column],
            |r| r.get(0),
        )
        .unwrap()
}

fn add_rig(s: &mut Store) {
    s.upsert_rig(
        &rigspec::compile(SPEC).unwrap(),
        &RigSource::File {
            path: "/specs/r.yaml".into(),
        },
        Desired::Running,
    )
    .unwrap();
}

fn models(s: &Store) -> (Option<String>, Option<String>) {
    let v = s.seat(A).unwrap().unwrap();
    (v.model_requested, v.model_observed)
}

/// A runner record about seat A, as hostd sends it.
fn ev(seq: u64, body: EventBody) -> HostEvent {
    let (actor, provenance) = (Actor::System("hostd".into()), Provenance::ProtocolObserved);
    HostEvent {
        host_seq: seq,
        at_ms: 1000 + seq as i64,
        actor,
        subject: Subject::Seat(A.into()),
        provenance,
        body,
    }
}

fn tenure(seq: u64, n: u32) -> HostEvent {
    let native_session_id = Some(format!("n{n}"));
    let caps = Caps::default();
    ev(
        seq,
        EventBody::TenureStarted {
            tenure: n,
            cause: TenureCause::Fresh,
            driver: "claude".into(),
            native_session_id,
            caps,
        },
    )
}

fn observed(seq: u64, requested: Option<&str>, observed: Option<&str>) -> HostEvent {
    let (requested, observed) = (requested.map(Into::into), observed.map(Into::into));
    ev(
        seq,
        EventBody::ModelObserved {
            requested,
            observed,
            provider: None,
            effort: None,
            rerouted_from: None,
            reason: None,
        },
    )
}

#[test]
fn the_model_migration_adds_the_seat_columns() {
    const { assert!(SCHEMA_VERSION >= MODEL_MIGRATION) };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    drop(Store::open(&path).unwrap());
    assert!(has_column(&path, "model_requested") && has_column(&path, "model_observed"));
    assert_eq!(version(&path), SCHEMA_VERSION);
}

/// A store one migration behind (the version before this one) gains the columns and keeps its seats;
/// a store set back to that version with the columns already present opens again without re-adding them.
#[test]
fn a_store_one_version_behind_gains_the_model_columns_and_reopens_after_a_downgrade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    add_rig(&mut Store::open(&path).unwrap());
    Connection::open(&path)
        .unwrap()
        .execute_batch(&format!(
            "ALTER TABLE seats DROP COLUMN model_requested; ALTER TABLE seats DROP COLUMN model_observed; PRAGMA user_version = {};",
            MODEL_MIGRATION - 1
        ))
        .unwrap();
    assert!(!has_column(&path, "model_requested") && !has_column(&path, "model_observed"));

    let s = Store::open(&path).unwrap();
    let seat = s.seat(A).unwrap().expect("the seat survives the migration");
    assert_eq!(
        (seat.model_requested, seat.model_observed),
        (None, None),
        "unknown, not invented"
    );
    drop(s);
    assert!(has_column(&path, "model_requested") && has_column(&path, "model_observed"));
    assert_eq!(version(&path), SCHEMA_VERSION);

    // An older build set the version back and left the columns in place.
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", MODEL_MIGRATION - 1)
        .unwrap();
    drop(Store::open(&path).expect("reopens although the columns already exist"));
    assert_eq!(version(&path), SCHEMA_VERSION);
}

/// The user's live store is at version 6: every later migration, this one included, applies in one open.
#[test]
fn a_version_6_store_reaches_the_model_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    add_rig(&mut Store::open(&path).unwrap());
    // Remove what migrations 7 and later added, then set the version back to 6.
    let c = Connection::open(&path).unwrap();
    c.execute_batch("DROP INDEX events_by_subject_type; DROP TABLE questions; DROP TABLE actions; DROP TABLE settings;").unwrap();
    for col in [
        "open_questions",
        "risk_count",
        "restore_offer",
        "cwd",
        "limit_hit",
        "model_requested",
        "model_observed",
    ] {
        c.execute_batch(&format!("ALTER TABLE seats DROP COLUMN {col};"))
            .unwrap();
    }
    c.pragma_update(None, "user_version", 6).unwrap();
    drop(c);

    let s = Store::open(&path).expect("a version-6 store opens");
    assert!(s.seat(A).unwrap().is_some());
    drop(s);
    assert!(has_column(&path, "model_requested") && has_column(&path, "model_observed"));
    assert_eq!(version(&path), SCHEMA_VERSION);
}

/// The seat shows what was asked for and what the CLI reports.
#[test]
fn the_seat_projects_the_requested_and_observed_model() {
    let mut s = Store::open_in_memory().unwrap();
    add_rig(&mut s);
    assert_eq!(models(&s), (None, None), "planned: unknown");
    s.ingest("local", tenure(1, 1)).unwrap();
    s.ingest("local", observed(2, Some("opus"), None)).unwrap();
    assert_eq!(
        models(&s),
        (Some("opus".into()), None),
        "requested, the CLI has not said yet"
    );
    s.ingest("local", observed(3, Some("opus"), Some("claude-opus-5-5")))
        .unwrap();
    assert_eq!(
        models(&s),
        (Some("opus".into()), Some("claude-opus-5-5".into()))
    );
    // A later record without an observed model keeps the one the CLI already reported.
    s.ingest("local", observed(4, Some("opus"), None)).unwrap();
    assert_eq!(
        models(&s),
        (Some("opus".into()), Some("claude-opus-5-5".into()))
    );
    // The CLI's own default (nothing requested) is recorded as None, not as a made-up name.
    s.ingest("local", observed(5, None, Some("claude-opus-5-5")))
        .unwrap();
    assert_eq!(models(&s), (None, Some("claude-opus-5-5".into())));
}

/// A new session asks again, so both clear; a seat view saved before the fields existed still loads.
#[test]
fn a_new_session_clears_the_seat_model() {
    let mut s = Store::open_in_memory().unwrap();
    add_rig(&mut s);
    s.ingest("local", tenure(1, 1)).unwrap();
    s.ingest(
        "local",
        observed(2, Some("sonnet"), Some("claude-sonnet-5-5")),
    )
    .unwrap();
    s.ingest("local", tenure(3, 2)).unwrap();
    assert_eq!(models(&s), (None, None), "a new session asks again");

    let mut v = serde_json::to_value(s.seat(A).unwrap().unwrap()).unwrap();
    assert!(
        v.get("model_requested").is_some() && v.get("model_observed").is_some(),
        "{v}"
    );
    v.as_object_mut()
        .unwrap()
        .retain(|k, _| !k.starts_with("model_"));
    let back: controld::views::SeatView = serde_json::from_value(v).unwrap();
    assert_eq!((back.model_requested, back.model_observed), (None, None));
}
