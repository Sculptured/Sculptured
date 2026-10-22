//! Running a check command: `sh -c <command>` in a folder, with a time limit, keeping the end of the
//! output. Who may run it, and recording the result, is the caller's business.
use crate::process;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Child;

/// A check still running after this long is stopped and counts as failed.
pub const TIMEOUT: Duration = Duration::from_secs(600);
/// How much of the end of the output is kept.
pub const KEEP: usize = 8000;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CheckOutcome {
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output: String,
}

/// Runs a check and keeps the end of stdout followed by stderr.
pub async fn execute(cmd: &str, cwd: &str) -> CheckOutcome {
    execute_within(cmd, cwd, TIMEOUT).await
}

/// `execute` with its own time limit (a workflow check stage's `timeout_s`).
pub async fn execute_within(cmd: &str, cwd: &str, limit: Duration) -> CheckOutcome {
    let mut child = match process::spawn(cmd, cwd) {
        Ok(child) => child,
        Err(error) => {
            return CheckOutcome::failed(
                format!("Could not run the check in {cwd}: {error}"),
                false,
            );
        }
    };
    match tokio::time::timeout(limit, collect(&mut child)).await {
        Ok(outcome) => outcome,
        Err(_) => CheckOutcome::failed(process::timeout_message(limit), true),
    }
}

impl CheckOutcome {
    fn failed(output: String, timed_out: bool) -> Self {
        Self {
            passed: false,
            exit_code: None,
            timed_out,
            output,
        }
    }
}

async fn collect(child: &mut Child) -> CheckOutcome {
    let mut stdout = child.stdout.take().expect("check stdout is piped");
    let mut stderr = child.stderr.take().expect("check stderr is piped");
    let (mut output, mut errors) = (Vec::new(), Vec::new());
    let (_, _, status) = tokio::join!(
        stdout.read_to_end(&mut output),
        stderr.read_to_end(&mut errors),
        child.wait()
    );
    let exit_code = status.ok().and_then(|status| status.code());
    let mut text = String::from_utf8_lossy(&output).into_owned();
    text.push_str(&String::from_utf8_lossy(&errors));
    CheckOutcome {
        passed: exit_code == Some(0),
        exit_code,
        timed_out: false,
        output: tail(&text),
    }
}

/// One run at a time per key (an agent's check); asking while one runs makes it run once more afterwards
/// (a turn that ended meanwhile), never more.
#[derive(Default)]
pub struct Rerun(Mutex<HashMap<String, bool>>);

impl Rerun {
    /// True when nothing runs for `key`: the caller starts the run. False: one more run is queued.
    pub fn start(&self, key: &str) -> bool {
        let mut r = self.0.lock().expect("rerun state mutex is not poisoned");
        match r.get_mut(key) {
            Some(again) => {
                *again = true;
                false
            }
            None => {
                r.insert(key.to_string(), false);
                true
            }
        }
    }

    /// After a run: true to run once more; false ends it.
    pub fn again(&self, key: &str) -> bool {
        let mut r = self.0.lock().expect("rerun state mutex is not poisoned");
        if let Some(again) = r.get_mut(key)
            && *again
        {
            *again = false;
            true
        } else {
            r.remove(key);
            false
        }
    }
}

#[must_use]
pub fn tail(s: &str) -> String {
    let s = s.trim_end();
    if s.len() <= KEEP {
        return s.to_string();
    }
    format!("…{}", model::cap::tail(s, KEEP))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rerun_runs_once_more_when_asked_during_a_run_and_never_twice_more() {
        let r = Rerun::default();
        assert!(r.start("a"), "nothing ran: start");
        assert!(!r.start("a") && !r.start("a"), "asked twice while running");
        assert!(r.again("a"), "one more run");
        assert!(!r.again("a"), "then done");
        assert!(r.start("a"), "free again");
    }

    #[tokio::test]
    async fn a_check_is_stopped_at_its_own_time_limit() {
        let out = execute_within("sleep 5", "/", std::time::Duration::from_millis(200)).await;
        assert!(!out.passed && out.timed_out);
    }

    #[tokio::test]
    async fn a_check_passes_on_exit_zero_and_keeps_the_end_of_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_str().unwrap();
        assert_eq!(
            execute("echo ok", d).await,
            CheckOutcome {
                passed: true,
                exit_code: Some(0),
                timed_out: false,
                output: "ok".into()
            }
        );
        let out = execute("echo out; echo bad >&2; exit 3", d).await;
        assert_eq!(
            (out.passed, out.exit_code, out.output.as_str()),
            (false, Some(3), "out\nbad")
        );
        let long = tail(&"x".repeat(KEEP + 50));
        assert!(long.starts_with('…') && long.chars().count() == KEEP + 1);
    }
}
