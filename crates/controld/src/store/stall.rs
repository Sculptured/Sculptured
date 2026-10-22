//! Stall flags (heuristic, CS2680 D4 p52): the same tool error `REPEATS` times in a row marks the agent
//! stalled, and so does a turn with no output for a long time (`silent_seats`, an idea from Gas Town's
//! patrol). Only flagged; the operator decides (nudge, restart fresh, soft stop).
use crate::views::Stall;
use model::{EventBody, Subject};
use rusqlite::{Transaction, params};

const REPEATS: usize = 3;

/// A successful result clears the flag; an error that repeats the last ones sets it (once: the time it
/// started stays).
pub(super) fn on_tool_result(
    tx: &Transaction,
    seat: &str,
    is_error: bool,
    at_ms: i64,
) -> rusqlite::Result<()> {
    if !is_error {
        tx.execute("UPDATE seats SET stalled = NULL WHERE id = ?1", [seat])?;
        return Ok(());
    }
    let Some(error) = repeated_error(tx, seat)? else {
        return Ok(());
    };
    let stall = Stall {
        reason: format!("the same tool error {REPEATS} times in a row: {error}"),
        at_ms,
        silent: false,
    };
    let json = serde_json::to_string(&stall).expect("a stall serializes");
    tx.execute(
        "UPDATE seats SET stalled = ?2 WHERE id = ?1 AND stalled IS NULL",
        params![seat, json],
    )?;
    Ok(())
}

/// The error's first line when the seat's last `REPEATS` tool results all failed with it.
fn repeated_error(tx: &Transaction, seat: &str) -> rusqlite::Result<Option<String>> {
    let sql = "SELECT json FROM events WHERE subject = ?1 AND json_extract(json, '$.body.type') = 'tool-resulted'
               ORDER BY seq DESC LIMIT ?2";
    let recent = crate::store::rows::query_events(
        tx,
        sql,
        params![Subject::Seat(seat.to_string()).key(), REPEATS as i64],
    )?;
    let lines: Vec<Option<String>> = recent
        .iter()
        .map(|e| match &e.body {
            EventBody::ToolResulted {
                is_error: true,
                output,
                ..
            } => Some(first_line(output)),
            _ => None,
        })
        .collect();
    let first = lines.first().cloned().flatten();
    Ok(first.filter(|l| lines.len() == REPEATS && lines.iter().all(|x| x.as_ref() == Some(l))))
}

fn first_line(output: &str) -> String {
    output
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

/// `StallSuspected`: flags a silent agent, once.
pub(super) fn on_suspected(
    tx: &Transaction,
    seat: &str,
    reason: &str,
    at_ms: i64,
) -> rusqlite::Result<()> {
    let json = serde_json::to_string(&Stall {
        reason: reason.to_string(),
        at_ms,
        silent: true,
    })
    .expect("a stall serializes");
    tx.execute(
        "UPDATE seats SET stalled = ?2 WHERE id = ?1 AND stalled IS NULL",
        params![seat, json],
    )?;
    Ok(())
}

/// Any output from the agent clears a silence flag (a repeated-error flag waits for a success).
pub(super) fn on_output(tx: &Transaction, seat: &str) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE seats SET stalled = NULL WHERE id = ?1 AND json_extract(stalled, '$.silent') = 1",
        [seat],
    )?;
    Ok(())
}

/// A running agent in a turn with no event for `quiet_ms`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Silent {
    pub seat: String,
    pub quiet_ms: i64,
    pub tool: Option<String>,
}

impl Silent {
    #[must_use]
    pub fn reason(&self) -> String {
        let minutes = self.quiet_ms / 60_000;
        match &self.tool {
            Some(t) => format!("no output for {minutes} minutes in its turn, while running {t}"),
            None => format!("no output for {minutes} minutes in its turn"),
        }
    }
}

/// Tools that wait for a person: an agent in one is waiting, not silent.
fn waits_for_a_person(tool: &str) -> bool {
    tool.ends_with("request_input") || tool.ends_with("__approve")
}

impl super::Store {
    /// Running agents in a turn whose last event is older than `quiet_ms`, not yet flagged and not
    /// waiting for a person.
    pub fn silent_seats(&self, now_ms: i64, quiet_ms: i64) -> rusqlite::Result<Vec<Silent>> {
        let mut q = self.conn.prepare(
            "SELECT id, last_event_ms, current_tool FROM seats WHERE lifecycle = 'running' AND current_turn IS NOT NULL
             AND stalled IS NULL AND last_event_ms IS NOT NULL AND last_event_ms <= ?1",
        )?;
        let rows = q.query_map([now_ms - quiet_ms], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (seat, last, tool) = row?;
            if tool.as_deref().is_some_and(waits_for_a_person) {
                continue;
            }
            out.push(Silent {
                seat,
                quiet_ms: now_ms - last,
                tool,
            });
        }
        Ok(out)
    }
}
