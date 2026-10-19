//! Running a check command: `sh -c <command>` in a folder, with a time limit, keeping the end of the
//! output. Who may run it, and recording the result, is the caller's business.
use std::time::Duration;
use tokio::io::AsyncReadExt;

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

/// (passed, exit code, timed out, the end of stdout then stderr).
pub async fn execute(cmd: &str, cwd: &str) -> CheckOutcome {
    execute_within(cmd, cwd, TIMEOUT).await
}

/// `execute` with its own time limit (a workflow check stage's `timeout_s`).
pub async fn execute_within(cmd: &str, cwd: &str, limit: Duration) -> CheckOutcome {
    let (passed, exit_code, timed_out, output) = run(cmd, cwd, limit).await;
    CheckOutcome { passed, exit_code, timed_out, output }
}

/// The check as a shell command line: `sh -c` on Unix, `cmd /C` on Windows (no console window).
pub(crate) fn shell(cmd: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").raw_arg(cmd).creation_flags(0x0800_0000);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    }
}

async fn run(cmd: &str, cwd: &str, limit: Duration) -> (bool, Option<i32>, bool, String) {
    let child = shell(cmd)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (false, None, false, format!("Could not run the check in {cwd}: {e}")),
    };
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let work = async {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let (_, _, status) = tokio::join!(out.read_to_end(&mut o), err.read_to_end(&mut e), child.wait());
        (status, o, e)
    };
    match tokio::time::timeout(limit, work).await {
        Ok((status, o, e)) => {
            let code = status.ok().and_then(|s| s.code());
            let mut text = String::from_utf8_lossy(&o).into_owned();
            if !e.is_empty() {
                text.push_str(&String::from_utf8_lossy(&e));
            }
            (code == Some(0), code, false, tail(&text))
        }
        Err(_) => (false, None, true, format!("Stopped after {} s without finishing.", limit.as_secs())),
    }
}

/// One run at a time per key (an agent's check); asking while one runs makes it run once more afterwards
/// (a turn that ended meanwhile), never more.
#[derive(Default)]
pub struct Rerun(std::sync::Mutex<std::collections::HashMap<String, bool>>);

impl Rerun {
    /// True when nothing runs for `key`: the caller starts the run. False: one more run is queued.
    pub fn start(&self, key: &str) -> bool {
        let mut r = self.0.lock().unwrap();
        match r.get_mut(key) {
            Some(again) => { *again = true; false }
            None => { r.insert(key.to_string(), false); true }
        }
    }

    /// After a run: true to run once more; false ends it.
    pub fn again(&self, key: &str) -> bool {
        let mut r = self.0.lock().unwrap();
        if r.get(key) == Some(&true) { r.insert(key.to_string(), false); true } else { r.remove(key); false }
    }
}

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
        assert_eq!(run("echo ok", d, TIMEOUT).await, (true, Some(0), false, "ok".into()));
        let (passed, code, _, out) = run("echo out; echo bad >&2; exit 3", d, TIMEOUT).await;
        assert_eq!((passed, code, out.as_str()), (false, Some(3), "out\nbad"));
        let long = tail(&"x".repeat(KEEP + 50));
        assert!(long.starts_with('…') && long.chars().count() == KEEP + 1);
    }
}
