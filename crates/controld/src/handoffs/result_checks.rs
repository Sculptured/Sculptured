//! Result checks as a gate between agents (`model::result_check`). A completed turn on an agent's work
//! whose result fails its checks does not finish the work (`work::Scan::passes_checks`), so nothing is
//! handed on. Each checked turn is recorded once (`ResultChecked`, verified). For the latest failure,
//! the failures and what the result must have go back to the agent, up to `ROUNDS` times; then it is
//! stuck, and a person can message it or hand the turn over anyway ("Hand over again").
use super::Action;
use super::work::{CHECKS_FAILED, SeatWork};
use crate::state::AppState;
use model::{Actor, EventBody, MessageIntent, Provenance, RuleResult, Subject};

/// How many times failures are sent back for the same work.
pub const ROUNDS: u32 = 3;
pub const ACTOR: &str = "result-check";

/// Checked turns not recorded yet; only the latest, if it failed with rounds left, is sent back.
pub(super) fn due(seat: &str, w: &SeatWork) -> Vec<Action> {
    let still_failing = w
        .unfinished
        .as_ref()
        .is_some_and(|(_, why)| *why == CHECKS_FAILED);
    let last = w.checked.len().saturating_sub(1);
    w.checked
        .iter()
        .enumerate()
        .filter(|(_, c)| !w.checks_recorded.contains(&c.turn_id))
        .map(|(i, c)| Action::Checked {
            seat: seat.to_string(),
            turn_id: c.turn_id.clone(),
            passed: c.passed,
            results: c.results.clone(),
            bounce: i == last && !c.passed && still_failing && c.failures <= ROUNDS,
        })
        .collect()
}

/// For an agent with result checks: how to end its result, told before it starts.
#[must_use]
pub fn briefing_line(_d: &model::TeamDraft, me: &model::DraftAgent) -> Option<String> {
    let checks = &me.advanced.result_checks;
    (!checks.is_empty()).then(|| {
        format!(
            "When you finish your work, end your reply with a fenced json block whose fields are:\n- {}\n\
             Sculptured checks it before your work is handed on; a result that fails is sent back to you.",
            model::result_check::wanted(checks).join("\n- ")
        )
    })
}

/// What the agent is sent: what failed, and what its result must have.
fn bounce_text(results: &[RuleResult], wanted: &[String]) -> String {
    let failed: Vec<String> = results
        .iter()
        .filter(|r| !r.passed)
        .map(|r| format!("- ✗ {}", r.words))
        .collect();
    format!(
        "Your result did not pass the checks it must pass before your work is handed on:\n{}\n\n\
         Fix this and finish again. End your reply with a fenced json block whose fields are:\n- {}",
        failed.join("\n"),
        wanted.join("\n- ")
    )
}

fn checks_of(st: &AppState, seat: &str) -> Vec<model::ResultCheck> {
    let store = st.store.lock().expect("store mutex must not be poisoned");
    crate::seat_ids::rig_of(seat)
        .and_then(|rig| store.draft(rig).ok().flatten())
        .and_then(|d| {
            d.agent_for_seat(seat)
                .map(|a| a.advanced.result_checks.clone())
        })
        .unwrap_or_default()
}

/// Records the check once; sends the failures back when `bounce`.
pub(super) async fn record(
    st: &AppState,
    seat: &str,
    turn_id: &str,
    passed: bool,
    results: Vec<RuleResult>,
    bounce: bool,
) {
    let wanted = model::result_check::wanted(&checks_of(st, seat));
    let text = bounce.then(|| bounce_text(&results, &wanted));
    let actor = Actor::System(ACTOR.into());
    let body = EventBody::ResultChecked {
        turn_id: turn_id.to_string(),
        passed,
        results,
        bounced: bounce,
    };
    // Checked and appended under one lock, so two plans of the same turn record it once.
    let appended = {
        let mut store = st.store.lock().expect("store mutex must not be poisoned");
        let subject = Subject::Seat(seat.to_string());
        let recorded = store
            .latest_of_types(&subject, &["result-checked"], 200)
            .unwrap_or_default()
            .iter()
            .any(
                |e| matches!(&e.body, EventBody::ResultChecked { turn_id: t, .. } if t == turn_id),
            );
        if recorded {
            return;
        }
        store.append_local(actor.clone(), subject, Provenance::Verified, body)
    };
    match appended {
        Ok(item) => st.publish(vec![item]),
        Err(e) => return tracing::error!(%seat, error = %e, "cannot record a result check"),
    }
    if let Some(text) = text
        && let Err(e) = crate::reconciler::send_message_as(
            st,
            seat,
            text,
            MessageIntent::Instruct,
            false,
            actor,
        )
        .await
    {
        tracing::warn!(%seat, error = %e.message, "cannot send a failed result back");
    }
}
