//! Runs the real `sculptured` binary for the Playwright end-to-end tests, on a throwaway
//! data dir and a throwaway `HOME`, and cleans up after itself.
//!
//! `sculptured serve` spawns `sculptured hostd` in its own session, so hostd outlives serve
//! and is not reached by a signal to serve's process group. [`Session::shutdown`] (also run on
//! drop) stops serve, then stops hostd by the pid it recorded, and only if that pid is still a
//! `sculptured` process started on this session's data dir. Dropping the session removes the
//! throwaway directory, so a failed build, a signal or a panic never leaves it behind.

use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

/// The folders the throwaway home holds: the folder picker shows these and nothing else.
pub const HOME_FOLDERS: [&str; 2] = ["projects/alpha", "projects/beta"];

pub struct Session {
    tmp: Option<tempfile::TempDir>,
    serve: Option<Child>,
    /// `fake-community`, the stub Sculptured server Community goes through in e2e.
    stub: Option<Child>,
}

impl Session {
    /// A new throwaway directory holding `data/`, `home/` (with [`HOME_FOLDERS`]) and `web-dist/`.
    pub fn new() -> std::io::Result<Self> {
        let tmp = tempfile::Builder::new().prefix("sculptured-e2e-").tempdir()?;
        for f in HOME_FOLDERS {
            std::fs::create_dir_all(tmp.path().join("home").join(f))?;
        }
        Ok(Self { tmp: Some(tmp), serve: None, stub: None })
    }

    pub fn root(&self) -> &Path {
        self.tmp.as_ref().expect("session is open").path()
    }
    pub fn data_dir(&self) -> PathBuf {
        self.root().join("data")
    }
    pub fn home(&self) -> PathBuf {
        self.root().join("home")
    }
    pub fn web_dist(&self) -> PathBuf {
        self.root().join("web-dist")
    }
    /// Where hostd records its pid (`{data}/host-local/hostd.pid`).
    pub fn hostd_pidfile(&self) -> PathBuf {
        self.data_dir().join("host-local").join("hostd.pid")
    }

    /// Hands the running serve process to the session, which stops it on shutdown.
    pub fn adopt_serve(&mut self, child: Child) {
        self.serve = Some(child);
    }
    pub fn serve_mut(&mut self) -> Option<&mut Child> {
        self.serve.as_mut()
    }
    /// Hands the running `fake-community` to the session, which stops it on shutdown.
    pub fn adopt_stub(&mut self, child: Child) {
        self.stub = Some(child);
    }
    pub fn stub_mut(&mut self) -> Option<&mut Child> {
        self.stub.as_mut()
    }

    /// Stops serve (SIGTERM, then SIGKILL after 10 s), then hostd, then the Community stub
    /// (SIGTERM, then SIGKILL after 5 s), then removes the directory. Safe to call more than once.
    pub fn shutdown(&mut self) {
        if let Some(child) = self.serve.take() {
            stop_child(child, Duration::from_secs(10));
        }
        if let Some(tmp) = self.tmp.as_ref() {
            stop_hostd(&tmp.path().join("data").join("host-local").join("hostd.pid"), tmp.path());
        }
        if let Some(child) = self.stub.take() {
            stop_child(child, Duration::from_secs(5));
        }
        if let Some(tmp) = self.tmp.take()
            && let Err(e) = tmp.close()
        {
            eprintln!("e2e-server: could not remove the throwaway directory: {e}");
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A child this session started: SIGTERM to its own pid, then SIGKILL after `grace`.
fn stop_child(mut child: Child, grace: Duration) {
    if matches!(child.try_wait(), Ok(None)) {
        signal(child.id() as i32, libc::SIGTERM);
        let deadline = Instant::now() + grace;
        while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn signal(pid: i32, sig: i32) {
    // SAFETY: kill(2) with a pid this module verified or owns.
    unsafe {
        libc::kill(pid, sig);
    }
}

fn proc_file(pid: i32, name: &str) -> Option<Vec<u8>> {
    std::fs::read(format!("/proc/{pid}/{name}")).ok()
}

/// True while `pid` exists and is not a zombie.
pub fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return false;
    }
    match proc_file(pid, "stat") {
        Some(stat) => {
            let s = String::from_utf8_lossy(&stat);
            // The state follows the parenthesised command name.
            !s.rsplit_once(')').is_some_and(|(_, rest)| rest.trim_start().starts_with('Z'))
        }
        None => true,
    }
}

/// True when `pid` is a `sculptured` process started on `session_dir` (so a reused pid is never
/// killed). Without `/proc`, the command name alone is checked with `ps`.
pub fn is_session_sculptured(pid: i32, session_dir: &Path) -> bool {
    if let (Some(comm), Some(cmdline)) = (proc_file(pid, "comm"), proc_file(pid, "cmdline")) {
        let comm = String::from_utf8_lossy(&comm);
        let dir = session_dir.to_string_lossy();
        return comm.trim() == "sculptured" && cmdline.split(|b| *b == 0).any(|arg| String::from_utf8_lossy(arg).contains(dir.as_ref()));
    }
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().ends_with("sculptured"))
        .unwrap_or(false)
}

/// Stops the hostd recorded in `pidfile` if it is this session's sculptured: SIGTERM, then
/// SIGKILL after 5 s. Returns the pid it stopped.
pub fn stop_hostd(pidfile: &Path, session_dir: &Path) -> Option<i32> {
    let text = std::fs::read_to_string(pidfile).ok()?;
    let pid: i32 = text.trim().parse().ok()?;
    if pid <= 1 || !alive(pid) || !is_session_sculptured(pid, session_dir) {
        return None;
    }
    signal(pid, libc::SIGTERM);
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if alive(pid) {
        signal(pid, libc::SIGKILL);
    }
    Some(pid)
}
