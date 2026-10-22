use super::{
    AppState, Duration, EnvFilter, ExitCode, HostRegistry, LOCAL_HOST, Parser, Path, PathBuf,
    Router, ServeDir, ServeFile, Store, Subcommand, app, connect, doctor, guard, run as run_module,
    web_cache,
};

#[derive(Parser)]
#[command(
    name = "sculptured",
    version,
    about = "Run and supervise teams of agents on your computer"
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) cmd: Cmd,
}

#[derive(Subcommand)]
pub(super) enum Cmd {
    /// Run controld (API, UI, store) and a local hostd.
    Serve {
        #[arg(long, default_value_t = 4410)]
        port: u16,
        #[arg(long, env = "SCULPTURED_DATA_DIR")]
        data_dir: Option<PathBuf>,
        #[arg(long, default_value = "web/dist")]
        web_dir: PathBuf,
        #[arg(long)]
        claude_program: Option<PathBuf>,
        #[arg(long)]
        codex_program: Option<PathBuf>,
        /// Question timeout in seconds, overriding the settings (tests and e2e only).
        #[arg(long, hide = true)]
        question_timeout_secs: Option<u64>,
        /// Hosted beta: serve one user behind the gateway (needs SCULPTURED_GATEWAY_SECRET); no local agent runner.
        #[arg(long)]
        hosted: bool,
        /// The Sculptured server Community goes through once it is turned on (not when hosted).
        #[arg(long, env = "SCULPTURED_COMMUNITY_ORIGIN", default_value = controld::community::DEFAULT_ORIGIN)]
        community_origin: String,
    },
    /// Run a host runtime that dials controld.
    Hostd {
        #[arg(long)]
        controld: String,
        #[arg(long, default_value = "local")]
        host_id: String,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long, env = "SCULPTURED_HOST_TOKEN", hide_env_values = true)]
        token: String,
        #[arg(long)]
        claude_program: Option<PathBuf>,
        #[arg(long)]
        codex_program: Option<PathBuf>,
    },
    /// The agent tools server a CLI starts for its agent (stdio). Not for people.
    #[command(hide = true)]
    SeatTools,
    /// Link this computer to a Sculptured account and run its agents here.
    Connect {
        /// The account's address, e.g. <https://app.example.com>.
        origin: String,
        /// How this computer appears in your account (default: its hostname).
        #[arg(long)]
        name: Option<String>,
        /// A folder agents may work in (repeatable). Add more later with `sculptured allow`.
        #[arg(long)]
        allow: Vec<PathBuf>,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        claude_program: Option<PathBuf>,
        #[arg(long)]
        codex_program: Option<PathBuf>,
        /// Connect automatically whenever you log in (systemd user service or LaunchAgent).
        #[arg(long)]
        install_service: bool,
        /// Don't open the system browser for the pairing link (the desktop app shows it itself).
        #[arg(long, hide = true)]
        no_browser: bool,
        /// Limit agents on this computer: Claude asks before risky actions, Codex writes only in its folder.
        #[arg(long, conflicts_with = "full_access")]
        sandboxed: bool,
        /// Let agents act as you do on this computer (the default).
        #[arg(long)]
        full_access: bool,
        /// Turn on something the account may do here: git, copies, checks, cli-sign-in, plan-usage, generate, usage-reports (repeatable).
        #[arg(long, value_name = "CAPABILITY")]
        enable: Vec<String>,
        /// Turn off something the account may do here (repeatable).
        #[arg(long, value_name = "CAPABILITY")]
        disable: Vec<String>,
        /// A one-time connect key from the Computers page (connects with no code to approve).
        #[arg(long)]
        key: Option<String>,
    },
    /// Let agents from your account work in a folder on this computer.
    Allow {
        folder: PathBuf,
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Let agents on this computer use a component that is not built in. Its digest is in the note on
    /// the agent that went without it, next to the command it runs.
    ApproveComponent {
        digest: String,
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Check what this computer has for its agents: Claude Code and Codex, allowed folders, component
    /// programs, approvals and secrets (names only). Changes nothing.
    Doctor {
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Keep a secret a component names (such as ${OPENFOAM_TOKEN}) on this computer, for its agents.
    /// The value is typed or piped in, never given on the command line.
    SetSecret {
        name: String,
        /// Forget it instead.
        #[arg(long)]
        remove: bool,
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Run a team from one MADL file here, in the terminal: what happens is printed as it happens, and
    /// the exit code says how it went (0 finished, 1 an agent failed, 2 stopped by a budget, 3 needs
    /// an answer, 4 the file or an option is invalid, 130 stopped with Ctrl+C).
    Run {
        file: PathBuf,
        /// Where the agents work (default: this folder).
        #[arg(long)]
        folder: Option<PathBuf>,
        /// The team's Input.
        #[arg(long)]
        input_text: Option<String>,
        /// Most tokens (of every kind) for the run; may only lower what the file sets.
        #[arg(long)]
        tokens: Option<u64>,
        /// Most minutes for the run; may only lower what the file sets.
        #[arg(long)]
        minutes: Option<u32>,
        /// The run's data (default: `~/.sculptured/runs/<name>-<time>`).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Seconds every agent must be idle, with nothing waiting, before the run counts as finished.
        #[arg(long, default_value_t = 20, hide = true)]
        settle_secs: u64,
        #[arg(long)]
        claude_program: Option<PathBuf>,
        #[arg(long)]
        codex_program: Option<PathBuf>,
    },
    /// Stop every agent this computer's runner started, now, without the server (like Ctrl+C).
    Stop {
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
}

pub(super) fn main() -> ExitCode {
    // Claude Code runs this hook before every guarded tool call: answer before anything else starts.
    if std::env::args_os().nth(1).is_some_and(|a| a == "guard") {
        return guard::run();
    }
    // And this one after Claude compacts an agent's context.
    if std::env::args_os().nth(1).is_some_and(|a| a == "rebrief") {
        return guard::rebrief(std::env::args_os().nth(2));
    }
    migrate_legacy_environment();
    let cli = Cli::parse();
    // `hostd` has read its token from the environment by now. Remove it so no agent
    // process (or anything else this process starts) inherits it.
    // SAFETY: no other thread exists yet; the tokio runtime is built below.
    unsafe {
        std::env::remove_var(drivers::HOST_TOKEN_ENV);
        std::env::remove_var("RIG_CONTROL_HOST_TOKEN");
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("sculptured: cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run(cli))
}

pub(super) async fn run(cli: Cli) -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let result = match cli.cmd {
        Cmd::Serve {
            port,
            data_dir,
            web_dir,
            claude_program,
            codex_program,
            question_timeout_secs,
            hosted,
            community_origin,
        } => {
            let data_dir = match data_dir
                .or_else(|| std::env::var_os("HOME").map(|h| default_data_dir(Path::new(&h))))
            {
                Some(d) => d,
                None => {
                    eprintln!(
                        "sculptured: set --data-dir or SCULPTURED_DATA_DIR (HOME is not set)"
                    );
                    return ExitCode::FAILURE;
                }
            };
            serve(
                port,
                data_dir,
                web_dir,
                claude_program,
                codex_program,
                question_timeout_secs,
                hosted,
                community_origin,
            )
            .await
        }
        Cmd::Hostd {
            controld,
            host_id,
            data_dir,
            token,
            claude_program,
            codex_program,
        } => {
            hostd::run_hostd(hostd::HostdOptions {
                controld_url: controld,
                host_id,
                data_dir,
                token,
                claude_program,
                codex_program,
                policy: None,
            })
            .await
        }
        // Logging goes to stderr, so stdout carries only the MCP protocol.
        Cmd::SeatTools => tokio::task::spawn_blocking(hostd::seat_tools::run_shim)
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r),
        Cmd::Connect {
            origin,
            name,
            allow,
            data_dir,
            claude_program,
            codex_program,
            install_service,
            no_browser,
            sandboxed,
            full_access,
            enable,
            disable,
            key,
        } => {
            let access = if sandboxed {
                Some(hostd::policy::Access::Sandboxed)
            } else if full_access {
                Some(hostd::policy::Access::Full)
            } else {
                None
            };
            let caps = enable
                .into_iter()
                .map(|c| (c, true))
                .chain(disable.into_iter().map(|c| (c, false)))
                .collect();
            connect::run_connect(connect::ConnectArgs {
                origin,
                name,
                allow,
                data_dir,
                claude_program,
                codex_program,
                install_service,
                no_browser,
                access,
                caps,
                key,
            })
            .await
        }
        Cmd::Allow { folder, data_dir } => connect::run_allow(&folder, data_dir),
        Cmd::ApproveComponent { digest, data_dir } => {
            connect::run_approve_component(&digest, data_dir)
        }
        Cmd::Doctor { data_dir } => match connect::runner_dirs(data_dir) {
            Ok(dirs) => doctor::run(dirs).await,
            Err(e) => Err(e),
        },
        Cmd::SetSecret {
            name,
            remove,
            data_dir,
        } => connect::run_set_secret(&name, remove, data_dir),
        Cmd::Stop { data_dir } => connect::run_stop(data_dir),
        Cmd::Run {
            file,
            folder,
            input_text,
            tokens,
            minutes,
            data_dir,
            settle_secs,
            claude_program,
            codex_program,
        } => {
            let code = run_module::run(run_module::RunArgs {
                file,
                folder,
                input_text,
                tokens,
                minutes,
                data_dir,
                settle: Duration::from_secs(settle_secs),
                claude_program,
                codex_program,
            })
            .await;
            return ExitCode::from(code);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        // The service must not start a runner whose account disconnected this computer again.
        Err(e) if e == hostd::DISCONNECTED => {
            eprintln!("sculptured: {e}");
            ExitCode::from(connect::DISCONNECTED_EXIT)
        }
        Err(e) if e == hostd::ALREADY_RUNNING => {
            eprintln!("sculptured: {e}");
            ExitCode::from(75)
        }
        Err(e) => {
            eprintln!("sculptured: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Reuses `{data_dir}/host-token` (mode 0600) so a hostd that outlives controld
/// can reconnect after a controld restart.
pub(super) fn host_token(data_dir: &Path) -> Result<String, String> {
    let path = data_dir.join("host-token");
    if let Ok(t) = std::fs::read_to_string(&path)
        && !t.trim().is_empty()
    {
        hostd::private::write_file(&path, t.trim().as_bytes())
            .map_err(|e| format!("cannot narrow {}: {e}", path.display()))?;
        return Ok(t.trim().to_string());
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    // Write a private temp file, then rename, so the token is never readable by others.
    let tmp = data_dir.join(format!("host-token.tmp-{}", std::process::id()));
    let write = || -> std::io::Result<()> {
        hostd::private::write_file(&tmp, token.as_bytes())?;
        std::fs::rename(&tmp, &path)
    };
    write().map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(token)
}

/// Creates `dir` (and missing parents) with mode 0700, and tightens it to 0700 if it
/// already exists: it holds the host token, the event store and agent transcripts.
pub(super) fn private_dir(dir: &Path) -> Result<(), String> {
    hostd::private::create_dir(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    hostd::private::narrow_dir(dir).map_err(|e| format!("cannot chmod {}: {e}", dir.display()))
}

/// Detaches the hostd child from serve's terminal, so Ctrl-C or SIGHUP aimed at serve cannot kill
/// the agents (spec 3.1). `serve` runs on Unix only; this keeps the binary building for Windows.
#[cfg(unix)]
pub(super) fn detach(cmd: &mut tokio::process::Command) {
    // SAFETY: setsid is async-signal-safe. A new session detaches hostd from serve's process group
    // and controlling terminal. process_group(0) alone would still leave hostd attached to the
    // terminal's session and exposed to SIGHUP.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}

#[cfg(windows)]
pub(super) fn detach(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(0x0000_0200 | 0x0800_0000);
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn serve(
    port: u16,
    data_dir: PathBuf,
    web_dir: PathBuf,
    claude_program: Option<PathBuf>,
    codex_program: Option<PathBuf>,
    question_timeout_secs: Option<u64>,
    hosted: bool,
    community_origin: String,
) -> Result<(), String> {
    private_dir(&data_dir)?;
    let mut store = Store::open(&data_dir.join("controld.sqlite"))
        .map_err(|e| format!("cannot open store: {e}"))?;
    store
        .set_question_timeout_override(question_timeout_secs.map(|s| s * 1000))
        .map_err(|e| format!("cannot write settings: {e}"))?;
    let token = host_token(&data_dir)?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|e| format!("cannot bind 127.0.0.1:{port}: {e}"))?;
    let actual = listener.local_addr().map_err(|e| e.to_string())?.port();
    let st = AppState::new(
        store,
        token.clone(),
        vec![format!("127.0.0.1:{actual}"), format!("localhost:{actual}")],
    )
    .with_cli_programs(claude_program.clone(), codex_program.clone());
    let st = if hosted {
        let secret = std::env::var("SCULPTURED_GATEWAY_SECRET")
            .map_err(|_| "--hosted needs SCULPTURED_GATEWAY_SECRET".to_string())?;
        let secret = std::sync::Arc::new(secret.into_bytes());
        let st = st.with_auth(controld::auth::AuthPolicy::Gateway {
            secret: secret.clone(),
        });
        // The gateway passes where to report (analytics) and as whom; without both, nothing is sent.
        match (
            std::env::var("SCULPTURED_GATEWAY_URL"),
            std::env::var("SCULPTURED_USER"),
        ) {
            (Ok(url), Ok(user)) => {
                let st =
                    st.with_analytics(controld::analytics::Analytics::start(url, user, secret));
                controld::analytics::watch(&st);
                st
            }
            _ => st,
        }
    } else {
        // Off until the user turns it on; the gateway serves Community itself when hosted.
        st.with_community(controld::community::CommunityLink::new(
            data_dir.clone(),
            community_origin,
        ))
    };
    // `fallback` (not `not_found_service`) keeps status 200 for SPA deep links.
    let web_ui = Router::new()
        .fallback_service(
            ServeDir::new(&web_dir).fallback(ServeFile::new(web_dir.join("index.html"))),
        )
        .layer(axum::middleware::from_fn(web_cache::layer));
    let router = app(st.clone(), Some(web_ui));
    let (_supervisor, server) = start_with(
        st,
        listener,
        actual,
        data_dir,
        token,
        claude_program,
        codex_program,
        hosted,
        router,
    )
    .await?;
    println!("listening on http://127.0.0.1:{actual}");
    server
        .await
        .map_err(|e| format!("server task failed: {e}"))?
        .map_err(|e| format!("server error: {e}"))
}

/// Serving handles: the local runner's supervisor (None when hosted) and the HTTP server.
pub(super) type Started = (
    Option<tokio::task::JoinHandle<()>>,
    tokio::task::JoinHandle<std::io::Result<()>>,
);

/// Serves `router` and, unless hosted, keeps a local runner connected; returns once that runner is
/// connected (or 10 s have passed).
#[allow(clippy::too_many_arguments)]
pub(super) async fn start_with(
    st: AppState,
    listener: tokio::net::TcpListener,
    actual: u16,
    data_dir: PathBuf,
    token: String,
    claude_program: Option<PathBuf>,
    codex_program: Option<PathBuf>,
    hosted: bool,
    router: Router,
) -> Result<Started, String> {
    let server = tokio::spawn(async move { axum::serve(listener, router).await });
    // A hostd that outlived a previous controld re-reads this file to find the new port.
    let host_dir = data_dir.join("host-local");
    private_dir(&host_dir)?;
    let url_file = host_dir.join("controld.url");
    let url_tmp = host_dir.join(format!("controld.url.tmp-{}", std::process::id()));
    std::fs::write(&url_tmp, format!("ws://127.0.0.1:{actual}/host"))
        .and_then(|_| std::fs::rename(&url_tmp, &url_file))
        .map_err(|e| format!("cannot write {}: {e}", url_file.display()))?;
    // Hosted: agents run on the user's own computer (milestone 2), never on the server.
    let mut supervisor = None;
    if !hosted {
        supervisor = Some(tokio::spawn(supervise_hostd(
            st.hosts.clone(),
            actual,
            host_dir,
            token,
            claude_program,
            codex_program,
        )));
        // Announce readiness only once the local hostd is connected; a boot sent before
        // that would put every seat in attention ("Sculptured's agent runner is not connected").
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !st.hosts.is_connected(LOCAL_HOST) {
            if tokio::time::Instant::now() >= deadline {
                tracing::warn!(
                    "local hostd has not connected after 10s; seats need attention until it does"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    Ok((supervisor, server))
}

/// controld and a local runner on 127.0.0.1 for `sculptured run`, with no web UI: its address, state,
/// and the local runner's supervisor (abort it before stopping the runner, or it starts another).
pub(crate) async fn start_local(
    data_dir: &Path,
    claude_program: Option<PathBuf>,
    codex_program: Option<PathBuf>,
) -> Result<(String, Option<tokio::task::JoinHandle<()>>), String> {
    private_dir(data_dir)?;
    let store = Store::open(&data_dir.join("controld.sqlite"))
        .map_err(|e| format!("cannot open store: {e}"))?;
    let token = host_token(data_dir)?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("cannot listen on 127.0.0.1: {e}"))?;
    let actual = listener.local_addr().map_err(|e| e.to_string())?.port();
    let st = AppState::new(
        store,
        token.clone(),
        vec![format!("127.0.0.1:{actual}"), format!("localhost:{actual}")],
    )
    .with_cli_programs(claude_program.clone(), codex_program.clone());
    let router = app(st.clone(), None);
    let (supervisor, _server) = start_with(
        st,
        listener,
        actual,
        data_dir.to_path_buf(),
        token,
        claude_program,
        codex_program,
        false,
        router,
    )
    .await?;
    Ok((format!("http://127.0.0.1:{actual}"), supervisor))
}

/// Keeps one local hostd child running; restarts it with backoff if it exits.
pub(super) async fn supervise_hostd(
    hosts: HostRegistry,
    port: u16,
    data_dir: PathBuf,
    token: String,
    claude_program: Option<PathBuf>,
    codex_program: Option<PathBuf>,
) {
    const MIN_BACKOFF: Duration = Duration::from_millis(500);
    const MAX_BACKOFF: Duration = Duration::from_secs(30);
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(%e, "cannot locate own executable; hostd not started");
            return;
        }
    };
    let mut backoff = MIN_BACKOFF;
    let mut owned_count = 0u32;
    loop {
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("hostd")
            .arg("--controld")
            .arg(format!("ws://127.0.0.1:{port}/host"))
            .arg("--host-id")
            .arg("local")
            .arg("--data-dir")
            .arg(&data_dir)
            .env("SCULPTURED_HOST_TOKEN", &token)
            .stdout(std::process::Stdio::null());
        if let Some(p) = &claude_program {
            cmd.arg("--claude-program").arg(p);
        }
        if let Some(p) = &codex_program {
            cmd.arg("--codex-program").arg(p);
        }
        detach(&mut cmd);
        let started = tokio::time::Instant::now();
        match cmd.spawn() {
            Ok(mut child) => {
                let status = child.wait().await;
                if status.as_ref().ok().and_then(|s| s.code()) == Some(75) {
                    // A hostd that outlived the previous controld holds the lock and
                    // follows controld.url by itself. Check back slowly.
                    owned_count += 1;
                    if owned_count >= 2 && !hosts.is_connected(LOCAL_HOST) {
                        let pid = std::fs::read_to_string(data_dir.join("hostd.pid"))
                            .unwrap_or_else(|_| "unknown".into());
                        tracing::warn!(pid = %pid.trim(), "an existing hostd owns the data dir; not starting another");
                    } else {
                        tracing::info!("existing hostd owns the data dir; not starting another");
                    }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue;
                }
                owned_count = 0;
                tracing::warn!(?status, "hostd exited; restarting");
                // Only a child that stayed up a while counts as healthy.
                if started.elapsed() >= Duration::from_secs(10) {
                    backoff = MIN_BACKOFF;
                }
            }
            Err(e) => tracing::error!(%e, "cannot start hostd"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Compatibility for existing installations and live agent tool processes during the rename.
/// Called before any thread is started; credentials are removed again before agents launch.
pub(super) fn migrate_legacy_environment() {
    for suffix in [
        "DATA_DIR",
        "HOST_TOKEN",
        "SEAT_SOCKET",
        "SEAT_TOKEN",
        "SEAT_APPROVE",
    ] {
        let current = format!("SCULPTURED_{suffix}");
        if std::env::var_os(&current).is_none()
            && let Some(value) = std::env::var_os(format!("RIG_CONTROL_{suffix}"))
        {
            // SAFETY: main calls this before parsing CLI arguments or creating threads.
            unsafe { std::env::set_var(current, value) };
        }
    }
}
pub(super) fn default_data_dir(home: &Path) -> PathBuf {
    let current = home.join(".sculptured");
    let legacy = home.join(".rig-control");
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}
#[cfg(test)]
mod branding_migration_tests {
    use super::*;
    #[test]
    fn existing_projects_are_reused_and_new_installs_use_sculptured() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(
            default_data_dir(home.path()),
            home.path().join(".sculptured")
        );
        std::fs::create_dir(home.path().join(".rig-control")).unwrap();
        assert_eq!(
            default_data_dir(home.path()),
            home.path().join(".rig-control")
        );
        std::fs::create_dir(home.path().join(".sculptured")).unwrap();
        assert_eq!(
            default_data_dir(home.path()),
            home.path().join(".sculptured")
        );
    }
}
