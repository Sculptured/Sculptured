//! Rigs, their seats, and the team drafts they are started from.
use super::Store;
use super::rows::SEAT_COLS;
use crate::views::{RigSnapshot, RigSummary, SeatView};
use model::{Desired, TeamDraft};
use rigspec::CompiledRig;
use rusqlite::{OptionalExtension, Transaction, params};

/// Where a rig's spec came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RigSource {
    /// Booted from a RigSpec file (the expert path).
    File { path: String },
    /// Started from a team draft at `revision`.
    Team { id: String, revision: u64 },
}

impl RigSource {
    fn columns(&self) -> (&'static str, String, Option<i64>) {
        match self {
            RigSource::File { path } => ("file", path.clone(), None),
            RigSource::Team { id, revision } => ("team", id.clone(), Some(*revision as i64)),
        }
    }

    /// Whether a rig recorded with this source may be restarted from `other`. Any
    /// spec file may replace a file-booted rig (M1 behaviour). A team's rig belongs
    /// to that team only.
    #[must_use]
    pub fn same_owner(&self, other: &RigSource) -> bool {
        match (self, other) {
            (RigSource::File { .. }, RigSource::File { .. }) => true,
            (RigSource::Team { id: a, .. }, RigSource::Team { id: b, .. }) => a == b,
            _ => false,
        }
    }
}

/// Outcome of an optimistic draft save.
#[derive(Clone, Debug, PartialEq)]
pub enum SaveDraft {
    Saved(Box<TeamDraft>),
    Conflict { current: u64 },
    Missing,
}

fn upsert_rig_row(
    tx: &Transaction,
    rig: &CompiledRig,
    source: &RigSource,
    desired: Desired,
) -> rusqlite::Result<()> {
    let (kind, reference, revision) = source.columns();
    let compiled = serde_json::to_string(rig).expect("compiled rig serializes");
    tx.execute(
        "INSERT INTO rigs (name, source_kind, source_ref, source_revision, spec_hash, compiled_json, desired)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (name) DO UPDATE SET source_kind = excluded.source_kind, source_ref = excluded.source_ref,
             source_revision = excluded.source_revision, spec_hash = excluded.spec_hash,
             compiled_json = excluded.compiled_json, desired = excluded.desired",
        params![rig.name, kind, reference, revision, rig.spec_hash, compiled, crate::store::rows::enum_text(&desired)],
    )?;
    Ok(())
}

