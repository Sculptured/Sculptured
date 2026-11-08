//! `e2e-server`: the Playwright web server. Builds the web UI into a throwaway directory, runs
//! `sculptured serve` on port 14410 with a throwaway data dir and `HOME`, and on exit, SIGINT,
//! SIGTERM or SIGHUP stops serve and hostd and removes the directory.
//!
//! The binary is `$SCULPTURED_BIN` when set; otherwise `cargo build -p sculptured` runs and
//! `${CARGO_TARGET_DIR:-target}/debug/sculptured` is used. `$RIG_E2E_PORT` picks another
//! port (the Playwright config reads the same variable), so two runs can share a machine; the
//! ports of the person's own Sculptured (4410, 4420) are refused.
//!
//! Claude Code agents are real: serve gets `--claude-program` (`$RIG_E2E_CLAUDE`, else `claude` on
//! `PATH` or in `~/.local/bin`) and `CLAUDE_CONFIG_DIR` (as set, else the real `~/.claude`), so the
//! CLI is signed in as the person while `HOME` stays throwaway. Specs that start agents are live
//! and opt in with `RIG_E2E_LIVE=1`; they work in a folder inside the throwaway home (`GET
//! /api/folders` gives its path; `POST /api/folders` makes one).
//!
//! Serve runs with `--question-timeout-secs 8`: questions expire in 8 s, long enough for a
//! Playwright test to answer one first and short enough to see an expiry.
//!
//! Codex agents are real too: serve gets `--codex-program` (`$RIG_E2E_CODEX`, else `codex` on
//! `PATH` or in `~/.local/bin`) and `CODEX_HOME` (as set, else the real `~/.codex`), so the CLI is
//! signed in as the person. Only live specs start them.
//!
//! Community goes through `fake-community` (`$RIG_E2E_FAKE_COMMUNITY`, else built with cargo), a
//! stand-in Community web service (not an agent) on `$RIG_E2E_COMMUNITY_PORT` (default 14411); serve gets
//! `--community-origin http://127.0.0.1:<port>`, so no test reaches a real server.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use e2e_server::{Session, usable_port};

const DEFAULT_PORT: u16 = 14410;
const DEFAULT_COMMUNITY_PORT: u16 = 14411;

/// `$<var>` when set (refused when it is not a port e2e may use), else `default`.
fn port_from(var: &str, default: u16) -> Result<String, String> {
    match std::env::var(var) {
        Err(_) => Ok(default.to_string()),
        Ok(v) => usable_port(&v)
            .map(|p| p.to_string())
            .ok_or_else(|| format!("{var}={v} is not a port e2e may use (4410 and 4420 are the person's own Sculptured)")),
    }
}

fn port() -> Result<String, String> {
    port_from("RIG_E2E_PORT", DEFAULT_PORT)
}

fn community_port() -> Result<String, String> {
    port_from("RIG_E2E_COMMUNITY_PORT", DEFAULT_COMMUNITY_PORT)
}

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
        unsafe {
            libc::signal(sig, on_signal as *const () as libc::sighandler_t);
        }
    }
}

/// The workspace root: two levels above this crate.
fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn run(what: &str, cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if STOP.load(Ordering::SeqCst) {
        return Err(format!("{what}: stopped by a signal"));
    }
    if !status.success() {
        return Err(format!("{what} failed ({status})"));
    }
    Ok(())
}

fn binary(root: &Path) -> Result<PathBuf, String> {
    if let Some(bin) = std::env::var_os("SCULPTURED_BIN") {
        let bin = PathBuf::from(bin);
        return if bin.is_file() {
            Ok(bin)
        } else {
            Err(format!("SCULPTURED_BIN is not a file: {}", bin.display()))
        };
    }
    run(
        "cargo build -p sculptured",
        Command::new("cargo")
            .args(["build", "-q", "-p", "sculptured"])
            .current_dir(root),
    )?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    Ok(target.join("debug").join("sculptured"))
}

/// A fixtures binary for e2e (the stand-in Community service): `$<env>` when set, else built with cargo.
fn fixture_bin(root: &Path, name: &str, env: &str) -> Result<PathBuf, String> {
    if let Some(bin) = std::env::var_os(env) {
        let bin = PathBuf::from(bin);
        return if bin.is_file() {
            Ok(bin)
        } else {
            Err(format!("{env} is not a file: {}", bin.display()))
        };
    }
    run(
        &format!("cargo build -p fixtures --bin {name}"),
        Command::new("cargo")
            .args(["build", "-q", "-p", "fixtures", "--bin", name])
            .current_dir(root),
    )?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    Ok(target.join("debug").join(name))
}

