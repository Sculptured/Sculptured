//! Starts the real binary (controld + spawned hostd) on a throwaway data dir, `HOME` and port, and
//! drives it over HTTP; the agent tests run the real Claude Code CLI (live, ignored by default).
//! Nothing here can reach `~/.sculptured` or the default ports: every serve gets `--data-dir`,
//! `--port 0` and its own `HOME`, and only `CLAUDE_CONFIG_DIR` points at the person's Claude Code
//! setup, so a live agent is signed in.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const SPEC: &str = r#"
version: "0.2"
name: smoke
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:a", profile: default, runtime: claude-code, model: haiku, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: claude-code, model: haiku, cwd: "." }
    edges: [{ kind: delegates_to, from: a, to: b }]
"#;

/// The ports a person's own Sculptured uses; no test may touch them.
const THEIR_PORTS: [u16; 2] = [4410, 4420];

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

/// A running `sculptured serve`. On drop, kills serve's process group and the hostd
/// recorded in `{data}/host-local/hostd.pid` (hostd runs in its own session).
struct Serve {
    child: std::process::Child,
    data: PathBuf,
    base: String,
    boot: Duration,
    _home: tempfile::TempDir,
}

impl Serve {
    fn start(data: &Path, port: u16) -> Serve {
        Serve::start_with(data, port, &[])
    }

