//! `sculptured stop`: the computer ends the agents its runner started, without the server.
#![cfg(unix)]
use hostd::journal::Journal;
use hostd::local_stop;
use std::os::unix::process::CommandExt;
use std::time::{Duration, Instant};

fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks the fixture PID; kill takes no pointers. The standard
    // library cannot probe an arbitrary recorded PID. Zombies are reaped by their parent's wait.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// A process in a group of its own, as a driver starts an agent.
fn agent_like() -> std::process::Child {
    std::process::Command::new("sleep")
        .arg("60")
        .process_group(0)
        .spawn()
        .unwrap()
}

#[test]
fn the_agents_the_runner_recorded_are_ended_and_a_reused_pid_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("hostd")).unwrap();
    let mut j = Journal::open(&dir.path().join("hostd").join("hostd.sqlite"), 100).unwrap();
    let mut agent = agent_like();
    let mut other = agent_like();
    let pid = agent.id();
    j.record_process("dev.a@t", pid, local_stop::identity(pid).as_deref())
        .unwrap();
    // A record whose process is gone: its pid now belongs to something else (here: a different start).
    j.record_process(
        "dev.b@t",
        other.id(),
        Some("not the process that was recorded"),
    )
    .unwrap();
    drop(j);
    let stopped = local_stop::run(dir.path()).unwrap();
    assert_eq!(stopped, ["dev.a@t"]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while agent.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the agent's group was ended");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        alive(other.id()),
        "a process the runner did not start is never signalled"
    );
    other.kill().unwrap();
    let _ = other.wait();
}

#[test]
fn a_record_is_kept_until_the_agent_exits() {
    let mut j = Journal::open_in_memory(100).unwrap();
    j.record_process("dev.a@t", 4242, Some("Thu Oct 16 01:00:00 2026"))
        .unwrap();
    assert_eq!(
        j.processes().unwrap(),
        [(
            "dev.a@t".to_string(),
            4242,
            Some("Thu Oct 16 01:00:00 2026".to_string())
        )]
    );
    j.clear_process("dev.a@t").unwrap();
    assert!(j.processes().unwrap().is_empty());
}
