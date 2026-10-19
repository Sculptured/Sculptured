//! Run budgets: what one run of a team may use (plan share, tokens, time), each with a soft value
//! (no new work; every agent wraps up) and a hard value (every agent is stopped at once). Checked
//! every second and after every usage or plan report. See docs/internals/limits-and-usage.md.
pub mod gate;
pub mod measure;
pub mod record;
pub mod pause;
pub mod raise;

use crate::state::AppState;
use crate::store::Store;
use measure::Meter;
pub use record::{BudgetState, Record};
use record::{PlanTrack, Reading};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A run's budget as the board shows it.
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct RunStatus {
    pub state: BudgetState,
    pub meters: Vec<Meter>,
    #[ts(type = "number | null")]
    pub wrap_up_ms: Option<i64>,
    /// How a run that pauses continues after its window resets (absent when it does not pause).
    pub continue_with: Option<model::ContinueMode>,
}

/// The newest reading of `window` on `provider`'s plan in `all`: (percent, resets_at, observed_ms).
fn reading(all: &[crate::plan_usage::PlanUsageView], provider: &str, window: &str) -> Option<measure::Observed> {
    let v = all.iter().find(|v| v.usage.provider == provider)?;
    let w = v.usage.windows.iter().find(|w| w.name == window)?;
    Some((w.used_percent?, w.resets_at, v.observed_ms))
}

/// Records the budget of a run that is starting (an empty one too, so an earlier run's record never
/// governs this run); events after `start_seq` are the run's. Call before any agent launches.
pub fn begin(st: &AppState, rig: &str, budget: &model::RunBudget, start_seq: i64) -> rusqlite::Result<()> {
    let now = model::now_ms();
    let all = if budget.plan.is_empty() { vec![] } else { crate::plan_usage::current(st) };
    let plans = budget.plan.iter().map(|b| {
        let mut t = PlanTrack { provider: b.provider.clone(), window: b.window.clone(), ..Default::default() };
        let fresh = reading(&all, &b.provider, &b.window).filter(|(_, resets, at)| now - at <= measure::FRESH_MS && resets.is_none_or(|r| r * 1000 > now));
        if let Some((percent, resets_at, at_ms)) = fresh {
            t.baseline = Some(percent);
            t.resets_at = resets_at;
            t.first = Some(Reading { percent, tokens: 0, at_ms });
            t.last = t.first;
        }
        t
    });
    let r = Record { budget: budget.clone(), start_seq, start_ms: now, plans: plans.collect(), ..Default::default() };
    st.store.lock().unwrap().put_run_budget(rig, &r)
}

/// Forgets the record `begin` wrote for a start that then failed.
pub fn forget(st: &AppState, rig: &str, start_seq: i64) -> rusqlite::Result<()> {
    st.store.lock().unwrap().connection().execute("DELETE FROM run_budgets WHERE rig = ?1 AND start_seq = ?2", params![rig, start_seq]).map(|_| ())
}

/// A learned rate is per plan window: a weekly window's percent per token is far smaller.
fn rate_key(t: &PlanTrack) -> String {
    format!("budget-rate:{}:{}", t.provider, t.window)
}

fn stored_rate(store: &Store, t: &PlanTrack) -> rusqlite::Result<Option<f64>> {
    let v: Option<String> = store.connection().query_row("SELECT json FROM settings WHERE k = ?1", [rate_key(t)], |r| r.get(0)).optional()?;
    Ok(v.and_then(|j| j.parse().ok()))
}

fn save_rate(store: &Store, t: &PlanTrack, rate: f64) -> rusqlite::Result<()> {
    store.connection().execute(
        "INSERT INTO settings (k, json) VALUES (?1, ?2) ON CONFLICT (k) DO UPDATE SET json = excluded.json",
        params![rate_key(t), rate.to_string()],
    )?;
    Ok(())
}

