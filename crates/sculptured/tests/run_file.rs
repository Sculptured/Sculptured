//! `sculptured run <file>`: a team from one MADL file, run in the terminal, printing what happens and
//! ending with an exit code that says how it went. Runs with agents are live: real Claude Code on
//! haiku (ignored by default; they need `claude` installed and signed in). Each run gets its own
//! `HOME` and data dir, so it never reaches `~/.sculptured`; only `CLAUDE_CONFIG_DIR` points at the
//! person's Claude Code setup, so the agents are signed in.
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// A one-agent team on Claude Code with `model`.
fn team_on(model: &str, job: &str, budget: &str) -> String {
    format!(
        "#!/usr/bin/env -S sculptured run\nmadl: draft\nsystem: {{ id: hello, name: Hello{budget} }}\nagents:\n  - {{ id: writer, name: Writer, runtime: claude-code, model: {model}, instructions: \"{job}\" }}\n"
    )
}

/// A one-agent team on the cheapest Claude model.
fn team(job: &str, budget: &str) -> String {
    team_on("haiku", job, budget)
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// The `claude` CLI, or None (with the reason printed) when this machine cannot run live tests.
fn require() -> Option<PathBuf> {
    let found = on_path("claude");
    if found.is_none() {
        eprintln!(
            "skipped: the claude CLI is not on PATH; live tests need it installed and signed in"
        );
    }
    found
}

/// The person's Claude Code setup (`$CLAUDE_CONFIG_DIR`, else `~/.claude`), when there is one.
fn claude_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".claude")))
        .filter(|d| d.is_dir())
}

/// `dir` is the run's throwaway `HOME`; the agents work in `dir/work` (folders must be inside home).
fn command(dir: &Path, file_text: &str, extra: &[&str]) -> Command {
    let file = dir.join("hello.madl");
    std::fs::write(&file, file_text).unwrap();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_sculptured"));
    c.env("HOME", dir).env_remove("CLAUDE_CONFIG_DIR");
    if let Some(d) = claude_config_dir() {
        c.env("CLAUDE_CONFIG_DIR", d);
    }
    c.arg("run")
        .arg(&file)
        .arg("--folder")
        .arg(&work)
        .arg("--data-dir")
        .arg(dir.join("data"))
        .args(["--settle-secs", "2"]);
    if let Some(claude) = on_path("claude") {
        c.arg("--claude-program").arg(claude);
    }
    c.args(extra)
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
        if start.elapsed() > Duration::from_secs(240) {
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

/// The run's throwaway home folder.
fn home_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
fn live_a_team_that_finishes_exits_0_and_says_what_happened() {
    let Some(_) = require() else { return };
    let dir = home_dir();
    let o = run(
        dir.path(),
        &team("Reply with the single word OK and nothing else.", ""),
        &[],
    );
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

/// A file with a scripted, model-less agent (older Sculptured files wrote `x-sculptured-practice`) is
/// refused: every agent needs a real model.
#[test]
fn a_model_less_agent_is_refused_and_exits_4() {
    let dir = home_dir();
    let file = "madl: draft\nsystem: { id: hello, name: Hello }\nagents:\n  - { id: writer, name: Writer, runtime: x-sculptured-practice, instructions: \"Say hello\" }\n";
    let o = run(dir.path(), file, &[]);
    assert_eq!(o.status.code(), Some(4), "{}", text(&o));
    assert!(text(&o).contains("/agents/0/runtime"), "{}", text(&o));
    assert!(text(&o).contains("real model"), "{}", text(&o));
}

/// Refused before the team starts, so no agent runs.
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

const ASK: &str = "Use your request_input tool to ask the person exactly this question: Which colour? Offer the options Red and Blue. Do nothing else.";
const HANG: &str = "Use your Bash tool to run exactly this command and wait for it: sleep 600";

#[test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
fn live_a_question_with_no_one_at_the_terminal_exits_3() {
    let Some(_) = require() else { return };
    let dir = home_dir();
    let o = run(dir.path(), &team(ASK, ""), &[]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
}

/// The CLI itself fails: the model it is asked for does not exist.
#[test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
fn live_an_agent_that_fails_exits_1() {
    let Some(_) = require() else { return };
    let dir = home_dir();
    let o = run(
        dir.path(),
        &team_on("no-such-model-xyz", "Reply with the single word OK.", ""),
        &[],
    );
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
}

#[test]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
fn live_ctrl_c_wraps_up_and_a_second_ctrl_c_stops_now() {
    let Some(_) = require() else { return };
    let dir = home_dir();
    let mut child = command(dir.path(), &team(HANG, ""), &[]).spawn().unwrap();
    let mut out = child.stdout.take().unwrap();
    let start = Instant::now();
    let mut seen = String::new();
    while !seen.contains("Writer: working") {
        assert!(
            start.elapsed() < Duration::from_secs(240),
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
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
fn live_asked_to_terminate_it_stops_the_team_and_its_runner() {
    let Some(_) = require() else { return };
    let dir = home_dir();
    let mut child = command(dir.path(), &team(HANG, ""), &[]).spawn().unwrap();
    let mut out = child.stdout.take().unwrap();
    let (start, mut seen) = (Instant::now(), String::new());
    while !seen.contains("Writer: working") {
        assert!(
            start.elapsed() < Duration::from_secs(240),
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