/// Starts the stub Sculptured server and waits until it answers; returns its origin.
fn start_stub(session: &mut Session, root: &Path) -> Result<String, String> {
    let bin = fixture_bin(root, "fake-community", "RIG_E2E_FAKE_COMMUNITY")?;
    let port = community_port()?;
    let child = Command::new(&bin)
        .args(["--port", &port])
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("start {}: {e}", bin.display()))?;
    session.adopt_stub(child);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap_or(14411))).is_err()
    {
        if session
            .stub_mut()
            .and_then(|c| c.try_wait().ok().flatten())
            .is_some()
            || std::time::Instant::now() > deadline
        {
            return Err(format!("fake-community did not start on port {port}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(format!("http://127.0.0.1:{port}"))
}

fn build_web(root: &Path, dist: &Path) -> Result<(), String> {
    run(
        "web build",
        Command::new("npm")
            .arg("--prefix")
            .arg(root.join("web"))
            .args(["run", "build", "--", "--outDir"])
            .arg(dist)
            .arg("--emptyOutDir"),
    )
}

/// Lets serve's agents run the person's `claude`, signed in through `CLAUDE_CONFIG_DIR`, while
/// `HOME` is the throwaway one. Call after the serve arguments.
fn real_claude(cmd: &mut Command) {
    let real_home = std::env::var_os("HOME").map(PathBuf::from);
    let config = e2e_server::claude_config_dir(
        std::env::var_os("CLAUDE_CONFIG_DIR").as_deref(),
        real_home.as_deref(),
    );
    let claude = e2e_server::find_claude(
        std::env::var_os("RIG_E2E_CLAUDE").as_deref(),
        std::env::var_os("PATH").as_deref(),
        real_home.as_deref(),
    );
    cmd.env_remove("CLAUDE_CONFIG_DIR");
    if let Some(dir) = &config {
        cmd.env("CLAUDE_CONFIG_DIR", dir);
    }
    if let Some(c) = &claude {
        cmd.arg("--claude-program").arg(c);
    }
    match (&claude, &config) {
        (Some(c), Some(d)) => eprintln!(
            "e2e-server: Claude Code agents run {} with the setup in {}",
            c.display(),
            d.display()
        ),
        _ => eprintln!(
            "e2e-server: no claude CLI or Claude Code setup found; live specs (RIG_E2E_LIVE=1) cannot start agents"
        ),
    }
}

/// Lets serve's Codex agents run the person's `codex`, signed in through `CODEX_HOME`, while `HOME`
/// is the throwaway one. Call after the serve arguments.
fn real_codex(cmd: &mut Command) {
    let real_home = std::env::var_os("HOME").map(PathBuf::from);
    let home = e2e_server::codex_home(
        std::env::var_os("CODEX_HOME").as_deref(),
        real_home.as_deref(),
    );
    let codex = e2e_server::find_codex(
        std::env::var_os("RIG_E2E_CODEX").as_deref(),
        std::env::var_os("PATH").as_deref(),
        real_home.as_deref(),
    );
    cmd.env_remove("CODEX_HOME");
    if let Some(dir) = &home {
        cmd.env("CODEX_HOME", dir);
    }
    if let Some(c) = &codex {
        cmd.arg("--codex-program").arg(c);
    }
    match (&codex, &home) {
        (Some(c), Some(d)) => eprintln!(
            "e2e-server: Codex agents run {} with the setup in {}",
            c.display(),
            d.display()
        ),
        _ => eprintln!(
            "e2e-server: no codex CLI or Codex setup found; live Codex specs (RIG_E2E_LIVE=1) cannot start agents"
        ),
    }
}

fn serve(session: &mut Session) -> Result<ExitCode, String> {
    let port = port()?;
    let root = workspace();
    let dist = session.web_dist();
    build_web(&root, &dist)?;
    let bin = binary(&root)?;
    let community = start_stub(session, &root)?;
    let mut cmd = Command::new(&bin);
    cmd.env("HOME", session.home())
        .env_remove("SCULPTURED_DATA_DIR")
        .args(["serve", "--port", &port, "--data-dir"])
        .arg(session.data_dir())
        .arg("--web-dir")
        .arg(&dist)
        .args(["--question-timeout-secs", "8"])
        .arg("--community-origin")
        .arg(&community);
    real_claude(&mut cmd);
    real_codex(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("start {}: {e}", bin.display()))?;
    eprintln!(
        "e2e-server: serving http://127.0.0.1:{port} from {} (data and HOME in {})",
        bin.display(),
        session.root().display()
    );
    session.adopt_serve(child);
    wait_for_serve(session)
}

fn wait_for_serve(session: &mut Session) -> Result<ExitCode, String> {
    loop {
        if STOP.load(Ordering::SeqCst) {
            return Ok(ExitCode::SUCCESS);
        }
        if let Some(status) = session
            .serve_mut()
            .and_then(|c| c.try_wait().ok().flatten())
        {
            return Ok(if status.success() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn main() -> ExitCode {
    install_signal_handlers();
    // Created before any build, so every way out removes the throwaway directory.
    let mut session = match Session::try_new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("e2e-server: cannot create a throwaway directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let code = serve(&mut session).unwrap_or_else(|e| {
        eprintln!("e2e-server: {e}");
        ExitCode::FAILURE
    });
    session.shutdown();
    code
}