/// Folds the newest readings into `r` and returns its meters (saving learned rates).
fn measure(st: &AppState, rig: &str, r: &mut Record) -> rusqlite::Result<Vec<Meter>> {
    let all = if r.plans.is_empty() { vec![] } else { crate::plan_usage::current(st) };
    let observed: Vec<Option<measure::Observed>> = r.plans.iter().map(|t| reading(&all, &t.provider, &t.window)).collect();
    let store = st.store.lock().unwrap();
    let spend = measure::spend(&store, rig, r.start_seq)?;
    let working = measure::working(&store, rig)?;
    let mut rates = HashMap::new();
    for (t, o) in r.plans.iter_mut().zip(observed) {
        let tokens = spend.get(t.provider.as_str()).map_or(0, |s| s.tokens);
        if let Some(o) = o.filter(|o| o.2 >= r.start_ms - measure::FRESH_MS) {
            measure::observe(t, o, tokens);
        }
        let learned = measure::rate(t, None);
        if let Some(rate) = learned {
            save_rate(&store, t, rate)?;
        }
        if let Some(rate) = learned.or(stored_rate(&store, t)?) {
            rates.insert(format!("{}:{}", t.provider, t.window), rate);
        }
    }
    let meters = measure::meters(r, &spend, &working, &rates, model::now_ms());
    for (t, m) in r.plans.iter_mut().zip(&meters) {
        t.peak = t.peak.max(m.used.unwrap_or(0.0));
    }
    r.peak_tokens = r.peak_tokens.max(spend.values().map(|s| s.tokens).sum());
    Ok(meters)
}

fn amount(m: &Meter, v: f64) -> String {
    match m.unit.as_str() {
        "percent" => format!("{v:.1}%"),
        "minutes" => format!("{v:.0} min"),
        _ => format!("{} tokens", v.round() as u64),
    }
}

/// The new state, if a meter calls for one. A run always wraps up before it stops, unless a value is
/// already past its hard value: from Normal, the reserve (what working agents may still add) brings
/// the wrap-up forward; once wrapping up, it brings the stop forward.
pub fn decide(state: &BudgetState, meters: &[Meter], now: i64) -> Option<BudgetState> {
    if matches!(state, BudgetState::Stopped { .. } | BudgetState::Paused { .. }) {
        return None;
    }
    let normal = matches!(state, BudgetState::Normal);
    let past = |m: &&Meter, limit: f64, reserve: f64| m.used.is_some_and(|u| u + reserve >= limit);
    let hard = |m: &&Meter| past(m, m.hard, if normal { 0.0 } else { m.reserve });
    if let Some(m) = meters.iter().find(hard) {
        if let Some(p) = m.pause {
            let reason = format!("{}: {} used of the run's hard limit of {}; every agent was stopped and the run is paused until the window resets (window {} of {})",
                m.what, amount(m, m.used.unwrap_or(0.0)), amount(m, m.hard), p.window, p.max_windows);
            return Some(BudgetState::Paused { reason, at_ms: now, until_ms: p.until_ms, window: p.window, max_windows: p.max_windows });
        }
        let reason = format!("{}: {} used of the run's hard limit of {}; every agent was stopped", m.what, amount(m, m.used.unwrap_or(0.0)), amount(m, m.hard));
        return Some(BudgetState::Stopped { reason, at_ms: now });
    }
    let soft = |m: &&Meter| past(m, m.soft, 0.0) || past(m, m.hard, m.reserve);
    if normal && let Some(m) = meters.iter().find(soft) {
        let reason = format!("{}: {} used, near the run's limits (wrap up at {}, stop at {}); no new work, agents are wrapping up",
            m.what, amount(m, m.used.unwrap_or(0.0)), amount(m, m.soft), amount(m, m.hard));
        return Some(BudgetState::WrapUp { reason, at_ms: now });
    }
    None
}

/// Agents of `rig` that are running, and the ones among them no other running agent started.
fn running(store: &Store, rig: &str) -> rusqlite::Result<(Vec<String>, Vec<String>)> {
    let Some(snap) = store.rig_snapshot(rig)? else { return Ok((vec![], vec![])) };
    let live: Vec<String> = snap.seats.into_iter().filter(|s| matches!(s.lifecycle, model::SeatLifecycle::Running | model::SeatLifecycle::Launching)).map(|s| s.id).collect();
    let mut roots = Vec::new();
    for s in &live {
        if store.parent(s)?.is_none_or(|p| !live.contains(&p)) {
            roots.push(s.clone());
        }
    }
    Ok((live, roots))
}

