use controld::store::{RigSource, Store, SCHEMA_VERSION};
use model::{Desired, Provenance};
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

/// The schema an M1 build wrote (user_version 1).
const M1_SCHEMA: &str = "
CREATE TABLE events (seq INTEGER PRIMARY KEY AUTOINCREMENT, host TEXT NOT NULL, host_seq INTEGER NOT NULL,
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
PRAGMA user_version = 1;
";

/// The schema the M1 fix wave wrote (user_version 2): the M1 schema plus two columns.
fn fix_wave_schema() -> String {
    M1_SCHEMA.replace(
        "PRAGMA user_version = 1;",
        "ALTER TABLE seats ADD COLUMN lifecycle_provenance TEXT;
         ALTER TABLE messages ADD COLUMN reason TEXT;
         PRAGMA user_version = 2;",
    )
}

fn version(path: &std::path::Path) -> i64 {
    Connection::open(path).unwrap().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap()
}

fn has_column(path: &std::path::Path, table: &str, column: &str) -> bool {
    Connection::open(path)
        .unwrap()
        .query_row(
            &format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"),
            [column],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn a_new_store_is_at_the_current_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    drop(Store::open(&path).unwrap());
    const { assert!(SCHEMA_VERSION >= 3) };
    assert_eq!(version(&path), SCHEMA_VERSION);
    assert!(has_column(&path, "seats", "lifecycle_provenance"));
    assert!(has_column(&path, "messages", "reason"));
    assert!(has_column(&path, "rigs", "source_kind"));
    assert!(has_column(&path, "seats", "waiting_for"));
    assert!(has_column(&path, "seats", "handoff_stuck"));
    assert!(has_table(&path, "handoff_cutoff"));
}

#[test]
fn an_m1_store_migrates_and_keeps_its_rigs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    let rig = rigspec::compile(SPEC).unwrap();
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(M1_SCHEMA).unwrap();
        c.execute(
            "INSERT INTO rigs (name, spec_path, spec_hash, compiled_json, desired) VALUES (?1, ?2, ?3, ?4, 'running')",
            rusqlite::params![rig.name, "/specs/r.yaml", rig.spec_hash, serde_json::to_string(&rig).unwrap()],
        )
        .unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.rig_source("r").unwrap(), Some(RigSource::File { path: "/specs/r.yaml".into() }));
    assert_eq!(s.list_rigs().unwrap()[0].name, "r");
    assert_eq!(s.rig_snapshot("r").unwrap().unwrap().desired, Desired::Running);
    drop(s);
    assert_eq!(version(&path), SCHEMA_VERSION);
    assert!(has_column(&path, "seats", "lifecycle_provenance"));
    assert!(has_column(&path, "messages", "reason"));
    // Reopening an up-to-date store changes nothing.
    let s = Store::open(&path).unwrap();
    assert_eq!(s.list_rigs().unwrap().len(), 1);
}

#[test]
fn a_fix_wave_store_at_version_2_migrates_its_spec_path_rigs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    let rig = rigspec::compile(SPEC).unwrap();
    {
        let c = Connection::open(&path).unwrap();
        c.execute_batch(&fix_wave_schema()).unwrap();
        c.execute(
            "INSERT INTO rigs (name, spec_path, spec_hash, compiled_json, desired) VALUES (?1, ?2, ?3, ?4, 'running')",
            rusqlite::params![rig.name, "/specs/r.yaml", rig.spec_hash, serde_json::to_string(&rig).unwrap()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO seats (id, rig, ord, pod, member, runtime, lifecycle, activity, lifecycle_provenance)
             VALUES ('dev.a@r', 'r', 0, 'dev', 'a', 'stub', 'stopped', 'idle', 'heuristic')",
            [],
        )
        .unwrap();
    }
    let mut s = Store::open(&path).unwrap();
    assert_eq!(s.rig_source("r").unwrap(), Some(RigSource::File { path: "/specs/r.yaml".into() }));
    assert_eq!(s.rig_snapshot("r").unwrap().unwrap().desired, Desired::Running);
    assert_eq!(s.seat("dev.a@r").unwrap().unwrap().lifecycle_provenance, Some(Provenance::Heuristic));
    s.upsert_rig(&rig, &RigSource::File { path: "/specs/r.yaml".into() }, Desired::Running).unwrap();
    drop(s);
    assert_eq!(version(&path), SCHEMA_VERSION);
}

#[test]
fn a_store_from_a_newer_build_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    drop(Store::open(&path).unwrap());
    Connection::open(&path).unwrap().pragma_update(None, "user_version", SCHEMA_VERSION + 1).unwrap();
    let err = Store::open(&path).err().expect("a newer schema must be refused");
    assert!(err.to_string().contains("newer"), "{err}");
}

