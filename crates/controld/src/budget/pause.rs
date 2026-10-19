//! A run paused at a plan budget, and how it continues once the window resets: each agent that was
//! working starts again from the note it wrote when the run wrapped up, or resumes its conversation.
use super::measure::Spend;
use super::record::{BudgetState, PlanTrack, Reading, Record};
use crate::api::{fingerprint, idempotent};
use crate::collaboration::Control;
use crate::error::ApiError;
use crate::extract::{Json, Path};
use crate::state::AppState;
use crate::store::Store;
use axum::extract::State;
use axum::response::Response;
use axum::routing::post;
use axum::Router;
use host_proto::LaunchMode;
use model::{Actor, ContinueMode, EventBody, MessageIntent, Provenance, Subject, TenureCause};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

/// A run that pauses wraps up at a checkpoint: its agents write the note they continue from.
pub fn wrap_up_control(store: &Store, rig: &str) -> rusqlite::Result<Control> {
    let pauses = store.run_budget(rig)?.is_some_and(|r| r.budget.plan.iter().any(|p| p.pause.is_some()));
    Ok(if pauses { Control::Checkpoint } else { Control::Soft })
}

/// The team's configured agents that are working: the ones a paused run continues with.
pub fn working(store: &Store, rig: &str) -> rusqlite::Result<Vec<String>> {
    let (live, _) = super::running(store, rig)?;
    let Some(snap) = store.rig_snapshot(rig)? else { return Ok(vec![]) };
    Ok(live.into_iter().filter(|s| snap.rig.seats.iter().any(|c| &c.id == s)).collect())
}

/// Adds the agents working now to the ones the run continues with: at its wrap-up (after which they
/// stop), at a stop, and again at a pause. A run continues after its window resets, or when a person
/// raises the limit it reached.
pub fn remember(store: &Store, rig: &str, r: &mut Record) -> rusqlite::Result<()> {
    for s in working(store, rig)? {
        if !r.paused_seats.contains(&s) {
            r.paused_seats.push(s);
        }
    }
    Ok(())
}

/// A pausing run whose agents all finished their wrap-up before a hard value: it pauses now, until the
/// window of its first plan budget that pauses resets.
fn pause_after_wrap_up(r: &Record) -> Option<BudgetState> {
    let (t, b) = r.plans.iter().zip(&r.budget.plan).find(|(_, b)| b.pause.is_some())?;
    let p = super::measure::pause_at(r, t, b)?;
    let what = super::measure::window_words(&t.provider, &t.window);
    let reason = format!("{what}: every agent wrapped up at the soft value, and the run is paused until the window resets (window {} of {})", p.window, p.max_windows);
    Some(BudgetState::Paused { reason, at_ms: model::now_ms(), until_ms: p.until_ms, window: p.window, max_windows: p.max_windows })
}

/// The budget loop's turn for a paused run: once every agent has stopped and the window has reset, it
/// continues. True when the run is paused (nothing else to measure).
pub async fn on_tick(st: &AppState, rig: &str, r: &Record, live: bool) -> rusqlite::Result<bool> {
    if matches!(r.state, BudgetState::WrapUp { .. }) && !live {
        let Some(paused) = pause_after_wrap_up(r) else { return Ok(false) };
        let next = Record { state: paused.clone(), ..r.clone() };
        st.store.lock().unwrap().put_run_budget(rig, &next)?;
        super::record(st, rig, &paused)?;
        return Ok(true);
    }
    let BudgetState::Paused { until_ms, .. } = r.state else { return Ok(false) };
    if !live && model::now_ms() >= until_ms {
        if let Err(e) = continue_run(st, rig, r.clone(), false).await {
            tracing::warn!(%rig, error = %e.message, "a paused run could not continue yet");
        }
    }
    Ok(!live)
}

/// Each budgeted window counts from now: one that reset begins at zero, and on Continue now (`forced`)
/// one that has not reset gives the run a new share from its current reading.
fn new_windows(r: &mut Record, spend: &HashMap<&str, Spend>, now: i64, forced: bool) {
    for t in &mut r.plans {
        let reset = t.resets_at.is_some_and(|s| s * 1000 <= now);
        if !reset && !forced {
            continue;
        }
        let tokens = spend.get(t.provider.as_str()).map_or(0, |s| s.tokens);
        let percent = if reset { 0.0 } else { t.last.map_or(0.0, |l| l.percent) };
        let start = Reading { percent, tokens, at_ms: now };
        let resets_at = if reset { None } else { t.resets_at };
        *t = PlanTrack { provider: t.provider.clone(), window: t.window.clone(), baseline: Some(percent), base_tokens: 0, resets_at, first: Some(start), last: Some(start), peak: t.peak };
    }
}