/// Wrap-up: every running agent is asked to finish (one after another: each takes the team's claim).
/// Stopped: every agent is stopped at once.
async fn act(st: &AppState, rig: &str, state: &BudgetState) -> rusqlite::Result<()> {
    let (live, roots) = running(&st.store.lock().unwrap(), rig)?;
    match state {
        BudgetState::WrapUp { .. } => {
            let control = pause::wrap_up_control(&st.store.lock().unwrap(), rig)?;
            for s in live.iter().filter(|s| st.store.lock().unwrap().control(s).ok().flatten().is_none()) {
                if let Err(e) = crate::collaboration::control_agent(st, s, control).await {
                    tracing::warn!(seat = %s, error = %e.message, "budget wrap-up not sent");
                }
            }
        }
        BudgetState::Stopped { .. } | BudgetState::Paused { .. } if !roots.is_empty() => {
            if let Err(e) = crate::collaboration::stop_now(st, rig, &roots).await {
                tracing::warn!(%rig, error = %e.message, "budget stop will retry");
            }
        }
        _ => {}
    }
    Ok(())
}

/// A stopped run whose agents still run (a stop that failed or a team that was busy): stop again.
fn stop_again(r: &Record, live: bool, now: i64) -> bool {
    matches!(r.state, BudgetState::Stopped { at_ms, .. } | BudgetState::Paused { at_ms, .. } if live && now - at_ms > 3_000)
}

/// Repeats what a run's state calls for when it did not take (the team was busy): a stopped run's
/// agents are stopped again, and agents a wrap-up missed get it. True when the run is stopped.
async fn catch_up(st: &AppState, rig: &str, r: &Record) -> rusqlite::Result<bool> {
    match r.state {
        BudgetState::Stopped { .. } | BudgetState::Paused { .. } => {
            if stop_again(r, true, model::now_ms()) {
                act(st, rig, &r.state).await?;
            }
            Ok(true)
        }
        BudgetState::WrapUp { .. } => act(st, rig, &r.state).await.map(|_| false),
        BudgetState::Normal => Ok(false),
    }
}

/// Measures every budgeted run and acts on what it finds.
pub async fn check(st: &AppState) -> rusqlite::Result<()> {
    let rigs = st.store.lock().unwrap().budgeted_rigs()?;
    for rig in rigs {
        let Some(mut r) = st.store.lock().unwrap().run_budget(&rig)? else { continue };
        let live = !running(&st.store.lock().unwrap(), &rig)?.0.is_empty();
        if pause::on_tick(st, &rig, &r, live).await? || r.budget.is_empty() || !live || catch_up(st, &rig, &r).await? {
            continue;
        }
        let meters = measure(st, &rig, &mut r)?;
        let next = decide(&r.state, &meters, model::now_ms());
        if let Some(BudgetState::WrapUp { at_ms, .. }) = &next {
            r.wrap_up_ms = Some(*at_ms);
        }
        if let Some(n) = &next {
            r.state = n.clone();
        }
        if matches!(next, Some(BudgetState::WrapUp { .. } | BudgetState::Paused { .. } | BudgetState::Stopped { .. })) {
            pause::remember(&st.store.lock().unwrap(), &rig, &mut r)?;
        }
        st.store.lock().unwrap().put_run_budget(&rig, &r)?;
        if let Some(n) = next {
            tracing::info!(%rig, state = ?n, "run budget");
            record(st, &rig, &n)?;
            act(st, &rig, &n).await?;
        }
    }
    Ok(())
}