#[test]
fn rig_sources_are_typed_and_round_trip() {
    let mut s = Store::open_in_memory().unwrap();
    let rig = rigspec::compile(SPEC).unwrap();
    s.upsert_rig(&rig, &RigSource::Team { id: "r".into(), revision: 4 }, Desired::Running).unwrap();
    assert_eq!(s.rig_source("r").unwrap(), Some(RigSource::Team { id: "r".into(), revision: 4 }));
    s.upsert_rig(&rig, &RigSource::File { path: "/x.yaml".into() }, Desired::Running).unwrap();
    assert_eq!(s.rig_source("r").unwrap(), Some(RigSource::File { path: "/x.yaml".into() }));
    assert_eq!(s.rig_source("nope").unwrap(), None);

    let file = RigSource::File { path: "/a".into() };
    let team = |id: &str| RigSource::Team { id: id.into(), revision: 1 };
    assert!(file.same_owner(&RigSource::File { path: "/b".into() }));
    assert!(team("t").same_owner(&team("t")));
    assert!(!team("t").same_owner(&team("u")));
    assert!(!file.same_owner(&team("t")));
}

/// The schema a v3 build wrote: the fix-wave schema with rigs recording their source.
fn v3_schema() -> String {
    fix_wave_schema().replace(
        "PRAGMA user_version = 2;",
        "DROP TABLE rigs;
         CREATE TABLE rigs (name TEXT PRIMARY KEY, source_kind TEXT NOT NULL CHECK (source_kind IN ('file', 'team')),
             source_ref TEXT NOT NULL, source_revision INTEGER, spec_hash TEXT NOT NULL, compiled_json TEXT NOT NULL,
             desired TEXT NOT NULL);
         PRAGMA user_version = 3;",
    )
}

fn has_table(path: &std::path::Path, table: &str) -> bool {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get(0))
        .unwrap()
}

/// Team drafts arrive in migration 4: an M1 store and a v3 store both gain the
/// `drafts` table, and it holds a draft across a reopen.
#[test]
fn m1_and_v3_stores_gain_the_drafts_table() {
    for (label, schema) in [("m1", M1_SCHEMA.to_string()), ("v3", v3_schema())] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.sqlite");
        Connection::open(&path).unwrap().execute_batch(&schema).unwrap();
        assert!(!has_table(&path, "drafts"), "{label}: the old schema has no drafts");
        let mut s = Store::open(&path).unwrap();
        let draft = rigspec::templates::template("solo", "solo-agent", "Solo agent", true).unwrap();
        s.insert_draft(&draft).unwrap();
        drop(s);
        assert!(has_table(&path, "drafts"), "{label}");
        assert_eq!(version(&path), SCHEMA_VERSION, "{label}");
        let s = Store::open(&path).unwrap();
        assert_eq!(s.draft("solo-agent").unwrap(), Some(draft), "{label}: the draft survives a reopen");
    }
}

/// What opening a store with an M1 build does (its `Store::init`): create any missing M1
/// table, add the fix-wave columns if missing, and set user_version back to 2.
fn m1_build_opens(path: &std::path::Path) {
    let c = Connection::open(path).unwrap();
    let if_missing = M1_SCHEMA
        .replace("CREATE TABLE ", "CREATE TABLE IF NOT EXISTS ")
        .replace("CREATE INDEX ", "CREATE INDEX IF NOT EXISTS ")
        .replace("PRAGMA user_version = 1;", "");
    c.execute_batch(&if_missing).unwrap();
    c.pragma_update(None, "user_version", 2).unwrap();
}

/// Run this build, then an M1 build (a downgrade), then this build again: the store
/// already has the new rigs table and the drafts table, so migrations 3 and 4 are
/// recognised as applied instead of failing on the missing `spec_path` column.
#[test]
fn a_store_downgraded_to_m1_and_upgraded_again_still_opens_with_its_teams() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    let rig = rigspec::compile(SPEC).unwrap();
    let draft = rigspec::templates::template("solo", "solo-agent", "Solo agent", true).unwrap();
    {
        let mut s = Store::open(&path).unwrap();
        s.upsert_rig(&rig, &RigSource::Team { id: "r".into(), revision: 3 }, Desired::Running).unwrap();
        s.insert_draft(&draft).unwrap();
    }
    m1_build_opens(&path);
    assert_eq!(version(&path), 2);

    let s = Store::open(&path).expect("the re-upgraded store opens");
    assert_eq!(s.rig_source("r").unwrap(), Some(RigSource::Team { id: "r".into(), revision: 3 }));
    assert_eq!(s.draft("solo-agent").unwrap(), Some(draft));
    drop(s);
    assert_eq!(version(&path), SCHEMA_VERSION);
    assert!(has_column(&path, "seats", "waiting_for"), "migration 5 is recognised as applied");
    assert!(has_table(&path, "handoff_cutoff"), "migration 6 is recognised as applied");
    // And it keeps opening.
    drop(Store::open(&path).unwrap());
}

/// A v3 store (new rigs table, no drafts) downgraded to M1 skips migration 3 only.
#[test]
fn a_v3_store_downgraded_to_m1_skips_only_the_rigs_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    Connection::open(&path).unwrap().execute_batch(&v3_schema()).unwrap();
    m1_build_opens(&path);
    assert!(!has_table(&path, "drafts"));
    let mut s = Store::open(&path).expect("opens");
    let draft = rigspec::templates::template("solo", "solo-agent", "Solo agent", true).unwrap();
    s.insert_draft(&draft).unwrap();
    drop(s);
    assert!(has_table(&path, "drafts"));
    assert!(has_column(&path, "rigs", "source_kind"));
    assert_eq!(version(&path), SCHEMA_VERSION);
}

