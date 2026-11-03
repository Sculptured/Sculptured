//! Budget continuations, coordinated with team controls. Operator pauses stay in force.
use crate::error::ApiError;
use crate::state::{AppState, BootGuard};
use crate::store::ControlMode;
use model::{Actor, ContinueMode, Desired, MessageIntent, SeatLifecycle};
use std::time::Duration;

/// Budget continuations queue behind team operations, then recheck the operator's control. This
/// also serializes the agents continued by one raise, rather than dropping all but the first. A
/// safety stop of the team or the agent after `since` (`AppState::stop_count` when the continuation
/// was asked for) cancels it, even one that came while it queued (`collaboration::preempt`).
async fn claim(st: &AppState, seat: &str, since: u64) -> Result<BootGuard, ApiError> {
    let rig = crate::seat_ids::rig_of(seat)
        .ok_or_else(|| ApiError::not_found("This agent has no team"))?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(guard) = st.try_begin_team_action(rig, since) {
            return Ok(guard);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ApiError::conflict(
                "team_starting",
                "The team is still changing. Continue its agents once it has settled.",
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn operator_blocks(st: &AppState, seat: &str) -> Result<bool, ApiError> {
    if crate::collaboration::preempt::preempted(st, seat) {
        return Ok(true);
    }
    let store = st.store.lock().expect("store mutex must not be poisoned");
    if matches!(
        store.control(seat)?,
        Some(ControlMode::Pausing | ControlMode::Paused)
    ) {
        return Ok(true);
    }
    let rig = crate::seat_ids::rig_or_empty(seat);
    Ok(store
        .rig_snapshot(rig)?
        .is_some_and(|s| s.desired != Desired::Running))
}

/// An agent still finishing its wrap-up goes on working; one the budget stopped (or is stopping) starts
/// again where it was, once its stop has finished.
pub(super) async fn carry_on(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let _claim = claim(st, seat, st.stop_count()).await?;
    if operator_blocks(st, seat)? {
        return Ok(());
    }
    let text =
        "The run's limit was raised, so you can carry on: continue your job where you left off."
            .to_string();
    let stopping = st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .control(seat)?
        == Some(ControlMode::Hard);
    if !stopping && is_live(st, seat)? {
        return continue_live(st, seat, text).await;
    }
    wait_for_stop(st, seat).await?;
    let by = Actor::System("budget".into());
    restart_claimed(st, seat, ContinueMode::Resume, text, by).await
}

async fn continue_live(st: &AppState, seat: &str, text: String) -> Result<(), ApiError> {
    crate::collaboration::preempt::clear_unless_stopped(st, seat)?;
    let by = Actor::System("budget".into());
    send_instruction(st, seat, text, by).await
}

async fn wait_for_stop(st: &AppState, seat: &str) -> Result<(), ApiError> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while is_live(st, seat)? && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

/// A budget window reset must respect an operator pause too. Explicit Resume uses `pause::relaunch`
/// directly, under its own team claim; it intentionally clears that pause.
pub(super) async fn restart(
    st: &AppState,
    seat: &str,
    mode: ContinueMode,
    text: String,
    by: Actor,
) -> Result<(), ApiError> {
    let _claim = claim(st, seat, st.stop_count()).await?;
    restart_claimed(st, seat, mode, text, by).await
}

/// Restarts and sends the continuation while the caller holds the team's claim.
async fn restart_claimed(
    st: &AppState,
    seat: &str,
    mode: ContinueMode,
    text: String,
    by: Actor,
) -> Result<(), ApiError> {
    // A team stop can change its desired state even while a continuation waits for a hard stop.
    if operator_blocks(st, seat)? {
        return Ok(());
    }
    super::pause::relaunch(st, seat, mode).await?;
    send_instruction(st, seat, text, by).await
}

async fn send_instruction(
    st: &AppState,
    seat: &str,
    text: String,
    by: Actor,
) -> Result<(), ApiError> {
    crate::reconciler::send_message_as(st, seat, text, MessageIntent::Instruct, false, by)
        .await
        .map(|_| ())
}

fn is_live(st: &AppState, seat: &str) -> Result<bool, ApiError> {
    Ok(st
        .store
        .lock()
        .expect("store mutex must not be poisoned")
        .seat(seat)?
        .is_some_and(|v| {
            matches!(
                v.lifecycle,
                SeatLifecycle::Running | SeatLifecycle::Launching
            )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{ControlMode, RigSource, Store};

    const SEAT: &str = "team.agent@budget-pause";
    const SPEC: &str = r#"
version: "0.2"
name: budget-pause
pods:
  - id: team
    label: Team
    members:
      - { id: agent, agent_ref: "local:agent", profile: default, runtime: stub, cwd: "." }
"#;

    fn state(mode: ControlMode) -> AppState {
        let mut store = Store::open_in_memory().unwrap();
        store
            .upsert_rig(
                &rigspec::compile(SPEC).unwrap(),
                &RigSource::File {
                    path: "/test/team.yaml".into(),
                },
                Desired::Running,
            )
            .unwrap();
        store.record_control(SEAT, mode, 0).unwrap();
        AppState::new(store, "test-token".into(), vec![])
    }

    #[tokio::test]
    async fn a_budget_raise_keeps_an_operator_pause_and_does_not_restart() {
        for mode in [ControlMode::Pausing, ControlMode::Paused] {
            let st = state(mode);
            // No runner exists: any attempt to restart must fail this test.
            carry_on(&st, SEAT).await.unwrap();
            assert_eq!(st.store.lock().unwrap().control(SEAT).unwrap(), Some(mode));
        }
    }

    #[tokio::test]
    async fn a_budget_window_reset_keeps_an_operator_pause() {
        for mode in [ControlMode::Pausing, ControlMode::Paused] {
            let st = state(mode);
            restart(
                &st,
                SEAT,
                ContinueMode::Resume,
                "Continue".into(),
                Actor::System("budget".into()),
            )
            .await
            .unwrap();
            assert_eq!(st.store.lock().unwrap().control(SEAT).unwrap(), Some(mode));
        }
    }

    #[tokio::test]
    async fn a_pause_recorded_during_another_team_operation_is_read_after_the_claim() {
        let st = state(ControlMode::Hard);
        let guard = st.try_begin_boot("budget-pause").unwrap();
        let continuation = carry_on(&st, SEAT);
        tokio::pin!(continuation);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut continuation)
                .await
                .is_err()
        );
        st.store
            .lock()
            .unwrap()
            .record_control(SEAT, ControlMode::Paused, 0)
            .unwrap();
        drop(guard);
        continuation.await.unwrap();
        assert_eq!(
            st.store.lock().unwrap().control(SEAT).unwrap(),
            Some(ControlMode::Paused)
        );
        assert!(
            st.try_begin_boot("budget-pause").is_some(),
            "continuation released its claim"
        );
    }

    #[tokio::test]
    async fn stop_now_goes_ahead_while_a_continuation_holds_the_claim_and_cancels_it() {
        let st = state(ControlMode::Budget);
        let since = st.stop_count();
        let _claim = claim(&st, SEAT, since).await.unwrap();
        // The continuation holds the team; Stop everything now does not wait for it.
        tokio::time::timeout(
            Duration::from_secs(1),
            crate::collaboration::stop_now(&st, "budget-pause", &[]),
        )
        .await
        .expect("a safety stop does not wait")
        .unwrap();
        // No runner: an attempt to restart would fail, so Ok means the continuation stood down.
        restart_claimed(
            &st,
            SEAT,
            ContinueMode::Resume,
            "Continue".into(),
            Actor::System("budget".into()),
        )
        .await
        .unwrap();
        assert_eq!(
            st.store.lock().unwrap().control(SEAT).unwrap(),
            Some(ControlMode::Budget)
        );
    }

    #[tokio::test]
    async fn a_continuation_queued_behind_resume_all_is_cancelled_by_stop_now() {
        let st = state(ControlMode::Hard);
        let resume_all = st
            .try_begin_team_action("budget-pause", st.stop_count())
            .unwrap();
        let continuation = carry_on(&st, SEAT);
        tokio::pin!(continuation);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut continuation)
                .await
                .is_err()
        );
        crate::collaboration::stop_now(&st, "budget-pause", &[])
            .await
            .unwrap();
        drop(resume_all);
        // No runner: Ok means it neither waited for a stop nor restarted the agent.
        continuation.await.unwrap();
        assert_eq!(
            st.store.lock().unwrap().control(SEAT).unwrap(),
            Some(ControlMode::Hard)
        );
    }

    #[tokio::test]
    async fn a_budget_raise_does_not_restart_a_stopped_team() {
        let st = state(ControlMode::Budget);
        st.store
            .lock()
            .unwrap()
            .set_desired("budget-pause", Desired::Stopped)
            .unwrap();
        carry_on(&st, SEAT).await.unwrap();
        // The restart rechecks too: stopping a team can cancel a continuation already in flight.
        let _claim = claim(&st, SEAT, 0).await.unwrap();
        restart_claimed(
            &st,
            SEAT,
            ContinueMode::Resume,
            "Continue".into(),
            Actor::System("budget".into()),
        )
        .await
        .unwrap();
        assert_eq!(
            st.store.lock().unwrap().control(SEAT).unwrap(),
            Some(ControlMode::Budget)
        );
    }

    #[tokio::test]
    async fn an_agent_stopped_by_the_budget_still_attempts_to_continue() {
        let st = state(ControlMode::Budget);
        let result = restart(
            &st,
            SEAT,
            ContinueMode::Resume,
            "Continue".into(),
            Actor::System("budget".into()),
        )
        .await;
        assert!(
            result.is_err(),
            "with no runner, a real continuation fails instead of being skipped"
        );
    }
}