/// Writes the change to the run's log, where run exports and reports of the run read it.
fn record(st: &AppState, rig: &str, state: &BudgetState) -> rusqlite::Result<()> {
    let (name, reason) = match state {
        BudgetState::WrapUp { reason, .. } => ("wrap-up", reason),
        BudgetState::Stopped { reason, .. } => ("stopped", reason),
        BudgetState::Paused { reason, .. } => ("paused", reason),
        BudgetState::Normal => return Ok(()),
    };
    let body = model::EventBody::BudgetStateChanged { state: name.into(), reason: reason.clone() };
    let subject = model::Subject::Rig(rig.to_string());
    st.local_event(crate::hosts::controld(), subject, model::Provenance::ProtocolObserved, body).map(|_| ())
}

/// The run's budget for the board; None when its latest run had none.
pub fn status(st: &AppState, rig: &str) -> rusqlite::Result<Option<RunStatus>> {
    let Some(mut r) = st.store.lock().unwrap().run_budget(rig)?.filter(|r| !r.budget.is_empty()) else { return Ok(None) };
    let meters = measure(st, rig, &mut r)?;
    let continue_with = r.continue_with.or_else(|| r.budget.plan.iter().find_map(|p| p.pause).map(|p| p.continue_with));
    Ok(Some(RunStatus { state: r.state, meters, wrap_up_ms: r.wrap_up_ms, continue_with }))
}

/// Usage, a plan reading, or a turn ending (which may free a slot).
fn is_spend(item: &crate::views::StreamItem) -> bool {
    matches!(item.event.body, model::EventBody::UsageReported { .. } | model::EventBody::PlanUsageReported { .. } | model::EventBody::TurnEnded { .. })
}

/// Event-driven checks run at most this often (the 1-second tick runs regardless).
const EVENT_CHECK_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

/// Checks every second, and soon after usage, a plan reading or a turn's end arrives.
pub async fn run(st: AppState) {
    let mut hub = st.hub.subscribe();
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
    let (mut last, mut pending) = (tokio::time::Instant::now(), false);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = tokio::time::sleep_until(last + EVENT_CHECK_EVERY), if pending => {}
            item = hub.recv() => {
                let spend = match item {
                    Ok(i) => is_spend(&i),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                    Err(_) => return,
                };
                if !spend {
                    continue;
                }
                if last.elapsed() < EVENT_CHECK_EVERY {
                    pending = true; // checked soon, by the branch above
                    continue;
                }
            }
        }
        (last, pending) = (tokio::time::Instant::now(), false);
        pass(&st).await;
    }
}

/// One round: run budgets, agents' own limits, then messages waiting for a slot.
async fn pass(st: &AppState) {
    if let Err(e) = check(st).await {
        tracing::warn!(error = %e, "run budget check failed");
    }
    crate::collaboration::check_agent_limits(st).await;
    crate::collaboration::context_ceiling::check(st).await;
    crate::collaboration::silence::check(st);
    let waiting = gate::rigs_waiting(&st.store.lock().unwrap()).unwrap_or_default();
    for rig in waiting {
        if let Err(e) = gate::deliver(st, &rig).await {
            tracing::warn!(%rig, error = %e, "waiting messages not sent");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meter(used: f64, reserve: f64) -> Meter {
        Meter { what: "Claude 5-hour".into(), unit: "percent".into(), used: Some(used), soft: 25.5, hard: 30.0, reserve, estimated: false, observed_ms: None, pause: None, limit: "plan".into(), provider: None, window: None }
    }

    #[test]
    fn a_run_wraps_up_before_it_stops_even_when_the_reserve_is_large() {
        let normal = BudgetState::Normal;
        assert_eq!(decide(&normal, &[meter(20.0, 1.0)], 1), None);
        // Six agents at about 1% a call: the reserve passes the hard value before the soft one is reached.
        assert!(matches!(decide(&normal, &[meter(24.0, 6.0)], 1), Some(BudgetState::WrapUp { .. })), "wrap up first");
        let wrapping = BudgetState::WrapUp { reason: String::new(), at_ms: 1 };
        assert!(matches!(decide(&wrapping, &[meter(24.0, 6.0)], 2), Some(BudgetState::Stopped { .. })), "then stop, a call early");
        assert!(matches!(decide(&normal, &[meter(31.0, 0.0)], 1), Some(BudgetState::Stopped { .. })), "already past hard: stop at once");
    }
}
