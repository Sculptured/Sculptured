//! Helpers shared by the controld integration tests.
#![allow(dead_code)]
pub mod community;
pub mod m2;
pub mod seat_tools;
use controld::AppState;
use model::SeatLifecycle;
use std::path::PathBuf;
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

/// A fixtures binary (`fake-claude`, `fake-codex`), built if needed, from this test binary's target folder.
pub fn fake_bin(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap().to_path_buf();
    let bin = dir.join(name);
    if !bin.exists() {
        let ok = std::process::Command::new(env!("CARGO"))
            .args(["build", "-q", "-p", "fixtures", "--bin", name])
            .status()
            .unwrap()
            .success();
        assert!(ok, "cannot build {name}");
    }
    bin
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
