//! The budget gate: every command that starts work (Launch, Restart, Send) passes it on its way to
//! a runner, whatever sent it (Start, a spawn, a copy, a dispatch, a handoff, a person's message).
//! Past the soft value nothing new starts; with "most agents working at once" (the team's, or the
//! computer's across all its teams), a message to an idle agent waits for a free slot. Interrupts,
//! stops and answers are never held.
use super::record::BudgetState;
use crate::store::Store;
use host_proto::HostCommand;
use rusqlite::params;
use std::sync::{Arc, Mutex};

/// What the gate does with a command.
#[derive(Debug, PartialEq)]
pub enum Gate {
    Pass,
    /// Not sent, with the reason.
    Refuse(String),
    /// Kept to be sent when a slot frees; the sender is told it went.
    Queued,
}

fn seat_of(cmd: &HostCommand) -> Option<&str> {
    match cmd {
        HostCommand::Launch { seat, .. } | HostCommand::Restart { seat, .. } | HostCommand::Send { seat, .. } => Some(seat),
        _ => None,
    }
}

/// Agents of `rig` busy now: working, starting, or holding a message they have not begun.
pub fn busy(store: &Store, rig: &str, except: &str) -> rusqlite::Result<u32> {
    store.connection().query_row(
        "SELECT COUNT(*) FROM seats s WHERE s.rig = ?1 AND s.id <> ?2 AND (s.activity = 'working' OR s.lifecycle = 'launching'
           OR EXISTS (SELECT 1 FROM messages m WHERE m.seat = s.id AND m.state = 'delivered'))",
        params![rig, except],
        |r| r.get::<_, i64>(0).map(|n| n.max(0) as u32),
    )
}

/// Agents busy now on computer `host`, across every team.
pub fn busy_on_host(store: &Store, host: &str, except: &str) -> rusqlite::Result<u32> {
    store.connection().query_row(
        "SELECT COUNT(*) FROM seats s WHERE s.host = ?1 AND s.id <> ?2 AND (s.activity = 'working' OR s.lifecycle = 'launching'
           OR EXISTS (SELECT 1 FROM messages m WHERE m.seat = s.id AND m.state = 'delivered'))",
        params![host, except],
        |r| r.get::<_, i64>(0).map(|n| n.max(0) as u32),
    )
}

/// The cap of the computer `seat` runs on, when it has one: (host, cap).
fn host_cap(store: &Store, view: &crate::views::SeatView) -> rusqlite::Result<Option<(String, u32)>> {
    let Some(host) = view.host.clone() else { return Ok(None) };
    Ok(store.host_limit(&host)?.map(|n| (host, n)))
}

/// True when the computer `seat` runs on has a free slot (or no cap).
fn host_free(store: &Store, seat: &str) -> rusqlite::Result<bool> {
    let Some(view) = store.seat(seat)? else { return Ok(true) };
    Ok(match host_cap(store, &view)? {
        Some((host, max)) => busy_on_host(store, &host, seat)? < max,
        None => true,
    })
}

fn queue(store: &Store, seat: &str, cmd: &HostCommand) -> rusqlite::Result<()> {
    let HostCommand::Send { message_id, .. } = cmd else { return Ok(()) };
    store.connection().execute(
        "INSERT OR IGNORE INTO budget_queue (seat, message_id, json, at_ms) VALUES (?1, ?2, ?3, ?4)",
        params![seat, message_id, serde_json::to_string(cmd).expect("command serializes"), model::now_ms()],
    )?;
    Ok(())
}

/// Why `rig` takes no new work now (its run is past a soft value), or None.
pub fn refusal(store: &Store, rig: &str) -> rusqlite::Result<Option<String>> {
    Ok(store.run_budget(rig)?.and_then(|r| match r.state {
        BudgetState::WrapUp { reason, .. } | BudgetState::Stopped { reason, .. } | BudgetState::Paused { reason, .. } => Some(format!("Run budget: {reason}")),
        BudgetState::Normal => None,
    }))
}

/// The gate's answer for `cmd`, from the store alone (it runs inside every command).
pub fn decide(store: &Store, cmd: &HostCommand) -> rusqlite::Result<Gate> {
    let Some(seat) = seat_of(cmd) else { return Ok(Gate::Pass) };
    let Some(view) = store.seat(seat)? else { return Ok(Gate::Pass) };
    let sending = matches!(cmd, HostCommand::Send { .. });
    let budget = store.run_budget(&view.rig)?;
    if let Some(r) = &budget {
        match &r.state {
            BudgetState::Stopped { reason, .. } | BudgetState::Paused { reason, .. } => return Ok(Gate::Refuse(format!("Run budget: {reason}"))),
            // The wrap-up instruction itself goes to agents that are wrapping up; nothing else does.
            BudgetState::WrapUp { .. } if sending && store.control(seat)?.is_some() => return Ok(Gate::Pass),
            BudgetState::WrapUp { reason, .. } => return Ok(Gate::Refuse(format!("Run budget: {reason}"))),
            BudgetState::Normal => {}
        }
    }
    if !sending || view.activity == model::SeatActivity::Working {
        return Ok(Gate::Pass);
    }
    let team_full = match budget.and_then(|r| r.budget.max_working) {
        Some(max) => busy(store, &view.rig, seat)? >= max,
        None => false,
    };
    if team_full || !host_free(store, seat)? {
        queue(store, seat, cmd)?;
        return Ok(Gate::Queued);
    }
    Ok(Gate::Pass)
}

