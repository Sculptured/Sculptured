//! Socket-free checks of Resume's claim and its re-read of the control mode.
use super::*;
use crate::store::{RigSource, Store};
use model::{Actor, Desired, Provenance, Subject};

const SEAT: &str = "team.agent@control-race";
const SPEC: &str = r#"
version: "0.2"
name: control-race
pods:
  - id: team
    label: Team
    members:
      - { id: agent, agent_ref: "local:agent", profile: default, runtime: stub, cwd: "." }
"#;

fn state(lifecycle: SeatLifecycle) -> AppState {
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
    store.record_control(SEAT, ControlMode::Soft, 0).unwrap();
    let st = AppState::new(store, "test-token".into(), vec![]);
    st.local_event(
        Actor::System("test".into()),
        Subject::Seat(SEAT.into()),
        Provenance::ProtocolObserved,
        EventBody::SeatLifecycleChanged {
            lifecycle,
            reason: None,
        },
    )
    .unwrap();
    st
}

fn mode(st: &AppState) -> Option<ControlMode> {
    st.store.lock().unwrap().control(SEAT).unwrap()
}

#[tokio::test]
async fn resume_does_not_clear_a_stop_while_another_team_operation_holds_the_claim() {
    for lifecycle in [SeatLifecycle::Running, SeatLifecycle::Stopped] {
        let st = state(lifecycle);
        let _claim = st.try_begin_boot("control-race").unwrap();
        let error = control_seat(&st, SEAT, Control::Resume).await.unwrap_err();
        assert_eq!(error.code, "team_starting");
        assert_eq!(mode(&st), Some(ControlMode::Soft));
    }
}

#[tokio::test]
async fn resume_of_a_running_agent_still_calls_off_its_soft_stop() {
    let st = state(SeatLifecycle::Running);
    assert!(
        control_seat(&st, SEAT, Control::Resume)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(mode(&st), None);
    assert!(st.try_begin_boot("control-race").is_some());
}

#[tokio::test]
async fn a_pause_that_arrived_after_the_initial_read_uses_the_pause_resume_path() {
    let st = state(SeatLifecycle::Stopped);
    let seen = st.store.lock().unwrap().seat(SEAT).unwrap().unwrap();
    st.store
        .lock()
        .unwrap()
        .record_control(SEAT, ControlMode::Paused, 0)
        .unwrap();
    // No runner: a failed session resume must restore Paused so the next Resume can retry.
    assert!(undo_stop(&st, &seen).await.is_err());
    assert_eq!(mode(&st), Some(ControlMode::Paused));
    assert!(st.try_begin_boot("control-race").is_some());
}