/// Inserts new seats as planned/unknown, keeps existing seat state, drops seats no longer in the spec.
fn sync_seats(tx: &Transaction, rig: &CompiledRig) -> rusqlite::Result<()> {
    let existing: Vec<String> = {
        let mut st = tx.prepare("SELECT id FROM seats WHERE rig = ?1")?;
        st.query_map([&rig.name], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let matching_existing = existing
        .iter()
        .filter(|id| !rig.seats.iter().any(|s| &s.id == *id));
    for id in matching_existing {
        tx.execute("DELETE FROM seats WHERE id = ?1", [id])?;
    }
    let indexed_seats = rig.seats.iter().enumerate();
    for (ord, s) in indexed_seats {
        tx.execute(
            "INSERT INTO seats (id, rig, ord, pod, member, runtime, label, lifecycle, activity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'planned', 'unknown')
             ON CONFLICT (id) DO UPDATE SET ord = excluded.ord, runtime = excluded.runtime, label = excluded.label",
            params![s.id, rig.name, ord as i64, s.pod, s.member, crate::store::rows::enum_text(&s.runtime), s.label],
        )?;
    }
    Ok(())
}

impl Store {
    /// Inserts new seats as planned/unknown, keeps existing seat state, drops seats
    /// no longer in the spec.
    pub fn upsert_rig(
        &mut self,
        rig: &CompiledRig,
        source: &RigSource,
        desired: Desired,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        upsert_rig_row(&tx, rig, source, desired)?;
        sync_seats(&tx, rig)?;
        tx.execute("INSERT OR REPLACE INTO agent_budget_baselines SELECT id,COALESCE(tokens_out,0) FROM seats WHERE rig=?1", [&rig.name])?;
        tx.commit()
    }

    /// Registers the last seat in `rig` as a newly appended child.
    ///
    /// # Panics
    /// Panics if `rig` has no seats.
    pub fn register_child(
        &mut self,
        rig: &CompiledRig,
        source: &RigSource,
        parent: &str,
        key: &str,
        task: &str,
    ) -> rusqlite::Result<()> {
        let child = rig.seats.last().expect("child was appended");
        let tx = self.conn.transaction()?;
        upsert_rig_row(&tx, rig, source, Desired::Running)?;
        sync_seats(&tx, rig)?;
        tx.execute(
            "INSERT INTO agent_children VALUES (?1,?2,?3,?4,?5,?6)",
            params![child.id, parent, rig.name, key, task, model::now_ms()],
        )?;
        tx.commit()
    }

    pub fn set_desired(&mut self, name: &str, desired: Desired) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE rigs SET desired = ?2 WHERE name = ?1",
            params![name, crate::store::rows::enum_text(&desired)],
        )?;
        Ok(())
    }

    pub fn rig_source(&self, name: &str) -> rusqlite::Result<Option<RigSource>> {
        self.conn
            .query_row(
                "SELECT source_kind, source_ref, source_revision FROM rigs WHERE name = ?1",
                [name],
                |r| {
                    let kind: String = r.get(0)?;
                    let reference: String = r.get(1)?;
                    let revision: Option<i64> = r.get(2)?;
                    Ok(match kind.as_str() {
                        "team" => RigSource::Team {
                            id: reference,
                            revision: revision.unwrap_or(0) as u64,
                        },
                        _ => RigSource::File { path: reference },
                    })
                },
            )
            .optional()
    }

    pub(super) fn seats_where(&self, clause: &str, arg: &str) -> rusqlite::Result<Vec<SeatView>> {
        let mut st = self
            .conn
            .prepare(&format!("SELECT {SEAT_COLS} FROM seats WHERE {clause}"))?;
        st.query_map([arg], crate::store::rows::seat_from_row)?
            .collect()
    }

    pub fn rig_snapshot(&self, name: &str) -> rusqlite::Result<Option<RigSnapshot>> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT compiled_json, desired FROM rigs WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((json, desired)) = row else {
            return Ok(None);
        };
        let rig: CompiledRig = crate::store::rows::json_parse(&json)?;
        let seats = self.seats_where("rig = ?1 ORDER BY ord", name)?;
        let last_seq: i64 =
            self.conn
                .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |r| r.get(0))?;
        Ok(Some(RigSnapshot {
            rig,
            desired: crate::store::rows::enum_parse(&desired)?,
            seats,
            last_seq,
        }))
    }

    pub fn list_rigs(&self) -> rusqlite::Result<Vec<RigSummary>> {
        let mut st = self.conn.prepare(
            "SELECT r.name, r.desired,
                (SELECT COUNT(*) FROM seats s WHERE s.rig = r.name),
                (SELECT COUNT(*) FROM seats s WHERE s.rig = r.name AND s.lifecycle = 'running'),
                (SELECT COUNT(*) FROM seats s WHERE s.rig = r.name AND s.lifecycle IN ('attention', 'failed', 'stale'))
             FROM rigs r WHERE NOT EXISTS(SELECT 1 FROM archived_projects a WHERE a.id=r.name) ORDER BY r.name",
        )?;
        st.query_map([], |r| {
            Ok(RigSummary {
                name: r.get(0)?,
                desired: crate::store::rows::enum_parse(&r.get::<_, String>(1)?)?,
                seat_count: r.get::<_, i64>(2)? as u32,
                running_count: r.get::<_, i64>(3)? as u32,
                attention_count: r.get::<_, i64>(4)? as u32,
            })
        })?
        .collect()
    }

    pub fn insert_draft(&mut self, d: &TeamDraft) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO drafts (id, json, revision, updated_at_ms) VALUES (?1, ?2, ?3, ?4)",
            params![
                d.id,
                serde_json::to_string(d).expect("draft serializes"),
                d.revision as i64,
                model::now_ms()
            ],
        )?;
        Ok(())
    }

    pub fn draft(&self, id: &str) -> rusqlite::Result<Option<TeamDraft>> {
        self.conn
            .query_row("SELECT json FROM drafts WHERE id = ?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|j| crate::store::rows::json_parse(&j))
            .transpose()
    }

    /// Every draft with its last-save time, newest first.
    pub fn drafts(&self) -> rusqlite::Result<Vec<(TeamDraft, i64)>> {
        let mut st = self.conn.prepare("SELECT json, updated_at_ms FROM drafts WHERE NOT EXISTS(SELECT 1 FROM archived_projects a WHERE a.id=drafts.id) ORDER BY updated_at_ms DESC, id")?;
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .map(|row| {
                row.and_then(|(j, at)| Ok((crate::store::rows::json_parse::<TeamDraft>(&j)?, at)))
            })
            .collect()
    }

    /// Saves `d` if the stored revision is still `expected`; the saved draft gets `expected + 1`.
    pub fn save_draft(&mut self, d: &TeamDraft, expected: u64) -> rusqlite::Result<SaveDraft> {
        let tx = self.conn.transaction()?;
        let current: Option<i64> = tx
            .query_row("SELECT revision FROM drafts WHERE id = ?1", [&d.id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(current) = current else {
            return Ok(SaveDraft::Missing);
        };
        if current as u64 != expected {
            return Ok(SaveDraft::Conflict {
                current: current as u64,
            });
        }
        let mut saved = d.clone();
        saved.revision = expected + 1;
        tx.execute(
            "UPDATE drafts SET json = ?2, revision = ?3, updated_at_ms = ?4 WHERE id = ?1",
            params![
                saved.id,
                serde_json::to_string(&saved).expect("draft serializes"),
                saved.revision as i64,
                model::now_ms()
            ],
        )?;
        tx.commit()?;
        Ok(SaveDraft::Saved(Box::new(saved)))
    }

    /// True when a draft or a rig already uses `id`.
    pub fn name_taken(&self, id: &str) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM drafts WHERE id = ?1) OR EXISTS(SELECT 1 FROM rigs WHERE name = ?1)",
            [id],
            |r| r.get(0),
        )
    }
}