/// The gate as the host registry calls it. A held message is recorded on its agent, so the board
/// says the agent waits for a slot.
pub fn hook(store: Arc<Mutex<Store>>, hub: Hub) -> crate::hosts::GateFn {
    Arc::new(move |cmd: &HostCommand| {
        let mut store = store.lock().unwrap();
        match decide(&store, cmd) {
            Ok(Gate::Queued) => {
                held(&mut store, &hub, cmd);
                Gate::Queued
            }
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(error = %e, "budget gate could not read the store; passing the command");
                Gate::Pass
            }
        }
    })
}

type Hub = tokio::sync::broadcast::Sender<crate::views::StreamItem>;

fn held(store: &mut Store, hub: &Hub, cmd: &HostCommand) {
    let HostCommand::Send { seat, message_id, .. } = cmd else { return };
    let view = store.seat(seat).ok().flatten();
    let team = view.as_ref().and_then(|v| store.run_budget(&v.rig).ok().flatten()).and_then(|r| r.budget.max_working);
    let team_full = team.zip(view.as_ref()).is_some_and(|(max, v)| busy(store, &v.rig, seat).is_ok_and(|n| n >= max));
    let host = view.as_ref().and_then(|v| host_cap(store, v).ok().flatten());
    let reason = match (team, host) {
        (Some(n), _) if team_full => format!("waiting for a slot: at most {n} agents of this run work at once"),
        (_, Some((_, n))) => format!("waiting for a slot: at most {n} agents work at once on this computer"),
        (Some(n), None) => format!("waiting for a slot: at most {n} agents of this run work at once"),
        (None, None) => "waiting for a slot".to_string(),
    };
    let body = model::EventBody::MessageHeld { message_id: message_id.clone(), reason };
    match store.append_local(crate::hosts::controld(), model::Subject::Seat(seat.clone()), model::Provenance::Operator, body) {
        Ok(item) => {
            let _ = hub.send(item);
        }
        Err(e) => tracing::warn!(%seat, error = %e, "budget gate could not record a held message"),
    }
}

/// A waiting message: (seat, message id, the Send command).
type Waiting = (String, String, HostCommand);

fn waiting(store: &Store, rig: &str) -> rusqlite::Result<Vec<Waiting>> {
    let mut q = store.connection().prepare(
        "SELECT q.seat, q.message_id, q.json FROM budget_queue q JOIN seats s ON s.id = q.seat WHERE s.rig = ?1 ORDER BY q.at_ms, q.rowid",
    )?;
    let rows = q.query_map([rig], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?;
    Ok(rows.filter_map(Result::ok).filter_map(|(s, m, j)| Some((s, m, serde_json::from_str(&j).ok()?))).collect())
}

fn unqueue(store: &Store, message_id: &str) -> rusqlite::Result<()> {
    store.connection().execute("DELETE FROM budget_queue WHERE message_id = ?1", [message_id]).map(|_| ())
}

fn still_queued(store: &Store, message_id: &str) -> rusqlite::Result<bool> {
    let state: Option<String> = store.connection().query_row("SELECT state FROM messages WHERE id = ?1", [message_id], |r| r.get(0)).ok();
    Ok(state.as_deref() == Some("queued"))
}

fn not_sent(st: &crate::state::AppState, seat: String, message_id: String, reason: String) -> rusqlite::Result<()> {
    st.local_event(crate::hosts::controld(), model::Subject::Seat(seat), model::Provenance::Operator, model::EventBody::MessageFailed { message_id, reason })?;
    Ok(())
}

/// Sends waiting messages while slots are free, oldest first. A message that has meanwhile failed
/// (its agent stopped or restarted) is dropped; past the soft value every waiting message is dropped
/// and recorded as not sent.
pub async fn deliver(st: &crate::state::AppState, rig: &str) -> rusqlite::Result<()> {
    let (list, record) = {
        let store = st.store.lock().unwrap();
        (waiting(&store, rig)?, store.run_budget(rig)?)
    };
    for (seat, message_id, cmd) in list {
        let queued = still_queued(&st.store.lock().unwrap(), &message_id)?;
        if !queued || record.as_ref().is_some_and(|r| !matches!(r.state, BudgetState::Normal)) {
            unqueue(&st.store.lock().unwrap(), &message_id)?;
            if queued {
                not_sent(st, seat, message_id, "Not sent: the run's budget stopped new work".into())?;
            }
            continue;
        }
        let team_free = record.as_ref().and_then(|r| r.budget.max_working).is_none_or(|max| busy(&st.store.lock().unwrap(), rig, &seat).is_ok_and(|n| n < max));
        let free = team_free && host_free(&st.store.lock().unwrap(), &seat)?;
        if !free {
            return Ok(());
        }
        unqueue(&st.store.lock().unwrap(), &message_id)?;
        let sent = match crate::launch::seat_host(st, &seat) {
            Ok(host) => st.hosts.command_ungated(&host, cmd).await.map_err(|e| e.into_message()),
            Err(e) => Err(e.message.to_string()),
        };
        if let Err(reason) = sent {
            not_sent(st, seat, message_id, reason)?;
        }
    }
    Ok(())
}

/// Every team with messages waiting.
pub fn rigs_waiting(store: &Store) -> rusqlite::Result<Vec<String>> {
    let mut q = store.connection().prepare("SELECT DISTINCT s.rig FROM budget_queue q JOIN seats s ON s.id = q.seat")?;
    let rows = q.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// Agents of `rig` with a message waiting for a slot (the board says so on their cards).
pub fn seats_waiting(store: &Store, rig: &str) -> rusqlite::Result<Vec<String>> {
    Ok(waiting(store, rig)?.into_iter().map(|w| w.0).collect())
}
