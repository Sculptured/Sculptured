//! The schema: migrations applied in order, counted by `PRAGMA user_version`.
use rusqlite::{Connection, Transaction};

/// Migration 1: the M1 schema, as an M1 build wrote it at user_version 1.
const MIGRATION_1: &str = "
CREATE TABLE IF NOT EXISTS events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    host TEXT NOT NULL,
    host_seq INTEGER NOT NULL,
    at_ms INTEGER NOT NULL,
    subject TEXT NOT NULL,
    json TEXT NOT NULL,
    UNIQUE (host, host_seq)
);
CREATE INDEX IF NOT EXISTS events_by_subject ON events (subject, seq);
CREATE TABLE IF NOT EXISTS host_cursor (host TEXT PRIMARY KEY, last_seq INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS rigs (
    name TEXT PRIMARY KEY,
    spec_path TEXT NOT NULL,
    spec_hash TEXT NOT NULL,
    compiled_json TEXT NOT NULL,
    desired TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS seats (
    id TEXT PRIMARY KEY,
    rig TEXT NOT NULL,
    ord INTEGER NOT NULL,
    pod TEXT NOT NULL,
    member TEXT NOT NULL,
    runtime TEXT NOT NULL,
    label TEXT,
    host TEXT,
    lifecycle TEXT NOT NULL,
    lifecycle_reason TEXT,
    activity TEXT NOT NULL,
    tenure INTEGER NOT NULL DEFAULT 0,
    native_session_id TEXT,
    caps_json TEXT,
    current_turn TEXT,
    current_tool TEXT,
    last_event_ms INTEGER,
    tokens_in INTEGER,
    tokens_out INTEGER
);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    seat TEXT NOT NULL,
    intent TEXT NOT NULL,
    text TEXT NOT NULL,
    state TEXT NOT NULL,
    turn_id TEXT,
    at_ms INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS receipts (
    request_id TEXT PRIMARY KEY,
    fingerprint TEXT NOT NULL,
    status INTEGER NOT NULL,
    body TEXT NOT NULL
);
";

/// Migration 2: the M1 fix wave's columns. A store the fix wave wrote is already at
/// user_version 2 with exactly these columns, so it skips this migration.
const MIGRATION_2: &str = "
ALTER TABLE seats ADD COLUMN lifecycle_provenance TEXT;
ALTER TABLE messages ADD COLUMN reason TEXT;
";

/// Migration 3: a rig records where it came from (a spec file or a team draft)
/// instead of a mandatory file path.
const MIGRATION_3: &str = "
CREATE TABLE rigs_v2 (
    name TEXT PRIMARY KEY,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('file', 'team')),
    source_ref TEXT NOT NULL,
    source_revision INTEGER,
    spec_hash TEXT NOT NULL,
    compiled_json TEXT NOT NULL,
    desired TEXT NOT NULL
);
INSERT INTO rigs_v2 (name, source_kind, source_ref, source_revision, spec_hash, compiled_json, desired)
    SELECT name, 'file', spec_path, NULL, spec_hash, compiled_json, desired FROM rigs;
DROP TABLE rigs;
ALTER TABLE rigs_v2 RENAME TO rigs;
";

/// Migration 4: team drafts (the builder's documents).
const MIGRATION_4: &str = "
CREATE TABLE drafts (
    id TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    revision INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);
";

/// Migration 5: which upstream agents a seat waits for before its first handoff
/// (a JSON array of seat ids; NULL = not waiting).
const MIGRATION_5: &str = "
ALTER TABLE seats ADD COLUMN waiting_for TEXT;
";

/// Migration 6: why a seat's handoffs are stuck, and the handoff cutoff. Starts booted
/// before handoffs existed (an upgraded store, with teams that may still be running) are
/// at or below the cutoff and are never planned: no phantom messages, no catch-up sends.
const MIGRATION_6: &str = "
ALTER TABLE seats ADD COLUMN handoff_stuck TEXT;
CREATE TABLE handoff_cutoff (seq INTEGER NOT NULL);
INSERT INTO handoff_cutoff (seq) SELECT COALESCE(MAX(seq), 0) FROM events;
";

/// Migration 7: events by subject and body type, so the handoff planner reads only the
/// event types it needs (a long Claude session is mostly tool calls, results and usage).
/// The expression must match the queries' `json_extract(json, '$.body.type')` exactly.
const MIGRATION_7: &str = "
CREATE INDEX IF NOT EXISTS events_by_subject_type ON events (subject, json_extract(json, '$.body.type'), seq);
";

/// Migration 8: M2 management. Questions, interpreted actions, settings, and the seat
/// columns the boards read. The work ledger has no table: it is read from the handoff events.
/// Changed before release (fa7c5c8): the `actions` key is `(seat, tenure, call_id)`, because call ids repeat
/// across sessions. A development store created by b4ae119..43d8868 keeps the old `(seat, call_id)` key
/// (user_version 8 is not migrated again, and `already_applied(8)` does not look at the key): recreate it by
/// deleting that development data folder's `controld.sqlite`. No released store has migration 8.
const MIGRATION_8: &str = "
ALTER TABLE seats ADD COLUMN open_questions INTEGER NOT NULL DEFAULT 0;
ALTER TABLE seats ADD COLUMN risk_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE seats ADD COLUMN restore_offer TEXT;
ALTER TABLE seats ADD COLUMN cwd TEXT;
ALTER TABLE seats ADD COLUMN limit_hit TEXT;
CREATE TABLE questions (
    id TEXT PRIMARY KEY, seat TEXT NOT NULL, rig TEXT NOT NULL, source TEXT NOT NULL, prompt_json TEXT NOT NULL,
    state TEXT NOT NULL, opened_at_ms INTEGER NOT NULL, opened_seq INTEGER NOT NULL, deadline_ms INTEGER, turn_id TEXT,
    answer_json TEXT, answered_at_ms INTEGER, late_answer_json TEXT, expired_seq INTEGER,
    decision_text TEXT, decision_rationale TEXT, decision_source TEXT, decision_provenance TEXT, decision_at_ms INTEGER,
    withdrawn_reason TEXT, acknowledged INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX questions_by_state ON questions (state, opened_at_ms);
CREATE INDEX questions_by_seat ON questions (seat, opened_at_ms);
CREATE TABLE actions (
    seat TEXT NOT NULL, call_id TEXT NOT NULL, rig TEXT NOT NULL, tenure INTEGER NOT NULL, turn_id TEXT,
    called_seq INTEGER NOT NULL, at_ms INTEGER NOT NULL, tool TEXT NOT NULL, input_json TEXT NOT NULL,
    class_json TEXT, paths_json TEXT NOT NULL DEFAULT '[]', risks_json TEXT NOT NULL DEFAULT '[]', ok INTEGER, result_seq INTEGER,
    PRIMARY KEY (seat, tenure, call_id)
);
CREATE INDEX actions_by_turn ON actions (seat, turn_id);
CREATE TABLE settings (k TEXT PRIMARY KEY, json TEXT NOT NULL);
";

/// Migration 9: question delivery. `unconfirmed_reason`: why a recorded answer (or expiry) may not have
/// reached the agent (NULL once confirmed); `settled`: the program reported the question settled itself.
const MIGRATION_9: &str = "
ALTER TABLE questions ADD COLUMN unconfirmed_reason TEXT;
ALTER TABLE questions ADD COLUMN settled INTEGER NOT NULL DEFAULT 0;
";

/// Migration 10: the model the seat's current session asked for (resolved at launch) and the model
/// its CLI reports using (NULL = unknown). A new session clears both.
const MIGRATION_10: &str = "
ALTER TABLE seats ADD COLUMN model_requested TEXT;
ALTER TABLE seats ADD COLUMN model_observed TEXT;
";

/// Applied in order. `PRAGMA user_version` counts the migrations already applied.
/// Append new migrations; never edit one that has shipped.
const MIGRATIONS: &[&str] = &[
    MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5, MIGRATION_6, MIGRATION_7, MIGRATION_8, MIGRATION_9,
    MIGRATION_10, "CREATE TABLE IF NOT EXISTS workflow_runs (rig TEXT NOT NULL, start_seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(rig, start_seq));",
    "CREATE TABLE group_blocks (id TEXT PRIMARY KEY, name TEXT NOT NULL, revision INTEGER NOT NULL, json TEXT NOT NULL);",
    "CREATE TABLE IF NOT EXISTS archived_projects (id TEXT PRIMARY KEY, archived_at_ms INTEGER NOT NULL);
     CREATE TABLE IF NOT EXISTS agent_children (child TEXT PRIMARY KEY, parent TEXT NOT NULL, rig TEXT NOT NULL, request_key TEXT NOT NULL UNIQUE, task TEXT NOT NULL, created_ms INTEGER NOT NULL);
     CREATE TABLE IF NOT EXISTS agent_controls (seat TEXT PRIMARY KEY, mode TEXT NOT NULL, baseline INTEGER NOT NULL, requested_ms INTEGER NOT NULL, message_id TEXT);
     CREATE TABLE IF NOT EXISTS agent_tool_receipts (id TEXT PRIMARY KEY, payload TEXT NOT NULL, result TEXT NOT NULL);",
    "CREATE TABLE IF NOT EXISTS team_output_budgets (rig TEXT PRIMARY KEY, output_limit INTEGER NOT NULL CHECK(output_limit > 0));
     CREATE TABLE IF NOT EXISTS agent_budget_baselines (seat TEXT PRIMARY KEY, baseline INTEGER NOT NULL);",
    "CREATE TABLE IF NOT EXISTS deployment_specs (rig TEXT NOT NULL, start_seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(rig,start_seq));
     INSERT OR IGNORE INTO deployment_specs SELECT name,(SELECT MAX(seq) FROM events WHERE subject='rig:'||rigs.name AND json_extract(json,'$.body.type')='rig-booted'),compiled_json FROM rigs WHERE EXISTS(SELECT 1 FROM events WHERE subject='rig:'||rigs.name AND json_extract(json,'$.body.type')='rig-booted');",
    "CREATE TABLE IF NOT EXISTS project_visibility (id TEXT PRIMARY KEY, status TEXT NOT NULL CHECK(status IN ('hidden','trashed')));",
    "ALTER TABLE seats ADD COLUMN stalled TEXT;",
    "CREATE TABLE IF NOT EXISTS plan_usage (provider TEXT PRIMARY KEY, json TEXT NOT NULL, at_ms INTEGER NOT NULL, seat TEXT);",
    "ALTER TABLE seats ADD COLUMN last_check TEXT;",
    "CREATE TABLE IF NOT EXISTS deployment_drafts (rig TEXT NOT NULL, start_seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(rig, start_seq));",
    "CREATE TABLE IF NOT EXISTS imported_runs (id INTEGER PRIMARY KEY AUTOINCREMENT, rig TEXT NOT NULL, header TEXT NOT NULL, created_ms INTEGER NOT NULL);
     CREATE TABLE IF NOT EXISTS imported_events (run INTEGER NOT NULL, n INTEGER NOT NULL, seq INTEGER NOT NULL, subject TEXT NOT NULL, line TEXT NOT NULL, PRIMARY KEY(run, n));",
    "CREATE TABLE IF NOT EXISTS team_origins (team TEXT PRIMARY KEY, json TEXT NOT NULL);",
    "CREATE TABLE IF NOT EXISTS schedule_runs (team TEXT NOT NULL, schedule TEXT NOT NULL, at_ms INTEGER NOT NULL, outcome TEXT NOT NULL);
     CREATE INDEX IF NOT EXISTS schedule_runs_by_team ON schedule_runs (team, schedule, at_ms);
     CREATE TABLE IF NOT EXISTS team_decisions (team TEXT NOT NULL, id TEXT NOT NULL, at_ms INTEGER NOT NULL, author TEXT NOT NULL, text TEXT NOT NULL, source TEXT NOT NULL, hidden_ms INTEGER, PRIMARY KEY(team, id));",
    "CREATE TABLE IF NOT EXISTS run_budgets (rig TEXT NOT NULL, start_seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(rig, start_seq));
     CREATE TABLE IF NOT EXISTS budget_queue (seat TEXT NOT NULL, message_id TEXT PRIMARY KEY, json TEXT NOT NULL, at_ms INTEGER NOT NULL);",
    "CREATE TABLE IF NOT EXISTS run_reports (rig TEXT NOT NULL, start_seq INTEGER NOT NULL, json TEXT NOT NULL, at_ms INTEGER NOT NULL, PRIMARY KEY(rig, start_seq));",
    "CREATE TABLE IF NOT EXISTS project_maps (team TEXT NOT NULL, folder TEXT NOT NULL, json TEXT NOT NULL, at_ms INTEGER NOT NULL, PRIMARY KEY(team, folder));
     ALTER TABLE team_decisions ADD COLUMN confirmed_ms INTEGER;",
    "CREATE TABLE IF NOT EXISTS host_limits (host TEXT PRIMARY KEY, max_working INTEGER NOT NULL);",
    // What came of a workflow's dispatch command, per start (`model::DispatchRecord`).
    "CREATE TABLE IF NOT EXISTS workflow_dispatches (rig TEXT NOT NULL, start_seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY(rig, start_seq));",
    // Status notes held for an idle agent: they reach it with its next message instead of waking it.
    "CREATE TABLE IF NOT EXISTS held_notes (message_id TEXT PRIMARY KEY, seat TEXT NOT NULL, sender TEXT NOT NULL, text TEXT NOT NULL, at_ms INTEGER NOT NULL);",
];

/// The schema version this build writes.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub(super) fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "the store was written by a newer sculptured (schema {current}; this build knows {SCHEMA_VERSION})"
        )));
    }
    if current < 0 {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "the store has an invalid schema version ({current}); it was not written by sculptured"
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        if !already_applied(&tx, i + 1)? {
            tx.execute_batch(sql)?;
        }
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
    }
    tx.commit()
}

