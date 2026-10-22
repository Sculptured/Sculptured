//! Long-running controld tasks for M2, started once per state with the router (like the handoff dispatcher).
use crate::state::AppState;
use std::sync::atomic::Ordering;

pub fn ensure_running(st: &AppState) {
    let Ok(rt) = tokio::runtime::Handle::try_current() else {
        return;
    };
    if st.background_started.swap(true, Ordering::SeqCst) {
        return;
    }
    rt.spawn(crate::questions::run(st.clone()));
    rt.spawn(crate::workflow::run(st.clone()));
    rt.spawn(crate::collaboration::run(st.clone()));
    rt.spawn(crate::checks::run(st.clone()));
    rt.spawn(crate::claims::run(st.clone()));
    rt.spawn(crate::budget::run(st.clone()));
    rt.spawn(crate::run_reports::run(st.clone()));
    rt.spawn(crate::traces::export::run(st.clone()));
}
