//! The e2e launcher leaves nothing behind: the throwaway directory is removed and hostd is
//! stopped. hostd is played by a copy of `sh` named `sculptured`, started on the session's
//! data dir like the real one, so the name-and-dir check is exercised for real. It also keeps away
//! from the person's own Sculptured (ports, home) while letting agents sign in to Claude Code and
//! Codex.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use e2e_server::{HOME_FOLDERS, Session};

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
        match Command::new(fake_exe())
            .args(["-c", "sleep 60; :", "hostd"])
            .arg(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => return c,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(20))
            }
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
    Command::new("sleep")
        .arg("60")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
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
    let session = Session::try_new().unwrap();
    for f in HOME_FOLDERS {
        assert!(session.home().join(f).is_dir(), "{f}");
    }
    let root = session.root().to_path_buf();
    drop(session);
    assert!(!root.exists(), "dropping the session removes its directory");
}

#[test]
fn shutdown_stops_serve_then_hostd_and_removes_the_directory() {
    let mut session = Session::try_new().unwrap();
    let root = session.root().to_path_buf();
    let serve = sleeper();
    let serve_pid = serve.id() as i32;
    session.adopt_serve(serve);
    let mut hostd = Reap(fake_sculptured(&session.data_dir().join("host-local")));
    write_pid(&session, hostd.0.id());

    session.shutdown();

    assert!(!e2e_server::alive(serve_pid), "serve is stopped and reaped");
    assert!(gone(&mut hostd.0), "hostd is stopped");
    assert!(!root.exists(), "the throwaway directory is removed");
}

#[test]
fn a_recorded_pid_that_is_not_this_sessions_sculptured_is_left_alone() {
    let elsewhere = tempfile::tempdir().unwrap();
    let mut session = Session::try_new().unwrap();
    // Another program, and a sculptured of another data dir: neither is touched.
    let mut other = Reap(sleeper());
    let mut foreign = Reap(fake_sculptured(elsewhere.path()));
    write_pid(&session, other.0.id());
    session.shutdown();
    let mut session2 = Session::try_new().unwrap();
    write_pid(&session2, foreign.0.id());
    session2.shutdown();

    assert!(
        matches!(other.0.try_wait(), Ok(None)),
        "an unrelated process keeps running"
    );
    assert!(
        matches!(foreign.0.try_wait(), Ok(None)),
        "another session's sculptured keeps running"
    );
}

#[test]
fn a_session_dropped_early_still_removes_its_directory() {
    // As when the web build fails before serve ever starts.
    let root = {
        let session = Session::try_new().unwrap();
        std::fs::write(session.root().join("partial-build"), "x").unwrap();
        session.root().to_path_buf()
    };
    assert!(!root.exists());
}

#[test]
fn shutdown_stops_the_community_stub_too() {
    let mut session = Session::try_new().unwrap();
    let stub = sleeper();
    let pid = stub.id() as i32;
    session.adopt_stub(stub);
    session.shutdown();
    assert!(!e2e_server::alive(pid), "the stub is stopped");
}

#[test]
fn the_ports_of_the_persons_own_sculptured_are_never_used() {
    use e2e_server::usable_port;
    assert_eq!(usable_port("4410"), None);
    assert_eq!(usable_port("4420"), None);
    assert_eq!(usable_port("0"), None);
    assert_eq!(usable_port("not a port"), None);
    assert_eq!(usable_port("14410"), Some(14410));
}

#[test]
fn agents_sign_in_with_the_persons_claude_code_setup_not_the_throwaway_home() {
    use e2e_server::claude_config_dir;
    let real = tempfile::tempdir().unwrap();
    assert_eq!(
        claude_config_dir(None, Some(real.path())),
        None,
        "no setup, nothing to point at"
    );
    std::fs::create_dir(real.path().join(".claude")).unwrap();
    assert_eq!(
        claude_config_dir(None, Some(real.path())),
        Some(real.path().join(".claude"))
    );
    let given = tempfile::tempdir().unwrap();
    assert_eq!(
        claude_config_dir(Some(given.path().as_os_str()), Some(real.path())),
        Some(given.path().to_path_buf()),
        "CLAUDE_CONFIG_DIR as the person set it wins"
    );
}

#[test]
fn the_claude_cli_is_found_on_path_or_in_the_real_home() {
    use e2e_server::find_claude;
    let bin = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    assert_eq!(
        find_claude(None, Some(bin.path().as_os_str()), Some(real.path())),
        None
    );
    std::fs::create_dir_all(real.path().join(".local/bin")).unwrap();
    std::fs::write(real.path().join(".local/bin/claude"), "").unwrap();
    assert_eq!(
        find_claude(None, Some(bin.path().as_os_str()), Some(real.path())),
        Some(real.path().join(".local/bin/claude")),
        "the real home's ~/.local/bin, which the throwaway HOME hides"
    );
    std::fs::write(bin.path().join("claude"), "").unwrap();
    assert_eq!(
        find_claude(None, Some(bin.path().as_os_str()), Some(real.path())),
        Some(bin.path().join("claude")),
        "PATH first"
    );
    let given = bin.path().join("my-claude");
    assert_eq!(
        find_claude(Some(given.as_os_str()), None, None),
        None,
        "a given program that is not there"
    );
    std::fs::write(&given, "").unwrap();
    assert_eq!(
        find_claude(Some(given.as_os_str()), None, None),
        Some(given)
    );
}

#[test]
fn codex_agents_sign_in_with_the_persons_codex_setup_not_the_throwaway_home() {
    use e2e_server::codex_home;
    let real = tempfile::tempdir().unwrap();
    assert_eq!(codex_home(None, Some(real.path())), None, "no setup");
    std::fs::create_dir(real.path().join(".codex")).unwrap();
    assert_eq!(
        codex_home(None, Some(real.path())),
        Some(real.path().join(".codex"))
    );
    let given = tempfile::tempdir().unwrap();
    assert_eq!(
        codex_home(Some(given.path().as_os_str()), Some(real.path())),
        Some(given.path().to_path_buf()),
        "CODEX_HOME as the person set it wins"
    );
}

#[test]
fn the_codex_cli_is_found_on_path_or_in_the_real_home() {
    use e2e_server::find_codex;
    let bin = tempfile::tempdir().unwrap();
    let real = tempfile::tempdir().unwrap();
    assert_eq!(
        find_codex(None, Some(bin.path().as_os_str()), Some(real.path())),
        None
    );
    std::fs::create_dir_all(real.path().join(".local/bin")).unwrap();
    std::fs::write(real.path().join(".local/bin/codex"), "").unwrap();
    assert_eq!(
        find_codex(None, Some(bin.path().as_os_str()), Some(real.path())),
        Some(real.path().join(".local/bin/codex"))
    );
    std::fs::write(bin.path().join("codex"), "").unwrap();
    assert_eq!(
        find_codex(None, Some(bin.path().as_os_str()), Some(real.path())),
        Some(bin.path().join("codex")),
        "PATH first"
    );
    let given = bin.path().join("my-codex");
    std::fs::write(&given, "").unwrap();
    assert_eq!(find_codex(Some(given.as_os_str()), None, None), Some(given));
}