    fn start_with(data: &Path, port: u16, extra: &[&str]) -> Serve {
        assert!(!THEIR_PORTS.contains(&port), "port {port} is not for tests");
        let home = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sculptured"));
        cmd.env("HOME", home.path())
            .env_remove("SCULPTURED_DATA_DIR")
            .env_remove("CLAUDE_CONFIG_DIR");
        if let Some(dir) = claude_config_dir() {
            cmd.env("CLAUDE_CONFIG_DIR", dir);
        }
        let mut child = cmd
            .args([
                "serve",
                "--port",
                &port.to_string(),
                "--web-dir",
                "/nonexistent",
            ])
            .arg("--data-dir")
            .arg(data)
            .args(extra)
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let mut serve = Serve {
            base: String::new(),
            boot: Duration::ZERO,
            data: data.to_path_buf(),
            child,
            _home: home,
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(out).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("serve printed no listening line within 30s");
        serve.base = line
            .trim()
            .strip_prefix("listening on ")
            .expect("listening line")
            .to_string();
        serve.boot = started.elapsed();
        let port = serve
            .base
            .rsplit(':')
            .next()
            .and_then(|p| p.parse::<u16>().ok());
        assert!(
            port.is_some_and(|p| !THEIR_PORTS.contains(&p)),
            "serve listens on {}",
            serve.base
        );
        serve
    }

    fn pgid(&self) -> i32 {
        self.child.id() as i32
    }

    fn hostd_pid(&self) -> Option<i32> {
        std::fs::read_to_string(self.data.join("host-local/hostd.pid"))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        // Only signal the group while serve is still unreaped (its pid cannot have been reused).
        if matches!(self.child.try_wait(), Ok(None)) {
            // SAFETY: signalling the process group this test created.
            unsafe {
                libc::killpg(self.pgid(), libc::SIGKILL);
            }
        }
        // hostd.pid may be stale: kill only a pid that is still a sculptured of this test.
        if let Some(pid) = self.hostd_pid()
            && sculptured_procs(&self.data).contains(&pid)
        {
            // SAFETY: pid verified above as this test's hostd.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        let _ = self.child.wait();
    }
}

fn is_zombie(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| s.contains(") Z "))
        .unwrap_or(true)
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks existence.
    unsafe { libc::kill(pid, 0) == 0 && !is_zombie(pid) }
}

/// Pids of live sculptured processes whose command line mentions `data`.
fn sculptured_procs(data: &Path) -> Vec<i32> {
    let needle = data.to_string_lossy().to_string();
    let mut found = vec![];
    let flattened_items = std::fs::read_dir("/proc").unwrap().flatten();
    for entry in flattened_items {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(cmd) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let cmd = String::from_utf8_lossy(&cmd).replace('\0', " ");
        if cmd.contains("sculptured") && cmd.contains(&needle) && alive(pid) {
            found.push(pid);
        }
    }
    found
}

fn wait_gone(data: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !sculptured_procs(data).is_empty() {
        assert!(
            Instant::now() < deadline,
            "sculptured processes left: {:?}",
            sculptured_procs(data)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

async fn poll(what: &str, f: impl AsyncFnMut() -> bool) {
    poll_for(what, Duration::from_secs(20), f).await;
}

async fn poll_for(what: &str, wait: Duration, mut f: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if f().await {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Live: two Claude Code agents on haiku, working in a throwaway folder.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_serve_boots_messages_and_stops_a_rig() {
    let Some(claude) = require() else { return };
    let data = tempfile::tempdir().unwrap();
    let specs = tempfile::tempdir().unwrap();
    let spec_path = specs.path().join("smoke.yaml");
    std::fs::write(&spec_path, SPEC).unwrap();

    let serve = Serve::start_with(
        data.path(),
        0,
        &["--claude-program", claude.to_str().unwrap()],
    );
    let base = serve.base.clone();
    let http = client();

    let boot: Value = http
        .post(format!("{base}/api/rigs"))
        .json(&json!({ "request_id": "boot-1", "spec_path": spec_path }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(boot["rig"], "smoke", "{boot}");

    let lifecycles = |http: reqwest::Client, base: String| async move {
        let snap: Value = http
            .get(format!("{base}/api/rigs/smoke"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        snap["seats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["lifecycle"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    poll_for("both seats running", Duration::from_secs(60), async || {
        lifecycles(http.clone(), base.clone()).await == ["running", "running"]
    })
    .await;

    let sent: Value = http
        .post(format!("{base}/api/seats/dev.a@smoke/send"))
        .json(&json!({ "request_id": "send-1", "text": "Reply with the single word OK and nothing else.", "intent": "inform", "interrupt": false }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(sent["message_id"].is_string(), "{sent}");
    poll_for("the agent's reply", Duration::from_secs(240), async || {
        let events: Value = http
            .get(format!(
                "{base}/api/seats/dev.a@smoke/events?after=0&limit=500"
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        events.as_array().unwrap().iter().any(|e| {
            e["body"]["type"] == "agent-text"
                && e["body"]["text"].as_str().is_some_and(|t| !t.is_empty())
        })
    })
    .await;

    let stop = http
        .post(format!("{base}/api/rigs/smoke/stop"))
        .json(&json!({ "request_id": "stop-1" }))
        .send()
        .await
        .unwrap();
    assert!(stop.status().is_success());
    poll_for("both seats stopped", Duration::from_secs(60), async || {
        lifecycles(http.clone(), base.clone()).await == ["stopped", "stopped"]
    })
    .await;

    let token = std::fs::read_to_string(data.path().join("host-token")).unwrap();
    assert!(token.len() >= 32);
    let data_path = data.path().to_path_buf();
    drop(serve);
    wait_gone(&data_path);
}

#[test]
fn sigint_to_serves_process_group_leaves_hostd_running() {
    let data = tempfile::tempdir().unwrap();
    let mut serve = Serve::start(data.path(), 0);
    let hostd = serve.hostd_pid().expect("hostd.pid written");
    assert!(alive(hostd));
    // SAFETY: signalling the process group this test created.
    unsafe {
        libc::killpg(serve.pgid(), libc::SIGINT);
    }
    serve.child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        alive(hostd),
        "hostd must survive Ctrl-C aimed at serve's process group"
    );
    drop(serve);
    wait_gone(data.path());
}

#[test]
fn restarted_serve_on_a_new_port_is_found_by_the_surviving_hostd() {
    let data = tempfile::tempdir().unwrap();
    let mut a = Serve::start(data.path(), 0);
    let hostd = a.hostd_pid().expect("hostd.pid written");
    // Kill only serve; hostd is in its own session and must survive.
    // SAFETY: signalling a process this test created.
    unsafe {
        libc::kill(a.pgid(), libc::SIGKILL);
    }
    a.child.wait().unwrap();
    assert!(alive(hostd));
    let b = Serve::start(data.path(), 0);
    assert_ne!(a.base, b.base, "second serve should get a different port");
    assert_eq!(
        b.hostd_pid(),
        Some(hostd),
        "the surviving hostd must keep owning the data dir"
    );
    // The listening line waits for the host to connect; reaching it well before the 10 s
    // timeout proves the old hostd followed controld.url to the new port.
    assert!(
        b.boot < Duration::from_secs(9),
        "hostd did not reconnect in time: {:?}",
        b.boot
    );
    drop(b);
    drop(a);
    wait_gone(data.path());
}

fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// I2: the data dir holds the host token and transcripts, so it is private.
#[test]
fn serve_creates_a_private_data_dir_and_tightens_an_existing_one() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    // A fresh data dir is created 0700.
    let fresh = parent.path().join("fresh");
    let serve = Serve::start(&fresh, 0);
    assert_eq!(mode(&fresh), 0o700, "fresh data dir");
    assert_eq!(mode(&fresh.join("host-local")), 0o700, "fresh host-local");
    drop(serve);
    wait_gone(&fresh);

    // An existing world-readable one is tightened.
    let open = parent.path().join("open");
    std::fs::create_dir_all(open.join("host-local")).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(
        open.join("host-local"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let serve = Serve::start(&open, 0);
    assert_eq!(mode(&open), 0o700, "existing data dir");
    assert_eq!(mode(&open.join("host-local")), 0o700, "existing host-local");
    drop(serve);
    wait_gone(&open);
}

/// M2: an agent process must not inherit hostd's controld token. The agent is the real Claude
/// Code CLI; its environment is read from /proc. No message is sent, so no model is called.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI (no message reaches the model); run with --ignored"]
async fn live_agent_processes_do_not_inherit_the_host_token() {
    let Some(claude) = require() else { return };
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let spec_path = work.path().join("claude.yaml");
    std::fs::write(&spec_path, one_agent_spec("envcheck")).unwrap();

    let serve = Serve::start_with(
        data.path(),
        0,
        &["--claude-program", claude.to_str().unwrap()],
    );
    let base = serve.base.clone();
    let http = client();
    let boot = http
        .post(format!("{base}/api/rigs"))
        .json(&json!({ "request_id": "boot-env", "spec_path": spec_path }))
        .send()
        .await
        .unwrap();
    assert!(boot.status().is_success(), "{}", boot.text().await.unwrap());
    let agent = agent_process(&serve).await;
    let _guard = AgentGroup(agent);

    let env = std::fs::read(format!("/proc/{agent}/environ")).unwrap();
    let env = String::from_utf8_lossy(&env).replace('\0', "\n");
    assert!(env.contains("PATH="), "the agent's environment was read");
    let token = std::fs::read_to_string(data.path().join("host-token")).unwrap();
    assert!(
        !env.contains("SCULPTURED_HOST_TOKEN"),
        "agent env has the token variable"
    );
    assert!(
        !env.contains(token.trim()),
        "agent env holds the token value"
    );

    let stop = http
        .post(format!("{base}/api/rigs/envcheck/stop"))
        .json(&json!({ "request_id": "stop-env" }))
        .send()
        .await
        .unwrap();
    assert!(stop.status().is_success());
    let data_path = data.path().to_path_buf();
    drop(serve);
    wait_gone(&data_path);
}

/// A rig of one Claude Code agent on haiku, working in the spec's folder.
fn one_agent_spec(name: &str) -> String {
    format!(
        "version: \"0.2\"\nname: {name}\npods:\n  - id: dev\n    label: Dev\n    members:\n      - {{ id: a, agent_ref: \"local:a\", profile: default, runtime: claude-code, model: haiku, cwd: \".\" }}\n"
    )
}

/// A field of /proc/<pid>/stat after the command name (0 is the state, 1 the parent, 2 the group).
fn stat_field(pid: i32, n: usize) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(n))
        .and_then(|p| p.parse().ok())
}

fn cmdline(pid: i32) -> String {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|c| String::from_utf8_lossy(&c).replace('\0', " "))
        .unwrap_or_default()
}

/// Live children of `parent` whose command line holds `needle`.
fn children_with(parent: i32, needle: &str) -> Vec<i32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<i32>().ok())
        .filter(|&pid| stat_field(pid, 1) == Some(parent) && alive(pid))
        .filter(|&pid| cmdline(pid).contains(needle))
        .collect()
}

/// A live descendant of `root` running `sleep 600` (the command itself, not a shell around it).
fn sleep_under(root: i32) -> Option<i32> {
    let is_sleep = |pid: i32| {
        let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let mut args = cmd.split(|b| *b == 0);
        let prog = args.next().unwrap_or_default();
        prog.rsplit(|b| *b == b'/').next() == Some(b"sleep".as_slice())
            && args.next() == Some(b"600".as_slice())
    };
    let mut todo = vec![root];
    while let Some(p) = todo.pop() {
        let kids = children_with(p, "");
        if let Some(&hit) = kids.iter().find(|&&k| is_sleep(k)) {
            return Some(hit);
        }
        todo.extend(kids);
    }
    None
}

/// The real Claude Code process this test's hostd started for its agent (`claude -p … stream-json`).
async fn agent_process(serve: &Serve) -> i32 {
    let hostd = serve.hostd_pid().expect("hostd pid");
    assert!(
        sculptured_procs(&serve.data).contains(&hostd),
        "hostd is this test's"
    );
    poll_for("the agent's process", Duration::from_secs(60), async || {
        !children_with(hostd, "stream-json").is_empty()
    })
    .await;
    children_with(hostd, "stream-json")[0]
}

/// Kills an agent's process group on drop if it is still running, so a failing test leaves no
/// agent behind once its hostd is gone. The agent leads its own group (the driver makes one).
struct AgentGroup(i32);

impl Drop for AgentGroup {
    fn drop(&mut self) {
        let pid = self.0;
        if alive(pid) && stat_field(pid, 2) == Some(pid) && cmdline(pid).contains("stream-json") {
            // SAFETY: the group led by this test's agent, checked just above.
            unsafe {
                libc::killpg(pid, libc::SIGKILL);
            }
        }
    }
}

/// The HTTP client, with the one TLS crypto provider the binary uses (`ring`).
fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::new()
}

/// A runner told to stop (SIGTERM: the desktop quitting, `systemctl stop`) stops its agents first,
/// rather than leaving them working unwatched. The agent is the real Claude Code CLI on haiku, busy
/// in a long turn (running `sleep 600` with its Bash tool).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: real claude CLI on haiku; run with --ignored"]
async fn live_a_runner_told_to_stop_stops_its_agents() {
    let Some(claude) = require() else { return };
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let spec_path = work.path().join("busy.yaml");
    std::fs::write(&spec_path, one_agent_spec("busy")).unwrap();

    let serve = Serve::start_with(
        data.path(),
        0,
        &["--claude-program", claude.to_str().unwrap()],
    );
    let base = serve.base.clone();
    let http = client();
    let boot = http
        .post(format!("{base}/api/rigs"))
        .json(&json!({ "request_id": "boot-busy", "spec_path": spec_path }))
        .send()
        .await
        .unwrap();
    assert!(boot.status().is_success(), "{}", boot.text().await.unwrap());
    let agent = agent_process(&serve).await;
    let _guard = AgentGroup(agent);
    poll_for("the seat running", Duration::from_secs(60), async || {
        let snap: Value = http
            .get(format!("{base}/api/rigs/busy"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        snap["seats"][0]["lifecycle"] == "running"
    })
    .await;
    let sent = http
        .post(format!("{base}/api/seats/dev.a@busy/send"))
        .json(&json!({ "request_id": "send-busy", "text": "Use your Bash tool to run exactly this command and wait for it: sleep 600", "intent": "inform", "interrupt": false }))
        .send()
        .await
        .unwrap();
    assert!(sent.status().is_success(), "{}", sent.text().await.unwrap());
    poll_for(
        "the agent busy running sleep",
        Duration::from_secs(240),
        async || sleep_under(agent).is_some(),
    )
    .await;
    let sleep = sleep_under(agent).expect("the agent's sleep");
    let hostd = serve.hostd_pid().expect("hostd pid");

    // SAFETY: hostd is this test's runner (checked by `agent_process`).
    unsafe { libc::kill(hostd, libc::SIGTERM) };
    poll("agent stopped", async || !alive(agent)).await;
    poll("runner exited", async || !alive(hostd)).await;
    if alive(sleep) && cmdline(sleep).contains("600") {
        // SAFETY: the agent's own `sleep`, found among its descendants above and still running.
        unsafe { libc::kill(sleep, libc::SIGKILL) };
    }
    let data_path = data.path().to_path_buf();
    drop(serve);
    wait_gone(&data_path);
}
