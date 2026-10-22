//! `sculptured stop`: ends every agent this computer's runner started, at once and without the
//! server, like Ctrl+C: each agent's own process group is interrupted, then forced after 2 seconds.
//! Only a process whose identity still matches what the runner recorded at launch is signalled.
use crate::journal::Journal;
use std::path::Path;
use std::time::{Duration, Instant};

/// What tells this process apart from a later one that reuses its pid: its start time (Unix), or its
/// program name (Windows).
#[must_use]
pub fn identity(pid: u32) -> Option<String> {
    #[cfg(unix)]
    let out = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    #[cfg(windows)]
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    #[cfg(windows)]
    let text = text
        .split(',')
        .next()
        .unwrap_or("")
        .trim_matches('"')
        .to_string();
    (out.status.success() && !text.is_empty() && !text.starts_with("INFO:")).then_some(text)
}

/// Ends the recorded agents of the runner whose data folder is `data_dir`; returns the seats ended.
pub fn run(data_dir: &Path) -> Result<Vec<String>, String> {
    let path = data_dir.join("hostd").join("hostd.sqlite");
    if !path.exists() {
        return Err(format!(
            "No runner data in {}; pass the --data-dir you connected with",
            data_dir.display()
        ));
    }
    let mut journal =
        Journal::open(&path, 1).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut ended = Vec::new();
    for (seat, pid, ident) in journal.processes().map_err(|e| e.to_string())? {
        let same = ident.is_some() && identity(pid) == ident;
        if same {
            end_group(pid);
            ended.push(seat.clone());
        }
        // Gone or not ours: either way the record is spent.
        let _ = journal.clear_process(&seat);
    }
    Ok(ended)
}

#[cfg(unix)]
fn end_group(pid: u32) {
    // SAFETY: signal 0 only probes the recorded PID; kill takes no Rust pointers.
    // POSIX process-group signalling has no equivalent in the standard library.
    let alive = || unsafe { libc::kill(pid as i32, 0) == 0 };
    // The group the driver made for this agent alone: the CLI and the commands it started.
    // SAFETY: this verified process group belongs to the agent, and SIGINT is a valid signal.
    unsafe { libc::killpg(pid as i32, libc::SIGINT) };
    let deadline = Instant::now() + Duration::from_secs(2);
    while alive() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if alive() {
        // SAFETY: the same recorded process group is still alive; SIGKILL is a valid signal.
        unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
    }
}

#[cfg(windows)]
fn end_group(pid: u32) {
    let _ = (Instant::now(), Duration::ZERO);
    let _ = std::process::Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .status();
}
