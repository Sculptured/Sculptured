//! The e2e launcher leaves nothing behind: the throwaway directory is removed and hostd is
//! stopped. hostd is played by a copy of `sh` named `sculptured`, started on the session's
//! data dir like the real one, so the name-and-dir check is exercised for real.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use e2e_server::{alive, Session, HOME_FOLDERS};

/// One copy of `sh` named `sculptured`, made once for every test in this file.
fn fake_exe() -> &'static Path {
    static EXE: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();
    &EXE.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("sculptured");
        std::fs::copy("/bin/sh", &exe).expect("copy sh");
        (dir, exe)
    })
    .1
}

/// A long-running process whose command name is `sculptured` and whose command line names `dir`.
fn fake_sculptured(dir: &Path) -> Child {
    // A fork in another test thread can briefly hold the fresh copy open for writing (ETXTBSY).
    for _ in 0..100 {
        match Command::new(fake_exe()).args(["-c", "sleep 60; :", "hostd"]).arg(dir).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(c) => return c,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => panic!("spawn fake hostd: {e}"),
        }
    }
    panic!("spawn fake hostd: executable stayed busy")
}

/// Kills a test's helper process even when an assertion fails.
struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn sleeper() -> Child {
    Command::new("sleep").arg("60").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()
}

fn write_pid(session: &Session, pid: u32) {
    let file = session.hostd_pidfile();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, pid.to_string()).unwrap();
}

fn gone(child: &mut Child) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn a_new_session_has_a_throwaway_home_with_two_project_folders() {
    let session = Session::new().unwrap();
    for f in HOME_FOLDERS {
        assert!(session.home().join(f).is_dir(), "{f}");
    }
    let root = session.root().to_path_buf();
    drop(session);
    assert!(!root.exists(), "dropping the session removes its directory");
}

#[test]
fn shutdown_stops_serve_then_hostd_and_removes_the_directory() {
    let mut session = Session::new().unwrap();
    let root = session.root().to_path_buf();
    let serve = sleeper();
    let serve_pid = serve.id() as i32;
    session.adopt_serve(serve);
    let mut hostd = Reap(fake_sculptured(&session.data_dir().join("host-local")));
    write_pid(&session, hostd.0.id());

    session.shutdown();

    assert!(!alive(serve_pid), "serve is stopped and reaped");
    assert!(gone(&mut hostd.0), "hostd is stopped");
    assert!(!root.exists(), "the throwaway directory is removed");
}

#[test]
fn a_recorded_pid_that_is_not_this_sessions_sculptured_is_left_alone() {
    let elsewhere = tempfile::tempdir().unwrap();
    let mut session = Session::new().unwrap();
    // Another program, and a sculptured of another data dir: neither is touched.
    let mut other = Reap(sleeper());
    let mut foreign = Reap(fake_sculptured(elsewhere.path()));
    write_pid(&session, other.0.id());
    session.shutdown();
    let mut session2 = Session::new().unwrap();
    write_pid(&session2, foreign.0.id());
    session2.shutdown();

    assert!(matches!(other.0.try_wait(), Ok(None)), "an unrelated process keeps running");
    assert!(matches!(foreign.0.try_wait(), Ok(None)), "another session's sculptured keeps running");
}

#[test]
fn a_session_dropped_early_still_removes_its_directory() {
    // As when the web build fails before serve ever starts.
    let root = {
        let session = Session::new().unwrap();
        std::fs::write(session.root().join("partial-build"), "x").unwrap();
        session.root().to_path_buf()
    };
    assert!(!root.exists());
}

#[test]
fn shutdown_stops_the_community_stub_too() {
    let mut session = Session::new().unwrap();
    let stub = sleeper();
    let pid = stub.id() as i32;
    session.adopt_stub(stub);
    session.shutdown();
    assert!(!alive(pid), "the stub is stopped");
}