/// Continues a paused run: a new window for the budget, and its agents started again.
pub async fn continue_run(st: &AppState, rig: &str, paused: Record, forced: bool) -> Result<(), ApiError> {
    let BudgetState::Paused { at_ms, max_windows, .. } = paused.state else {
        return Err(ApiError::conflict("not_paused", "This run is not paused."));
    };
    let (now, mut r) = (model::now_ms(), paused.clone());
    let spend = super::measure::spend(&st.store.lock().unwrap(), rig, r.start_seq)?;
    new_windows(&mut r, &spend, now, forced);
    let mode = r.continue_with.or_else(|| r.budget.plan.iter().find_map(|p| p.pause).map(|p| p.continue_with)).unwrap_or_default();
    let seats = std::mem::take(&mut r.paused_seats);
    (r.windows, r.paused_ms, r.state, r.wrap_up_ms, r.continue_with) = (r.windows + 1, r.paused_ms + (now - at_ms), BudgetState::Normal, None, None);
    st.store.lock().unwrap().put_run_budget(rig, &r)?;
    let said = format!("The plan window reset; the run continues (window {} of {max_windows})", r.windows + 1);
    let body = EventBody::BudgetStateChanged { state: "continued".into(), reason: said };
    st.local_event(crate::hosts::controld(), Subject::Rig(rig.to_string()), Provenance::ProtocolObserved, body)?;
    let text = |seat: &str| continuation(seat, mode, r.windows + 1, max_windows);
    let results = futures::future::join_all(seats.iter().map(|s| restart(st, s, mode, text(s)))).await;
    if !seats.is_empty() && results.iter().all(Result::is_err) {
        // Nothing came back (the computer is offline): stay paused, and try again in a minute.
        let mut retry = paused;
        if let BudgetState::Paused { until_ms, .. } = &mut retry.state {
            *until_ms = now + 60_000;
        }
        st.store.lock().unwrap().put_run_budget(rig, &retry)?;
        return Err(results.into_iter().find_map(Result::err).expect("a failure"));
    }
    Ok(())
}

fn continuation(seat: &str, mode: ContinueMode, window: u32, max: u32) -> String {
    let head = format!("The plan window reset, so this run continues (window {window} of {max}).");
    match mode {
        ContinueMode::Notes => format!("{head} Read your note {} in your folder and continue your job from there. If the run pauses again, update the note.", crate::collaboration::note_path(seat)),
        ContinueMode::Resume => format!("{head} Continue your job where you stopped."),
    }
}

/// Starts `seat` again (fresh, or resuming its conversation), then sends it `text`.
pub(super) async fn restart(st: &AppState, seat: &str, mode: ContinueMode, text: String) -> Result<(), ApiError> {
    st.store.lock().unwrap().clear_control(seat)?;
    let session = st.store.lock().unwrap().seat(seat)?.and_then(|v| v.native_session_id);
    match (mode, session) {
        (ContinueMode::Resume, Some(id)) => {
            let baseline = Some(crate::restore::usage_baseline(st, seat)?);
            crate::restore::relaunch_and_send(st, seat, LaunchMode::Resume { native_session_id: id }, TenureCause::Resume, baseline).await?;
        }
        _ => crate::restore::relaunch_and_send(st, seat, LaunchMode::Fresh, TenureCause::Fresh, None).await?,
    }
    crate::reconciler::send_message_as(st, seat, text, MessageIntent::Instruct, false, Actor::System("budget".into())).await.map(|_| ())
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/teams/{id}/budget/paused", post(paused_action))
}

#[derive(Serialize, Deserialize)]
struct PausedRequest {
    request_id: String,
    /// `continue` (now), `stop`, or `mode` (how the agents continue).
    action: String,
    #[serde(default)]
    continue_with: Option<ContinueMode>,
}

