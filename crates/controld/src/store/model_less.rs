//! Teams saved before Sculptured dropped its model-less "Practice" agents (kind `practice`, runtime
//! `stub`) still open: on every open, such agents are taken out of every stored design (with the
//! connections to them) and every compiled team (with their launch order and edges), and their seat
//! rows are dropped. Every agent left runs a real model. It touches only rows that mention them.
use rusqlite::{Transaction, params};
use serde_json::Value;
use std::collections::HashSet;

/// Stored designs: a team draft (`agents`, `connections`) at the top or inside the JSON.
const DESIGNS: [&str; 3] = ["drafts", "deployment_drafts", "group_blocks"];
/// Stored compiled teams (`seats`, `edges`, `launch_layers`), by table and JSON column.
const COMPILED: [(&str, &str); 2] = [("rigs", "compiled_json"), ("deployment_specs", "json")];

pub(super) fn drop_model_less_agents(tx: &Transaction) -> rusqlite::Result<()> {
    for table in DESIGNS {
        rewrite(tx, table, "json", "%\"practice\"%", drop_from_design)?;
    }
    for (table, column) in COMPILED {
        rewrite(tx, table, column, "%\"stub\"%", drop_from_compiled)?;
    }
    tx.execute("DELETE FROM seats WHERE runtime = 'stub'", [])?;
    Ok(())
}

/// Rewrites each row of `table` whose `column` is like `like` with `fix`, when it changes it.
fn rewrite(
    tx: &Transaction,
    table: &str,
    column: &str,
    like: &str,
    fix: fn(&mut Value) -> bool,
) -> rusqlite::Result<()> {
    let exists: bool = tx.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let rows: Vec<(i64, String)> = {
        let mut q = tx.prepare(&format!(
            "SELECT rowid, {column} FROM {table} WHERE {column} LIKE ?1"
        ))?;
        q.query_map([like], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    for (rowid, text) in rows {
        let Ok(mut v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if fix(&mut v) {
            tx.execute(
                &format!("UPDATE {table} SET {column} = ?1 WHERE rowid = ?2"),
                params![v.to_string(), rowid],
            )?;
        }
    }
    Ok(())
}

/// Takes every agent of kind `practice` out of each design in `v`, with its connections. True when
/// it changed anything.
fn drop_from_design(v: &mut Value) -> bool {
    let mut changed = false;
    if let Some(o) = v.as_object_mut() {
        let mut gone = HashSet::new();
        if let Some(Value::Array(agents)) = o.get_mut("agents") {
            agents.retain(|a| {
                let practice = a["kind"] == "practice";
                if practice && let Some(id) = a["id"].as_str() {
                    gone.insert(id.to_string());
                }
                !practice
            });
        }
        if !gone.is_empty() {
            changed = true;
            if let Some(Value::Array(links)) = o.get_mut("connections") {
                links.retain(|c| {
                    !(c["from"].as_str().is_some_and(|f| gone.contains(f))
                        || c["to"].as_str().is_some_and(|t| gone.contains(t)))
                });
            }
        }
        for child in o.values_mut() {
            changed |= drop_from_design(child);
        }
    } else if let Some(items) = v.as_array_mut() {
        for child in items {
            changed |= drop_from_design(child);
        }
    }
    changed
}

/// Takes every seat with runtime `stub` out of a compiled team, with its edges and its place in the
/// launch order. True when it changed anything.
fn drop_from_compiled(v: &mut Value) -> bool {
    let mut gone = HashSet::new();
    if let Some(Value::Array(seats)) = v.get_mut("seats") {
        seats.retain(|s| {
            let stub = s["runtime"] == "stub";
            if stub && let Some(id) = s["id"].as_str() {
                gone.insert(id.to_string());
            }
            !stub
        });
    }
    if gone.is_empty() {
        return false;
    }
    let named = |x: &Value| x.as_str().is_some_and(|s| gone.contains(s));
    if let Some(Value::Array(edges)) = v.get_mut("edges") {
        edges.retain(|e| !(named(&e["from"]) || named(&e["to"])));
    }
    if let Some(Value::Array(layers)) = v.get_mut("launch_layers") {
        for layer in layers.iter_mut() {
            if let Some(ids) = layer.as_array_mut() {
                ids.retain(|id| !named(id));
            }
        }
        layers.retain(|l| l.as_array().is_none_or(|ids| !ids.is_empty()));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_design_loses_its_practice_agents_and_their_connections() {
        let mut d = json!({"id": "t", "agents": [
            {"id": "a", "kind": "claude"}, {"id": "p1", "kind": "practice"}],
            "connections": [{"id": "c1", "from": "a", "to": "p1"}, {"id": "c2", "from": "a", "to": "a"}]});
        assert!(drop_from_design(&mut d));
        assert_eq!(d["agents"], json!([{"id": "a", "kind": "claude"}]));
        assert_eq!(
            d["connections"],
            json!([{"id": "c2", "from": "a", "to": "a"}])
        );
        assert!(!drop_from_design(&mut d), "nothing left to drop");
        let mut block =
            json!({"group": {"agents": [{"id": "p", "kind": "practice"}], "connections": []}});
        assert!(drop_from_design(&mut block), "a design inside the JSON");
        assert_eq!(block["group"]["agents"], json!([]));
    }

    #[test]
    fn a_compiled_team_loses_its_stub_seats_their_edges_and_launch_places() {
        let mut c = json!({"seats": [{"id": "x", "runtime": "claude-code"}, {"id": "y", "runtime": "stub"}],
            "edges": [{"from": "x", "to": "y"}], "launch_layers": [["y"], ["x"]]});
        assert!(drop_from_compiled(&mut c));
        assert_eq!(c["seats"], json!([{"id": "x", "runtime": "claude-code"}]));
        assert_eq!(c["edges"], json!([]));
        assert_eq!(c["launch_layers"], json!([["x"]]));
        assert!(!drop_from_compiled(&mut c));
    }
}
