//! Test-only hooks, compiled only with the `test-seams` feature (enabled by this crate's
//! own dev-dependency). Each hook is called with a team id at a fixed point, so a test can
//! act "between" two steps deterministically. Release builds carry none of this.
use std::sync::{Arc, Mutex};

pub type Hook = Arc<dyn Fn(&str) + Send + Sync>;

/// Called after a save's checks pass and before its write, while the store lock is held.
pub static BETWEEN_SAVE_CHECK_AND_WRITE: Mutex<Option<Hook>> = Mutex::new(None);

/// Called with the spec's name once an imported spec is parsed and compiled, before the
/// new team is stored.
pub static AFTER_IMPORT_PARSE: Mutex<Option<Hook>> = Mutex::new(None);

pub(crate) fn run(seam: &Mutex<Option<Hook>>, team: &str) {
    let hook = seam
        .lock()
        .expect("seam mutex must not be poisoned")
        .clone();
    if let Some(hook) = hook {
        hook(team);
    }
}

/// Called with the question id inside `questions::expire`'s claim, after its "still open?" check and
/// before it records the expiry: the window the claim guards against a racing answer.
pub static IN_EXPIRE_BEFORE_RECORD: Mutex<Option<Hook>> = Mutex::new(None);

/// When non-zero, how long (ms) an answer waits for a question someone else holds (default 45 s).
pub static CLAIM_WAIT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// When non-zero, how long (ms) controld waits for a host to reply to a command (default 30 s).
pub static HOST_REPLY_TIMEOUT_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// When non-zero, how long (ms) work waits for a host whose link just dropped (default 45 s).
pub static HOST_RECONNECT_GRACE_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// A duration override from one of the statics above, if set.
pub(crate) fn override_ms(v: &std::sync::atomic::AtomicU64) -> Option<std::time::Duration> {
    match v.load(std::sync::atomic::Ordering::SeqCst) {
        0 => None,
        ms => Some(std::time::Duration::from_millis(ms)),
    }
}
