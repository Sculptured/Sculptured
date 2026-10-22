//! `sculptured run <file>`: a team from one MADL file, run in the terminal with Practice agents (no
//! model), printing what happens and ending with an exit code that says how it went.
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn team(job: &str, budget: &str) -> String {
    format!(
        "#!/usr/bin/env -S sculptured run\nmadl: draft\nsystem: {{ id: hello, name: Hello{budget} }}\nagents:\n  - {{ id: writer, name: Writer, runtime: x-sculptured-practice, instructions: \"{job}\" }}\n"
    )
}

fn command(dir: &Path, file_text: &str, extra: &[&str]) -> Command {
    let file = dir.join("hello.madl");
    std::fs::write(&file, file_text).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_sculptured"));
    c.arg("run")
        .arg(&file)
        .arg("--folder")
        .arg(dir)
        .arg("--data-dir")
        .arg(dir.join("data"))
        .args(["--settle-secs", "2"])
        .args(extra)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    c
}

fn run(dir: &Path, file_text: &str, extra: &[&str]) -> Output {
    let mut child = command(dir, file_text, extra).spawn().unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(90) {
            // Asked to stop (not killed), so it stops its runner and agents before the test fails.
            // SAFETY: the unreaped child is owned by this test; POSIX signals require libc.
            unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
            let _ = child.wait();
            panic!("sculptured run did not finish");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    child.wait_with_output().unwrap()
}

/// Agents' folders must be inside the home folder.
fn home_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::var("HOME").unwrap()).unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn a_team_that_finishes_exits_0_and_says_what_happened() {
    let dir = home_dir();
    let o = run(dir.path(), &team("Say hello", ""), &[]);
    let out = text(&o);
    assert_eq!(o.status.code(), Some(0), "{out}");
    assert!(out.contains("Writer") && out.contains("finished"), "{out}");
}

#[test]
fn a_file_that_is_not_valid_madl_exits_4_with_its_errors() {
    let dir = home_dir();
    let o = run(dir.path(), "madl: draft\nagents: []\n", &[]);
    assert_eq!(o.status.code(), Some(4), "{}", text(&o));
    assert!(text(&o).contains("system"), "{}", text(&o));
}

#[test]
fn a_budget_may_only_be_lowered() {
    let dir = home_dir();
    let o = run(
        dir.path(),
        &team("Say hello", ", budget: { tokens: { hard: 1000 } }"),
        &["--tokens", "5000"],
    );
    assert_eq!(o.status.code(), Some(4), "{}", text(&o));
    assert!(text(&o).contains("only lower"), "{}", text(&o));
}

#[test]
fn a_question_with_no_one_at_the_terminal_exits_3() {
    let dir = home_dir();
    let o = run(dir.path(), &team("ask:Which colour?|Red|Blue", ""), &[]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("Which colour?"), "{}", text(&o));
}

#[test]
fn an_agent_that_fails_exits_1() {
    let dir = home_dir();
    let o = run(dir.path(), &team("fail", ""), &[]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
}

#[test]
fn ctrl_c_wraps_up_and_a_second_ctrl_c_stops_now() {
    let dir = home_dir();
    let mut child = command(dir.path(), &team("hang", ""), &[]).spawn().unwrap();
    let mut out = child.stdout.take().unwrap();
    let start = Instant::now();
    let mut seen = String::new();
    while !seen.contains("Writer: working") {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "never started: {seen}"
        );
        let mut buf = [0u8; 4096];
        let n = out.read(&mut buf).unwrap();
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    let pid = child.id() as i32;
    // SAFETY: this test owns the unreaped child and sends a valid signal, with no Rust pointers.
    unsafe { libc::kill(pid, libc::SIGINT) };
    std::thread::sleep(Duration::from_millis(500));
    // SAFETY: the child remains owned and unreaped; a second SIGINT exercises the force-stop path.
    unsafe { libc::kill(pid, libc::SIGINT) };
    let stopped = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            stopped.elapsed() < Duration::from_secs(20),
            "did not stop after a second Ctrl+C"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut rest = String::new();
    out.read_to_string(&mut rest).unwrap();
    assert!(
        format!("{seen}{rest}").contains("Wrapping up"),
        "{seen}{rest}"
    );
    assert_eq!(child.wait().unwrap().code(), Some(130));
}

#[test]
fn asked_to_terminate_it_stops_the_team_and_its_runner() {
    let dir = home_dir();
    let mut child = command(dir.path(), &team("hang", ""), &[]).spawn().unwrap();
    let mut out = child.stdout.take().unwrap();
    let (start, mut seen) = (Instant::now(), String::new());
    while !seen.contains("Writer: working") {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "never started: {seen}"
        );
        let mut buf = [0u8; 4096];
        let n = out.read(&mut buf).unwrap();
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    let runner: i32 = std::fs::read_to_string(dir.path().join("data/host-local/hostd.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: this is the unreaped fixture process owned by this test; kill takes no pointers.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let code = child.wait().unwrap().code();
    let gone = Instant::now();
    // SAFETY: signal 0 probes the recorded runner PID without sending a signal or using pointers.
    while unsafe { libc::kill(runner, 0) } == 0 {
        assert!(
            gone.elapsed() < Duration::from_secs(10),
            "its runner is still running"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(code, Some(130));
}
