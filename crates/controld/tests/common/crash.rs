//! Crashing a live agent: kills the real CLI process the runner started for a seat, and everything it
//! started, as a crash would end them. Only processes descended from this test process are touched.
#![allow(dead_code)]
use hostd::supervisor::Supervisor;

fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // `pid (name) state ppid ...`: the name may hold spaces, so read after its last ')'.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn descends_from(pid: u32, ancestor: u32) -> bool {
    let mut p = pid;
    for _ in 0..64 {
        match parent_of(p) {
            Some(q) if q == ancestor => return true,
            Some(q) if q > 1 => p = q,
            _ => return false,
        }
    }
    false
}

fn processes() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .map(|d| {
            d.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Kills `seat`'s CLI process (as `runner` has it on record) with SIGKILL, then what it started.
pub fn kill_agent(runner: &Supervisor, seat: &str) {
    let pid = runner
        .recorded_processes()
        .into_iter()
        .find(|(s, ..)| s == seat)
        .map(|(_, pid, _)| pid)
        .expect("the agent's process is on record");
    assert!(
        descends_from(pid, std::process::id()),
        "process {pid} was not started by this test"
    );
    let children: Vec<u32> = processes()
        .into_iter()
        .filter(|p| descends_from(*p, pid))
        .collect();
    for p in std::iter::once(pid).chain(children) {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &p.to_string()])
            .status();
    }
}