fn has_index(path: &std::path::Path, name: &str) -> bool {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'index' AND name = ?1", [name], |r| r.get(0))
        .unwrap()
}

/// Migration 7 indexes events by subject and body type over the events already stored;
/// a store whose user_version was set back below 7 with the index present skips it.
#[test]
fn migration_7_indexes_existing_events_by_type_and_is_recognised_as_applied() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("c.sqlite");
    {
        let mut s = Store::open(&path).unwrap();
        s.append_local(model::Actor::Human("local".into()), model::Subject::Rig("r".into()), Provenance::Operator, model::EventBody::RigBooted {
            spec_hash: "x".into(),
            seat_count: 1,
        })
        .unwrap();
    }
    // As a schema-6 build wrote it: no index.
    Connection::open(&path).unwrap().execute_batch("DROP INDEX events_by_subject_type; PRAGMA user_version = 6;").unwrap();
    assert!(!has_index(&path, "events_by_subject_type"));
    let s = Store::open(&path).expect("upgrades");
    assert_eq!(s.latest_of_types(&model::Subject::Rig("r".into()), &["rig-booted"], 1).unwrap().len(), 1);
    drop(s);
    assert!(has_index(&path, "events_by_subject_type"));
    assert_eq!(version(&path), SCHEMA_VERSION);
    // Set back (an older build opened it) with the index kept: it opens again.
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 6).unwrap();
    drop(Store::open(&path).expect("reopens"));
    assert_eq!(version(&path), SCHEMA_VERSION);
}

#[test]
fn an_m1_store_gains_the_m2_tables_and_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    Connection::open(&path).unwrap().execute_batch(M1_SCHEMA).unwrap();
    drop(Store::open(&path).unwrap());
    assert_eq!(version(&path), SCHEMA_VERSION);
    assert_eq!(SCHEMA_VERSION, 29);
    for (table, column) in [("seats", "open_questions"), ("seats", "restore_offer"), ("seats", "cwd"), ("seats", "limit_hit"), ("questions", "deadline_ms"), ("actions", "turn_id"), ("questions", "unconfirmed_reason"), ("questions", "settled"), ("seats", "model_requested"), ("seats", "model_observed"), ("seats", "stalled"), ("seats", "last_check"), ("team_decisions", "confirmed_ms")] {
        assert!(has_column(&path, table, column), "{table}.{column}");
    }
    assert!(has_table(&path, "project_maps"));
    assert!(has_table(&path, "host_limits"));
    assert!(has_table(&path, "held_notes"));
    assert!(has_table(&path, "workflow_dispatches"));
    assert!(has_table(&path, "settings"));
    assert!(has_table(&path, "plan_usage"));
    assert!(has_table(&path, "run_reports"));
    assert!(has_table(&path, "run_budgets") && has_table(&path, "budget_queue"));
    assert!(has_table(&path, "deployment_drafts"));
    assert!(has_table(&path, "imported_runs") && has_table(&path, "imported_events"));
    assert!(has_table(&path, "team_origins"));
}

#[test]
fn a_negative_user_version_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    Connection::open(&path).unwrap().execute_batch("PRAGMA user_version = -3;").unwrap();
    let err = Store::open(&path).err().expect("refused").to_string();
    assert!(err.contains("invalid schema version (-3)"), "{err}");
}

#[test]
fn a_store_set_back_to_version_7_skips_migration_8() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    drop(Store::open(&path).unwrap());
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 7).unwrap();
    drop(Store::open(&path).expect("reopens although the M2 tables already exist"));
    assert_eq!(version(&path), SCHEMA_VERSION);
}

/// Migration 9 (question delivery) adds two columns to a version-8 store, and a store set back to 8 with
/// them already present opens again without re-adding them.
#[test]
fn a_version_8_store_gains_the_question_delivery_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("controld.sqlite");
    drop(Store::open(&path).unwrap());
    let has = |col: &str| -> bool {
        Connection::open(&path)
            .unwrap()
            .query_row("SELECT COUNT(*) > 0 FROM pragma_table_info('questions') WHERE name = ?1", [col], |r| r.get(0))
            .unwrap()
    };
    // A version-8 store: the columns dropped, the version set back.
    Connection::open(&path)
        .unwrap()
        .execute_batch("ALTER TABLE questions DROP COLUMN unconfirmed_reason; ALTER TABLE questions DROP COLUMN settled; PRAGMA user_version = 8;")
        .unwrap();
    assert!(!has("unconfirmed_reason") && !has("settled"));
    drop(Store::open(&path).unwrap());
    assert!(has("unconfirmed_reason") && has("settled"));
    assert_eq!(version(&path), SCHEMA_VERSION);
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 8).unwrap();
    drop(Store::open(&path).expect("reopens although the columns already exist"));
    assert_eq!(version(&path), SCHEMA_VERSION);
}