async fn paused_action(State(st): State<AppState>, Path(id): Path<String>, Json(req): Json<PausedRequest>) -> Response {
    let fp = fingerprint(&format!("budget-paused:{id}"), &req);
    let st2 = st.clone();
    idempotent(&st, &req.request_id, fp, move || async move { act(&st2, &id, &req.action, req.continue_with).await.map(|_| json!({})) }).await
}

async fn act(st: &AppState, rig: &str, action: &str, mode: Option<ContinueMode>) -> Result<(), ApiError> {
    let r = st.store.lock().unwrap().run_budget(rig)?.filter(|r| matches!(r.state, BudgetState::Paused { .. }));
    let mut r = r.ok_or_else(|| ApiError::conflict("not_paused", "This run is not paused."))?;
    match action {
        "continue" => continue_run(st, rig, r, true).await,
        "mode" => {
            r.continue_with = Some(mode.ok_or_else(|| ApiError::bad_request("no_mode", "Choose notes or resume."))?);
            Ok(st.store.lock().unwrap().put_run_budget(rig, &r)?)
        }
        "stop" => {
            r.state = BudgetState::Stopped { reason: "Stopped by you while paused".into(), at_ms: model::now_ms() };
            st.store.lock().unwrap().put_run_budget(rig, &r)?;
            Ok(super::record(st, rig, &r.state)?)
        }
        _ => Err(ApiError::bad_request("invalid_action", "Choose continue, stop or mode.")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::measure::{Meter, PauseAt};
    use super::super::{decide, BudgetState};

    fn meter(used: f64, pause: Option<PauseAt>) -> Meter {
        Meter { what: "Claude 5-hour".into(), unit: "percent".into(), used: Some(used), soft: 25.5, hard: 30.0, reserve: 0.0, estimated: false, observed_ms: None, pause, limit: "plan".into(), provider: None, window: None }
    }

    #[test]
    fn a_window_that_reset_counts_from_zero_and_continue_now_gives_a_new_share() {
        use super::super::measure::{share, Spend};
        use super::super::record::{PlanTrack, Reading, Record};
        let track = |resets_at| PlanTrack { provider: "claude".into(), window: "five_hour".into(), baseline: Some(21.0), resets_at: Some(resets_at),
            last: Some(Reading { percent: 50.0, tokens: 30_000_000, at_ms: 1 }), ..Default::default() };
        let spend = std::collections::HashMap::from([("claude", Spend { tokens: 31_000_000, max_call: 0 })]);
        let mut r = Record { plans: vec![track(100)], ..Default::default() };
        super::new_windows(&mut r, &spend, 200_000, false);
        assert_eq!(share(&r.plans[0], 31_000_000, Some(1e-6)), Some((0.0, false)), "the new window starts at zero, from now");
        assert_eq!(share(&r.plans[0], 32_000_000, Some(1e-6)).map(|s| s.0.round()), Some(1.0), "and counts the tokens since");
        let mut early = Record { plans: vec![track(9_999_999)], ..Default::default() };
        super::new_windows(&mut early, &spend, 200_000, false);
        assert_eq!(early.plans[0].baseline, Some(21.0), "a window that has not reset is left alone");
        super::new_windows(&mut early, &spend, 200_000, true);
        assert_eq!((early.plans[0].baseline, early.plans[0].resets_at), (Some(50.0), Some(9_999_999)), "Continue now: a new share from the current reading");
    }

    #[test]
    fn a_pausing_budget_pauses_at_its_hard_value_and_the_last_window_stops() {
        let at = PauseAt { until_ms: 9_000_000, window: 1, max_windows: 3 };
        let wrapping = BudgetState::WrapUp { reason: String::new(), at_ms: 1 };
        let paused = decide(&wrapping, &[meter(30.5, Some(at))], 5).expect("a new state");
        let BudgetState::Paused { until_ms, window, max_windows, reason, .. } = &paused else { panic!("{paused:?}") };
        assert_eq!((*until_ms, *window, *max_windows), (9_000_000, 1, 3));
        assert!(reason.contains("paused until the window resets"), "{reason}");
        assert_eq!(decide(&paused, &[meter(31.0, Some(at))], 6), None, "a paused run stays paused");
        assert!(matches!(decide(&wrapping, &[meter(30.5, None)], 5), Some(BudgetState::Stopped { .. })), "no window left: stop");
    }
}
