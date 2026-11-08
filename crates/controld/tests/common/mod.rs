//! Helpers shared by the controld integration tests.
#![allow(dead_code)]
pub mod community;
pub mod live;
pub mod m2;
pub mod pausing;
pub mod seat_tools;
use controld::AppState;
use model::SeatLifecycle;
use std::time::Duration;

pub fn lifecycle(st: &AppState, seat: &str) -> Option<SeatLifecycle> {
    st.store
        .lock()
        .unwrap()
        .seat(seat)
        .unwrap()
        .map(|s| s.lifecycle)
}

pub async fn wait_for(what: &str, pred: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !pred() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Compare the work payload separately from its captured architecture briefing.
pub fn work_message(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find("Team architecture (operator configuration):") {
        let Some(end) = out[start..].find("\n\n--- Current message ---\n") else {
            break;
        };
        out.replace_range(
            start..start + end + "\n\n--- Current message ---\n".len(),
            "",
        );
    }
    out
}