/// True when migration `n` already shaped this store although `user_version` says
/// otherwise. An M1 build sets user_version back to 2 when it opens a newer store (a
/// downgrade) and leaves the newer tables as they are; running migration 3 or 4 again
/// would then fail and the store would never open.
fn already_applied(tx: &Transaction, n: usize) -> rusqlite::Result<bool> {
    let column = |table: &str, column: &str| -> rusqlite::Result<bool> {
        tx.query_row(&format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"), [column], |r| r.get(0))
    };
    let table = |name: &str| -> rusqlite::Result<bool> {
        tx.query_row("SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |r| r.get(0))
    };
    // The knowledge migration, found by what it makes rather than by its number.
    if MIGRATIONS.get(n - 1).is_some_and(|m| m.contains("project_maps")) {
        return column("team_decisions", "confirmed_ms");
    }
    match n {
        2 => Ok(column("seats", "lifecycle_provenance")? && column("messages", "reason")?),
        3 => column("rigs", "source_kind"),
        4 => table("drafts"),
        5 => column("seats", "waiting_for"),
        6 => table("handoff_cutoff"),
        7 => tx.query_row("SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'index' AND name = 'events_by_subject_type'", [], |r| r.get(0)),
        8 => table("questions"),
        9 => column("questions", "unconfirmed_reason"),
        10 => column("seats", "model_requested"),
        12 => table("group_blocks"),
        17 => column("seats", "stalled"),
        19 => column("seats", "last_check"),
        _ => Ok(false),
    }
}
