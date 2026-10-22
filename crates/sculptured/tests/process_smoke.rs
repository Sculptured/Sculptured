//! Starts the real binary (controld + spawned hostd) and drives a stub rig over HTTP.
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
      - { id: a, agent_ref: "local:a", profile: default, runtime: stub, cwd: "." }
      - { id: b, agent_ref: "local:b", profile: default, runtime: stub, cwd: "." }
    edges: [{ kind: delegates_to, from: a, to: b }]
"#;

/// A running `sculptured serve`. On drop, kills serve's process group and the hostd
/// recorded in `{data}/host-local/hostd.pid` (hostd runs in its own session).
struct Serve {
    child: std::process::Child,
    data: PathBuf,
    base: String,
    boot: Duration,
}

impl Serve {
    fn start(data: &Path, port: u16) -> Serve {
        Serve::start_with(data, port, &[])
    }

    fn start_with(data: &Path, port: u16, extra: &[&str]) -> Serve {
        let started = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_sculptured"))
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

async fn poll(what: &str, mut f: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
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

#[tokio::test(flavor = "multi_thread")]
async fn serve_boots_messages_and_stops_a_stub_rig() {
    let data = tempfile::tempdir().unwrap();
    let specs = tempfile::tempdir().unwrap();
    let spec_path = specs.path().join("smoke.yaml");
    std::fs::write(&spec_path, SPEC).unwrap();

    let serve = Serve::start(data.path(), 0);
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
    poll("both seats running", async || {
        lifecycles(http.clone(), base.clone()).await == ["running", "running"]
    })
    .await;

    let sent: Value = http
        .post(format!("{base}/api/seats/dev.a@smoke/send"))
        .json(&json!({ "request_id": "send-1", "text": "hello", "intent": "inform", "interrupt": false }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(sent["message_id"].is_string(), "{sent}");
    poll("stub reply", async || {
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
            e["body"]["type"] == "agent-text" && e["body"]["text"] == "stub received: hello"
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
    poll("both seats stopped", async || {
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

/// M2: an agent process must not inherit hostd's controld token. The "claude" here is a
/// script that records its environment and then waits on stdin like a real session.
#[tokio::test(flavor = "multi_thread")]
async fn agent_processes_do_not_inherit_the_host_token() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let env_log = work.path().join("agent.env");
    let program = work.path().join("fake-claude.sh");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nenv > '{}'\nexec cat > /dev/null\n",
            env_log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let spec_path = work.path().join("claude.yaml");
    std::fs::write(
        &spec_path,
        "version: \"0.2\"\nname: envcheck\npods:\n  - id: dev\n    label: Dev\n    members:\n      - { id: a, agent_ref: \"local:a\", profile: default, runtime: claude-code, cwd: \".\" }\n",
    )
    .unwrap();

    let serve = Serve::start_with(
        data.path(),
        0,
        &["--claude-program", program.to_str().unwrap()],
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
    poll("agent wrote its environment", async || {
        std::fs::read_to_string(&env_log).is_ok_and(|s| s.contains("PATH="))
    })
    .await;

    let env = std::fs::read_to_string(&env_log).unwrap();
    let token = std::fs::read_to_string(data.path().join("host-token")).unwrap();
    assert!(
        !env.contains("SCULPTURED_HOST_TOKEN"),
        "agent env has the token variable:\n{env}"
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

/// The HTTP client, with the one TLS crypto provider the binary uses (`ring`).
fn client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::new()
}

/// A runner told to stop (SIGTERM: the desktop quitting, `systemctl stop`) stops its agents first,
/// rather than leaving them working unwatched. The "claude" here ignores stdin, as a busy agent does.
#[tokio::test(flavor = "multi_thread")]
async fn a_runner_told_to_stop_stops_its_agents() {
    use std::os::unix::fs::PermissionsExt;
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let pid_file = work.path().join("agent.pid");
    let program = work.path().join("busy-claude.sh");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 600\n",
            pid_file.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let spec_path = work.path().join("busy.yaml");
    std::fs::write(
        &spec_path,
        "version: \"0.2\"\nname: busy\npods:\n  - id: dev\n    label: Dev\n    members:\n      - { id: a, agent_ref: \"local:a\", profile: default, runtime: claude-code, cwd: \".\" }\n",
    )
    .unwrap();

    let serve = Serve::start_with(
        data.path(),
        0,
        &["--claude-program", program.to_str().unwrap()],
    );
    let http = client();
    let boot = http
        .post(format!("{}/api/rigs", serve.base))
        .json(&json!({ "request_id": "boot-busy", "spec_path": spec_path }))
        .send()
        .await
        .unwrap();
    assert!(boot.status().is_success(), "{}", boot.text().await.unwrap());
    poll("agent started", async || {
        std::fs::read_to_string(&pid_file).is_ok_and(|s| s.trim().parse::<i32>().is_ok())
    })
    .await;
    let agent: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let hostd = serve.hostd_pid().expect("hostd pid");

    // SAFETY: hostd is this test's runner (read from its pid file just above).
    unsafe { libc::kill(hostd, libc::SIGTERM) };
    poll("agent stopped", async || !alive(agent)).await;
    poll("runner exited", async || !alive(hostd)).await;
    let data_path = data.path().to_path_buf();
    drop(serve);
    wait_gone(&data_path);
}
