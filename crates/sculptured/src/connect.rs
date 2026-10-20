//! `sculptured connect <origin>`: link this computer to a Sculptured account. It pairs once (a code
//! approved in the browser), keeps its key in `<data dir>/device.json` (0600), then runs the agent
//! runner against the account over `wss://…/host`, within the folders allowed in `runner.json`.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credentials { pub origin: String, pub device_id: String, pub key: String }

impl Credentials {
    pub fn load(dir: &Path) -> Result<Option<Credentials>, String> {
        match std::fs::read_to_string(dir.join("device.json")) {
            Ok(t) => serde_json::from_str(&t).map(Some).map_err(|e| format!("device.json is not valid: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let tmp = dir.join("device.json.tmp");
        hostd::private::write_file(&tmp, serde_json::to_string_pretty(self).unwrap().as_bytes()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dir.join("device.json")).map_err(|e| e.to_string())
    }
}

pub fn host_url(origin: &str) -> Result<String, String> {
    let bad = || format!("{origin} must be like https://app.example.com (http:// only for this computer, for testing)");
    let u = reqwest::Url::parse(origin.trim_end_matches('/')).map_err(|_| bad())?;
    if !u.username().is_empty() || u.password().is_some() || u.query().is_some() || !matches!(u.path(), "" | "/") { return Err(bad()) }
    let host = u.host_str().ok_or_else(bad)?;
    let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
    match u.scheme() {
        "https" => Ok(format!("wss://{host}{port}/host")),
        "http" if matches!(host, "127.0.0.1" | "localhost") => Ok(format!("ws://{host}{port}/host")),
        _ => Err(bad()),
    }
}

/// The one TLS crypto provider (`ring`) this process uses, for the pairing client and the runner link.
pub fn tls_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub(crate) fn http() -> reqwest::Client { tls_provider(); reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().expect("client") }

/// Runs the pairing: shows the code via `on_code(code, url)`, waits for approval, returns the credentials.
pub async fn pair(origin: &str, name: &str, platform: &str, on_code: impl Fn(&str, &str)) -> Result<Credentials, String> {
    let origin = origin.trim_end_matches('/');
    #[derive(Deserialize)] struct Start { device_code: String, user_code: String, verify_url: String, interval: u64, expires_in: u64 }
    let c = http();
    let s: Start = c.post(format!("{origin}/gw/api/device/start")).header("origin", origin).json(&serde_json::json!({"name": name, "platform": platform}))
        .send().await.map_err(|e| format!("cannot reach {origin}: {e}"))?.error_for_status().map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    on_code(&s.user_code, &s.verify_url);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(s.expires_in);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(s.interval.clamp(1, 30))).await;
        if std::time::Instant::now() > deadline { return Err("The code expired before it was approved. Run `sculptured connect` again.".into()) }
        let r = c.post(format!("{origin}/gw/api/device/token")).header("origin", origin).json(&serde_json::json!({"device_code": s.device_code})).send().await.map_err(|e| e.to_string())?;
        match r.status().as_u16() {
            200 => {
                #[derive(Deserialize)] struct Got { device_id: String, device_key: String }
                let g: Got = r.json().await.map_err(|e| e.to_string())?;
                return Ok(Credentials { origin: origin.to_string(), device_id: g.device_id, key: g.device_key });
            }
            428 => continue,
            _ => return Err("Pairing was not approved in time or was refused. Run `sculptured connect` again.".into()),
        }
    }
}

/// Trades a one-time connect key (from the Computers page's copied command) for this computer's own key.
pub async fn redeem(origin: &str, key: &str, name: &str, platform: &str) -> Result<Credentials, String> {
    let origin = origin.trim_end_matches('/');
    let r = http().post(format!("{origin}/gw/api/device/redeem")).header("origin", origin)
        .json(&serde_json::json!({"key": key, "name": name, "platform": platform}))
        .send().await.map_err(|e| format!("cannot reach {origin}: {e}"))?;
    if !r.status().is_success() {
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        return Err(v["error"]["message"].as_str().unwrap_or("This connect command did not work. Copy a new one from the Computers page.").to_string());
    }
    #[derive(Deserialize)] struct Got { device_id: String, device_key: String }
    let g: Got = r.json().await.map_err(|e| e.to_string())?;
    Ok(Credentials { origin: origin.to_string(), device_id: g.device_id, key: g.device_key })
}

/// Whether the server still accepts these saved credentials. Only a refusal (401: the computer was
/// disconnected) is no; an unreachable server is taken as yes, so a network blip never re-pairs.
pub async fn still_connected(c: &Credentials) -> bool {
    let r = http().get(format!("{}/gw/api/device/check", c.origin)).bearer_auth(format!("{}.{}", c.device_id, c.key)).send().await;
    !matches!(r, Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED)
}

/// The credentials to connect with when a one-time `key` was given: the saved ones while they still
/// work (re-running the command does not add the computer twice), else the key's. None without a key.
pub async fn credentials(origin: &str, saved: Option<Credentials>, key: Option<&str>, name: &str) -> Option<Result<Credentials, String>> {
    let key = key?;
    if let Some(c) = saved.filter(|c| c.origin == origin) {
        if still_connected(&c).await {
            return Some(Ok(c));
        }
    }
    Some(redeem(origin, key, name, std::env::consts::OS).await)
}

/// A computer with no folders allowed yet lets agents work anywhere in its home folder. True when it did.
pub fn default_to_home(policy: &mut hostd::policy::RunnerPolicy, home: &Path) -> Result<bool, String> {
    if !policy.allowed_folders.is_empty() {
        return Ok(false);
    }
    policy.allow(home)?;
    Ok(true)
}

/// What the login service runs: `connect` for this origin, with the CLIs' full paths, because a login
/// service starts with a minimal PATH that usually lacks `~/.local/bin`, nvm or Homebrew.
pub fn service_args(exe: &Path, origin: &str, data_dir: &Path, claude: Option<&Path>, codex: Option<&Path>) -> Vec<String> {
    let mut a = vec![exe.display().to_string(), "connect".into(), origin.into(), "--data-dir".into(), data_dir.display().to_string()];
    if let Some(c) = claude { a.extend(["--claude-program".into(), c.display().to_string()]); }
    if let Some(c) = codex { a.extend(["--codex-program".into(), c.display().to_string()]); }
    a
}


fn xml(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }
fn systemd_quote(s: &str) -> String { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%")) }

/// Windows: the Scheduled Task (XML) that runs the runner at logon as this user, without a window
/// (`sculptured-background.exe` starts `sculptured.exe` hidden), never stopped for running long, and
/// restarted if it ends.
pub fn windows_task(args: &[String], user: &str) -> String {
    // Split by hand: `\\` is a separator only on Windows, and this is also checked on Unix.
    let exe = &args[0];
    let launcher = format!("{}sculptured-background.exe", &exe[..exe.rfind(['\\', '/']).map_or(0, |i| i + 1)]);
    let rest: Vec<String> = args[1..].iter().map(|a| win_quote(a)).collect();
    format!(r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
<RegistrationInfo><Description>Keeps this computer connected to Sculptured so agents can run here.</Description></RegistrationInfo>
<Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{user}</UserId></LogonTrigger></Triggers>
<Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
<Settings><MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy><DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries><StopIfGoingOnBatteries>false</StopIfGoingOnBatteries><ExecutionTimeLimit>PT0S</ExecutionTimeLimit><RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure><StartWhenAvailable>true</StartWhenAvailable></Settings>
<Actions Context="Author"><Exec><Command>{}</Command><Arguments>{}</Arguments></Exec></Actions>
</Task>
"#, xml(&launcher), xml(&rest.join(" ")), user = xml(user))
}

/// One argument on a Windows command line: quoted when it holds a space, tab or quote.
fn win_quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '"']) {
        return a.to_string();
    }
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in a.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => { out.push_str(&"\\".repeat(2 * slashes + 1)); slashes = 0; }
            _ => slashes = 0,
        }
        out.push(c);
    }
    out.push_str(&"\\".repeat(slashes));
    out.push('"');
    out
}

/// The commands that (re)start the login service from `file`. Running them again restarts it, so a
/// newly installed runner takes over from the old one.
#[cfg(test)]
pub fn service_commands(file: &Path) -> Vec<Vec<String>> {
    service_commands_for(file, None)
}

/// The service's name: the first account's runner keeps the original name; another account's adds
/// its folder's name, so each account's runner is a service of its own.
fn named(base: &str, sep: &str, suffix: Option<&str>, end: &str) -> String {
    suffix.map_or_else(|| base.to_string(), |s| format!("{base}{sep}{s}{end}"))
}

/// `service_commands` for the runner whose folder gives `suffix`.
pub fn service_commands_for(file: &Path, suffix: Option<&str>) -> Vec<Vec<String>> {
    let f = file.display().to_string();
    let unit = named("sculptured-runner", "-", suffix, "");
    let task = named(WINDOWS_TASK, " (", suffix, ")");
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if cfg!(target_os = "macos") {
        vec![v(&["launchctl", "unload", &f]), v(&["launchctl", "load", "-w", &f])]
    } else if cfg!(windows) {
        vec![v(&["schtasks", "/End", "/TN", &task]), v(&["schtasks", "/Create", "/TN", &task, "/XML", &f, "/F"]), v(&["schtasks", "/Run", "/TN", &task])]
    } else {
        vec![v(&["systemctl", "--user", "daemon-reload"]), v(&["systemctl", "--user", "enable", &unit]), v(&["systemctl", "--user", "restart", &unit])]
    }
}

const WINDOWS_TASK: &str = "Sculptured runner";

/// The exit code of a runner whose account disconnected this computer: its service does not restart it.
pub const DISCONNECTED_EXIT: u8 = 78;

/// Commands that keep the runner's login service from starting again (systemd also skips a restart on
/// DISCONNECTED_EXIT; launchd and Task Scheduler cannot tell exit codes apart).
pub fn service_off_commands(file: &Path, suffix: Option<&str>) -> Vec<Vec<String>> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if cfg!(target_os = "macos") {
        vec![v(&["launchctl", "unload", "-w", &file.display().to_string()])]
    } else if cfg!(windows) {
        vec![v(&["schtasks", "/Change", "/TN", &named(WINDOWS_TASK, " (", suffix, ")"), "/DISABLE"])]
    } else {
        // `--no-block`: this runner is the service being stopped, so it must not wait for its own stop.
        vec![v(&["systemctl", "--user", "disable", "--now", "--no-block", &named("sculptured-runner", "-", suffix, "")])]
    }
}

/// A disconnected computer's runner turns its login service off, so it is not started again every few
/// seconds. Connecting again (`sculptured connect`) turns it back on.
fn service_off(dir: &Path) {
    let suffix = crate::runners::service_suffix(dir);
    let (file, _) = service_unit_for(&[], "", suffix.as_deref());
    if !file.exists() {
        return;
    }
    for c in service_off_commands(&file, suffix.as_deref()) {
        let _ = std::process::Command::new(&c[0]).args(&c[1..]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

/// The login service that keeps this computer connected: (file to write, contents).
#[cfg(test)]
pub fn service_unit(args: &[String], path_env: &str) -> (PathBuf, String) {
    service_unit_for(args, path_env, None)
}

/// `service_unit` for the runner whose folder gives `suffix`.
pub fn service_unit_for(args: &[String], path_env: &str, suffix: Option<&str>) -> (PathBuf, String) {
    let home = hostd::platform::home().unwrap_or_default();
    let folder = suffix.map_or_else(|| home.join(".sculptured").join("runner"), |s| home.join(".sculptured").join("runners").join(s));
    if cfg!(windows) {
        let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
        return (folder.join("sculptured-runner.xml"), windows_task(args, user.trim_start_matches('\\')));
    }
    let label = named("dev.sculptured.runner", ".", suffix, "");
    if cfg!(target_os = "macos") {
        let file = home.join(format!("Library/LaunchAgents/{label}.plist"));
        let argv: String = args.iter().map(|a| format!("<string>{}</string>", xml(a))).collect();
        let text = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{label}</string>
<key>ProgramArguments</key><array>{argv}</array>
<key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string></dict>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
</dict></plist>
"#, xml(path_env));
        (file, text)
    } else {
        let file = home.join(format!(".config/systemd/user/{}.service", named("sculptured-runner", "-", suffix, "")));
        let exec: Vec<String> = args.iter().map(|a| systemd_quote(a)).collect();
        let text = format!("[Unit]\nDescription=Sculptured runner\nAfter=network-online.target\n\n[Service]\nEnvironment={}\nExecStart={}\nRestart=always\nRestartSec=5\nRestartPreventExitStatus={DISCONNECTED_EXIT}\n\n[Install]\nWantedBy=default.target\n",
            systemd_quote(&format!("PATH={path_env}")), exec.join(" "));
        (file, text)
    }
}

pub struct ConnectArgs {
    pub origin: String,
    pub name: Option<String>,
    pub allow: Vec<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub claude_program: Option<PathBuf>,
    pub codex_program: Option<PathBuf>,
    pub install_service: bool,
    /// Don't open the system browser for the pairing link (the desktop app shows it itself).
    pub no_browser: bool,
    /// Change this computer's access level (None: keep what `runner.json` says; new computers get full access).
    pub access: Option<hostd::policy::Access>,
    /// Capabilities to turn on (true) or off (false), by name.
    pub caps: Vec<(String, bool)>,
    /// A one-time connect key from the Computers page: connects with no code to approve.
    pub key: Option<String>,
}

/// Writes the service definition. Windows reads task XML as UTF-16 (with its byte-order mark).
fn write_service_file(file: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = file.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let bytes: Vec<u8> = if cfg!(windows) {
        [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect()
    } else {
        text.as_bytes().to_vec()
    };
    std::fs::write(file, bytes).map_err(|e| format!("cannot write {}: {e}", file.display()))
}

/// Applies `--enable` / `--disable` to the policy's capabilities.
pub fn set_caps(policy: &mut hostd::policy::RunnerPolicy, changes: &[(String, bool)]) -> Result<(), String> {
    let c = &mut policy.capabilities;
    for (name, on) in changes {
        let slot = match name.as_str() {
            "git" => &mut c.git,
            "copies" => &mut c.copies,
            "checks" => &mut c.checks,
            "cli-sign-in" | "cli_sign_in" => &mut c.cli_sign_in,
            "plan-usage" | "plan_usage" => &mut c.plan_usage,
            "generate" => &mut c.generate,
            "usage-reports" | "usage_reports" => &mut c.usage_reports,
            other => return Err(format!("Unknown capability {other:?}: use git, copies, checks, cli-sign-in, plan-usage, generate or usage-reports")),
        };
        *slot = Some(*on);
    }
    Ok(())
}

/// The runner folder for this connect, and credentials already settled for it. With a connect key and
/// no `--data-dir`, the key's account is matched to the runner already connected to it; another
/// account gets a runner of its own, so this computer serves both.
async fn account_runner(origin: &str, a: &ConnectArgs, name: &str) -> Result<(PathBuf, Option<Credentials>), String> {
    let (Some(key), None) = (a.key.as_deref(), &a.data_dir) else { return Ok((runner_dir(a.data_dir.clone())?, None)) };
    let base = runner_dir(None)?;
    match crate::runners::place_for_key(origin, key, &base).await? {
        crate::runners::Place::Reuse(dir, c) => Ok((dir, Some(c))),
        crate::runners::Place::Redeem(dir) => Ok((dir, None)),
        crate::runners::Place::NewAccount => {
            let c = redeem(origin, key, name, std::env::consts::OS).await?;
            let dir = runner_dir(Some(crate::runners::new_dir(&base, &c.device_id)))?;
            // The same folders and access as this computer's first runner, to begin with.
            if let Ok(text) = std::fs::read_to_string(base.join("runner.json")) {
                std::fs::write(dir.join("runner.json"), text).map_err(|e| e.to_string())?;
            }
            c.save(&dir)?;
            println!("This computer is already connected to another account. It now runs this account's agents too, from {}.", dir.display());
            Ok((dir, Some(c)))
        }
    }
}

/// `~/.sculptured/runner` unless `--data-dir` is given; created owner-only.
fn runner_dir(given: Option<PathBuf>) -> Result<PathBuf, String> {
    let dir = match given {
        Some(d) => d,
        None => hostd::platform::home().ok_or("your home folder is unknown; pass --data-dir")?.join(".sculptured").join("runner"),
    };
    hostd::run::private_data_dir(&dir)?;
    Ok(dir)
}

fn this_computer() -> String {
    std::fs::read_to_string("/etc/hostname").ok().map(|h| h.trim().to_string()).filter(|h| !h.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::process::Command::new("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|h| !h.is_empty()))
        .unwrap_or_else(|| "This computer".into())
}

pub async fn run_connect(a: ConnectArgs) -> Result<(), String> {
    tls_provider();
    let origin = a.origin.trim_end_matches('/').to_string();
    let url = host_url(&origin)?;
    let name = a.name.clone().unwrap_or_else(this_computer);
    let (dir, chosen) = account_runner(&origin, &a, &name).await?;
    let policy_file = dir.join("runner.json");
    let mut policy = hostd::policy::RunnerPolicy::load(&policy_file)?;
    for f in &a.allow { policy.allow(f)?; }
    if let Some(home) = hostd::platform::home() {
        default_to_home(&mut policy, &home)?;
    }
    if let Some(access) = a.access { policy.access = access; }
    set_caps(&mut policy, &a.caps)?;
    policy.save(&policy_file)?;
    let saved = Credentials::load(&dir)?;
    let from_key = match chosen { Some(c) => Some(c), None => credentials(&origin, saved.clone(), a.key.as_deref(), &name).await.transpose()? };
    let creds = match (from_key, saved) {
        (Some(c), _) => { c.save(&dir)?; c }
        (None, Some(c)) if c.origin == origin => c,
        _ => {
            let c = pair(&origin, &name, std::env::consts::OS, |code, link| {
                println!("To connect this computer, open:\n  {link}\nand confirm the code {code}");
                // The desktop app shows the link in its own window instead (`--no-browser`).
                if !a.no_browser {
                    let mut open = if cfg!(windows) {
                        let mut c = std::process::Command::new("cmd");
                        c.args(["/C", "start", ""]);
                        c
                    } else {
                        std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" })
                    };
                    let _ = open.arg(link).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
                }
            }).await?;
            c.save(&dir)?;
            c
        }
    };
    if a.install_service {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        // Full paths, found on PATH or where their installers put them: the login service's PATH may lack them.
        let found = |name: &str| Some(hostd::platform::program(name)).filter(|p| p.is_absolute());
        let claude = a.claude_program.clone().or_else(|| found("claude"));
        let codex = a.codex_program.clone().or_else(|| found("codex"));
        let args = service_args(&exe, &origin, &dir, claude.as_deref(), codex.as_deref());
        let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
        let suffix = crate::runners::service_suffix(&dir);
        let (file, text) = service_unit_for(&args, &path_env, suffix.as_deref());
        write_service_file(&file, &text)?;
        // The first command (reload, or stop the old one) may fail when nothing was installed yet; the rest must work.
        let run = |c: &Vec<String>| std::process::Command::new(&c[0]).args(&c[1..]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        let cmds = service_commands_for(&file, suffix.as_deref());
        let ok = cmds.iter().enumerate().fold(true, |ok, (i, c)| run(c) || i == 0 && ok);
        if ok {
            println!("Connected to {origin}. Sculptured now runs in the background and starts again whenever you log in.");
            println!("Agents may work in: {}", policy.allowed_folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", "));
        } else {
            println!("Wrote {}, but could not start it. Start it yourself, or run `sculptured connect {origin}` to stay connected while it runs.", file.display());
        }
        return Ok(());
    }
    println!("Connected to {origin}. Agents start here, in: {}", policy.allowed_folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", "));
    println!("{}", match policy.access {
        hostd::policy::Access::Full => "Access: full. Agents can act as you do on this computer. Use --sandboxed to limit them.",
        hostd::policy::Access::Sandboxed => "Access: sandboxed. Claude asks before risky actions; Codex writes only inside its folder.",
    });
    println!("Your account may also use here: {}", { let c = policy.info().capabilities; if c.is_empty() { "nothing else".to_string() } else { c.join(", ") } });
    let ran = hostd::run_hostd(hostd::HostdOptions {
        controld_url: url,
        host_id: creds.device_id.clone(),
        data_dir: dir.join("hostd"),
        token: format!("{}.{}", creds.device_id, creds.key),
        claude_program: a.claude_program,
        codex_program: a.codex_program,
        policy: Some(policy),
    }).await;
    if ran.as_ref().is_err_and(|e| e == hostd::DISCONNECTED) {
        service_off(&dir);
    }
    ran
}

/// `sculptured stop`: ends this computer's agents without the server: every account's runner here,
/// or only the one in `--data-dir`.
pub fn run_stop(data_dir: Option<PathBuf>) -> Result<(), String> {
    let dirs = match data_dir {
        Some(d) => vec![runner_dir(Some(d))?],
        None => { let base = runner_dir(None)?; std::iter::once(base.clone()).chain(crate::runners::other_dirs(&base)).collect() }
    };
    let mut ended = Vec::new();
    for d in dirs.iter().filter(|d| d.join("hostd").join("hostd.sqlite").exists()) {
        ended.extend(hostd::local_stop::run(d)?);
    }
    match ended.len() {
        0 => println!("No agent started by this computer's runner is running."),
        n => println!("Stopped {n} agent{}: {}", if n == 1 { "" } else { "s" }, ended.join(", ")),
    }
    Ok(())
}

pub fn run_allow(folder: &Path, data_dir: Option<PathBuf>) -> Result<(), String> {
    let dir = runner_dir(data_dir)?;
    let file = dir.join("runner.json");
    let mut policy = hostd::policy::RunnerPolicy::load(&file)?;
    let added = policy.allow(folder)?;
    policy.save(&file)?;
    println!("Agents may now work in {}.", added.display());
    println!("Allowed: {}", policy.allowed_folders.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", "));
    println!("Restart `sculptured connect` (or its service) so running agents see it.");
    Ok(())
}

/// Adds a component manifest's digest to the ones this computer's owner approved. The runner reads the
/// list at every launch, so running agents are not restarted.
pub fn run_approve_component(digest: &str, data_dir: Option<PathBuf>) -> Result<(), String> {
    let digest = digest.trim().to_ascii_lowercase();
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("That is not a component digest: copy the 64-character code from the agent's note.".into());
    }
    let dir = runner_dir(data_dir)?.join("hostd");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let file = dir.join(hostd::supervisor::APPROVED_COMPONENTS_FILE);
    let mut list: Vec<String> = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if !list.contains(&digest) {
        list.push(digest);
    }
    hostd::private::write_file(&file, serde_json::to_string_pretty(&list).expect("serializes").as_bytes()).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    println!("Approved. Agents given this component start it at their next launch; nothing is restarted.");
    Ok(())
}

/// Every runner's data folder on this computer, or only the one given.
pub fn runner_dirs(data_dir: Option<PathBuf>) -> Result<Vec<PathBuf>, String> {
    Ok(match data_dir {
        Some(d) => vec![runner_dir(Some(d))?],
        None => { let base = runner_dir(None)?; std::iter::once(base.clone()).chain(crate::runners::other_dirs(&base)).collect() }
    })
}

/// `sculptured set-secret NAME`: keeps a component's secret for every runner on this computer (or the
/// one in `--data-dir`), read at each launch. The value comes from the terminal unechoed, or from stdin.
pub fn run_set_secret(name: &str, remove: bool, data_dir: Option<PathBuf>) -> Result<(), String> {
    let name = name.trim().trim_start_matches("${").trim_end_matches('}');
    if model::component::placeholders(&format!("${{{name}}}")).map_or(true, |n| n.len() != 1) {
        return Err(format!("{name:?} is not a secret's name: use the capitals the component shows, such as OPENFOAM_TOKEN."));
    }
    let value = if remove {
        None
    } else {
        use std::io::IsTerminal;
        let text = if std::io::stdin().is_terminal() {
            rpassword::prompt_password(format!("Value for {name} (not shown): ")).map_err(|e| format!("cannot read the value: {e}"))?
        } else {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).map_err(|e| format!("cannot read the value: {e}"))?;
            t
        };
        let text = text.trim_end_matches(['\r', '\n']).to_string();
        if text.is_empty() || text.contains(['\r', '\n']) {
            return Err("The value must be one non-empty line.".into());
        }
        Some(text)
    };
    let dirs = match data_dir {
        Some(d) => vec![runner_dir(Some(d))?],
        None => { let base = runner_dir(None)?; std::iter::once(base.clone()).chain(crate::runners::other_dirs(&base)).collect() }
    };
    for d in &dirs {
        set_secret_in(&d.join("hostd"), name, value.as_deref())?;
    }
    match value {
        Some(_) => println!("Saved {name} on this computer. Agents given a component that names it pick it up at their next start."),
        None => println!("Removed {name} from this computer."),
    }
    Ok(())
}

fn set_secret_in(dir: &Path, name: &str, value: Option<&str>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let file = dir.join(hostd::supervisor::COMPONENT_SECRETS_FILE);
    let mut all: std::collections::BTreeMap<String, String> = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    match value {
        Some(v) => all.insert(name.to_string(), v.to_string()),
        None => all.remove(name),
    };
    hostd::private::write_file(&file, serde_json::to_string_pretty(&all).expect("serializes").as_bytes()).map_err(|e| format!("cannot write {}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::sync::{Arc, atomic::{AtomicU32, Ordering}};

    #[test]
    fn a_secret_is_kept_privately_and_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        set_secret_in(dir.path(), "OPENFOAM_TOKEN", Some("s3cret")).unwrap();
        set_secret_in(dir.path(), "OTHER", Some("x")).unwrap();
        let file = dir.path().join(hostd::supervisor::COMPONENT_SECRETS_FILE);
        let read = || serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(read()["OPENFOAM_TOKEN"], "s3cret");
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&file).unwrap().permissions()) & 0o777, 0o600);
        set_secret_in(dir.path(), "OPENFOAM_TOKEN", None).unwrap();
        assert_eq!(read(), serde_json::json!({ "OTHER": "x" }));
        assert!(run_set_secret("lower", true, Some(dir.path().into())).is_err(), "a name must be in capitals");
    }

    #[test]
    fn host_urls_use_wss_except_on_this_machine() {
        assert_eq!(host_url("https://app.example.com").unwrap(), "wss://app.example.com/host");
        assert_eq!(host_url("http://127.0.0.1:4430/").unwrap(), "ws://127.0.0.1:4430/host");
        assert!(host_url("http://app.example.com").is_err(), "plain http to another machine would expose the key");
        assert!(host_url("http://localhost:1@evil.example").is_err(), "credentials-in-URL tricks are not this machine");
        assert!(host_url("https://user@app.example.com").is_err());
    }

    #[tokio::test]
    async fn pairing_shows_the_code_waits_for_approval_and_saves_private_credentials() {
        let polls = Arc::new(AtomicU32::new(0));
        let p2 = polls.clone();
        let app = Router::new()
            .route("/gw/api/device/start", post(|Json(b): Json<Value>| async move {
                assert_eq!(b["platform"], std::env::consts::OS);
                Json(json!({"device_code":"d".repeat(64),"user_code":"WXYZ-2345","verify_url":"http://127.0.0.1/connect?code=WXYZ-2345","interval":0,"expires_in":600}))
            }))
            .route("/gw/api/device/token", post(move || { let n = p2.fetch_add(1, Ordering::SeqCst); async move {
                if n < 2 { (axum::http::StatusCode::PRECONDITION_REQUIRED, Json(json!({"error":{"code":"authorization_pending","message":"wait"}}))) }
                else { (axum::http::StatusCode::OK, Json(json!({"device_id":"11111111-1111-1111-1111-111111111111","device_key":"k".repeat(64)}))) }
            }}));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let shown = Arc::new(std::sync::Mutex::new(String::new()));
        let s2 = shown.clone();
        let creds = pair(&origin, "Laptop", std::env::consts::OS, move |code, _url| *s2.lock().unwrap() = code.to_string()).await.unwrap();
        assert_eq!(&*shown.lock().unwrap(), "WXYZ-2345");
        assert_eq!(polls.load(Ordering::SeqCst), 3);
        let dir = tempfile::tempdir().unwrap();
        creds.save(dir.path()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(dir.path().join("device.json")).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(Credentials::load(dir.path()).unwrap(), Some(creds));
    }

    #[tokio::test]
    async fn a_connect_key_is_traded_for_this_computers_own_key_once() {
        let app = Router::new().route("/gw/api/device/redeem", post(|Json(b): Json<Value>| async move {
            assert_eq!((b["name"].as_str(), b["platform"].as_str()), (Some("Laptop"), Some(std::env::consts::OS)));
            if b["key"] == "k".repeat(64) {
                (axum::http::StatusCode::OK, Json(json!({"device_id":"11111111-1111-1111-1111-111111111111","device_key":"d".repeat(64)})))
            } else {
                (axum::http::StatusCode::GONE, Json(json!({"error":{"code":"key_used","message":"This connect command was already used or has expired. Copy a new one from the Computers page."}})))
            }
        }));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let creds = redeem(&origin, &"k".repeat(64), "Laptop", std::env::consts::OS).await.unwrap();
        assert_eq!((creds.origin.as_str(), creds.key.len()), (origin.as_str(), 64));
        let err = redeem(&origin, &"x".repeat(64), "Laptop", std::env::consts::OS).await.unwrap_err();
        assert!(err.contains("already used or has expired"), "{err}");
    }

    #[tokio::test]
    async fn a_disconnected_computer_reconnects_with_the_new_key_in_its_command() {
        let app = Router::new()
            .route("/gw/api/device/check", axum::routing::get(|h: axum::http::HeaderMap| async move {
                let ok = h.get("authorization").is_some_and(|v| v == "Bearer live-id.live-key");
                if ok { axum::http::StatusCode::OK } else { axum::http::StatusCode::UNAUTHORIZED }
            }))
            .route("/gw/api/device/redeem", post(|| async { Json(json!({"device_id":"new-id","device_key":"n".repeat(64)})) }));
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        let saved = |id: &str, key: &str| Some(Credentials { origin: origin.clone(), device_id: id.into(), key: key.into() });
        let live = credentials(&origin, saved("live-id", "live-key"), Some("k"), "PC").await.unwrap().unwrap();
        assert_eq!(live.device_id, "live-id", "a key that still works is kept: re-running does not add a second computer");
        let revoked = credentials(&origin, saved("old-id", "old-key"), Some("k"), "PC").await.unwrap().unwrap();
        assert_eq!(revoked.device_id, "new-id", "a disconnected computer uses the new key");
        let fresh = credentials(&origin, None, Some("k"), "PC").await.unwrap().unwrap();
        assert_eq!(fresh.device_id, "new-id");
    }

    #[test]
    fn a_new_computer_lets_agents_work_in_its_home_folder() {
        let home = tempfile::tempdir().unwrap();
        let mut fresh = hostd::policy::RunnerPolicy::default();
        assert!(default_to_home(&mut fresh, home.path()).unwrap(), "nothing allowed yet: home is");
        assert_eq!(fresh.allowed_folders, [home.path().canonicalize().unwrap()]);
        let mut chosen = hostd::policy::RunnerPolicy::default();
        let projects = home.path().join("projects");
        std::fs::create_dir(&projects).unwrap();
        chosen.allow(&projects).unwrap();
        assert!(!default_to_home(&mut chosen, home.path()).unwrap(), "folders already chosen are kept as they are");
        assert_eq!(chosen.allowed_folders.len(), 1);
    }

    #[test]
    fn another_accounts_runner_is_a_service_of_its_own() {
        let args = vec!["/x/sculptured".to_string(), "connect".into()];
        let (file, text) = service_unit_for(&args, "/usr/bin", Some("b1c2d3e4"));
        let cmds = service_commands_for(&file, Some("b1c2d3e4"));
        if cfg!(target_os = "linux") {
            assert!(file.ends_with(".config/systemd/user/sculptured-runner-b1c2d3e4.service"), "{}", file.display());
            assert_eq!(cmds.last().unwrap(), &["systemctl", "--user", "restart", "sculptured-runner-b1c2d3e4"]);
        }
        if cfg!(target_os = "macos") {
            assert!(file.ends_with("Library/LaunchAgents/dev.sculptured.runner.b1c2d3e4.plist") && text.contains("<string>dev.sculptured.runner.b1c2d3e4</string>"));
        }
        let (first, _) = service_unit_for(&args, "/usr/bin", None);
        assert_ne!(first, file, "the first account's service is left as it is");
        let _ = text;
    }

    #[test]
    fn installing_again_restarts_the_service_so_the_new_runner_takes_over() {
        let cmds = service_commands(Path::new("/home/a/.config/systemd/user/sculptured-runner.service"));
        if cfg!(target_os = "linux") {
            assert_eq!(cmds.last().unwrap(), &["systemctl", "--user", "restart", "sculptured-runner"]);
            assert!(cmds.iter().any(|c| c[..] == ["systemctl", "--user", "enable", "sculptured-runner"]));
        }
        if cfg!(target_os = "macos") {
            assert_eq!(cmds[0][..2], ["launchctl", "unload"]);
            assert_eq!(cmds[1][..3], ["launchctl", "load", "-w"]);
        }
    }

    #[test]
    fn on_windows_a_logon_task_runs_the_runner_without_a_window_and_restarts_it() {
        let args = vec![r"C:\Users\Ana\AppData\Local\Programs\Sculptured\sculptured.exe".to_string(), "connect".into(), "https://app.x".into(), "--data-dir".into(), r"C:\Users\Ana\.sculptured\runner".into()];
        let xml = windows_task(&args, r"PC\Ana");
        assert!(xml.contains(r"<Command>C:\Users\Ana\AppData\Local\Programs\Sculptured\sculptured-background.exe</Command>"), "{xml}");
        assert!(xml.contains(r#"<Arguments>connect https://app.x --data-dir "C:\Users\Ana\.sculptured\runner"</Arguments>"#) || xml.contains(r"<Arguments>connect https://app.x --data-dir C:\Users\Ana\.sculptured\runner</Arguments>"), "{xml}");
        assert!(xml.contains(r"<LogonTrigger><Enabled>true</Enabled><UserId>PC\Ana</UserId></LogonTrigger>"), "{xml}");
        assert!(xml.contains("<RestartOnFailure><Interval>PT1M</Interval><Count>999</Count></RestartOnFailure>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"), "never stopped for running long");
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>") && xml.contains("<RunLevel>LeastPrivilege</RunLevel>"), "as this user, no admin");
    }

    #[test]
    fn the_login_service_runs_connect_for_this_origin_and_finds_the_clis_after_a_reboot() {
        let args = service_args(Path::new("/opt/My Apps/sculptured"), "https://app.x", Path::new("/home/a/.sculptured/runner"),
            Some(Path::new("/home/a/.local/bin/claude")), Some(Path::new("/opt/homebrew/bin/codex")));
        assert_eq!(args, ["/opt/My Apps/sculptured", "connect", "https://app.x", "--data-dir", "/home/a/.sculptured/runner",
            "--claude-program", "/home/a/.local/bin/claude", "--codex-program", "/opt/homebrew/bin/codex"]);
        let (file, text) = service_unit(&args, "/usr/bin:/home/a/.local/bin:/a&b");
        if cfg!(target_os = "linux") {
            assert!(file.ends_with(".config/systemd/user/sculptured-runner.service"));
            assert!(text.contains(r#"ExecStart="/opt/My Apps/sculptured" "connect""#), "{text}");
            assert!(text.contains(r#"Environment="PATH=/usr/bin:/home/a/.local/bin:/a&b""#), "{text}");
            assert!(text.contains("Restart=always"));
            assert!(text.contains(&format!("RestartPreventExitStatus={DISCONNECTED_EXIT}")), "a disconnected computer is not restarted: {text}");
        }
        if cfg!(target_os = "macos") {
            assert!(file.ends_with("Library/LaunchAgents/dev.sculptured.runner.plist"));
            assert!(text.contains("<string>/opt/My Apps/sculptured</string>") && text.contains("/a&amp;b") && text.contains("<key>KeepAlive</key>"));
        }
    }
}

#[test]
fn a_disconnected_runner_turns_its_own_service_off() {
    let off = service_off_commands(Path::new("/home/a/.config/systemd/user/sculptured-runner-b.service"), Some("b"));
    if cfg!(target_os = "linux") {
        assert_eq!(off, [["systemctl", "--user", "disable", "--now", "--no-block", "sculptured-runner-b"]]);
    }
    if cfg!(target_os = "macos") {
        assert_eq!(off[0][..3], ["launchctl", "unload", "-w"]);
    }
}

#[cfg(test)]
mod caps_tests {
    #[test]
    fn capabilities_turn_on_and_off_by_name() {
        let mut p = hostd::policy::RunnerPolicy::default();
        super::set_caps(&mut p, &[("checks".into(), false), ("git".into(), false)]).unwrap();
        assert!(!p.caps().checks && !p.caps().git && p.caps().copies);
        super::set_caps(&mut p, &[("git".into(), true)]).unwrap();
        assert!(p.caps().git);
        assert!(super::set_caps(&mut p, &[("rm".into(), true)]).is_err());
        super::set_caps(&mut p, &[("generate".into(), false)]).unwrap();
        assert!(!p.caps().generate);
    }

    #[test]
    fn switching_to_sandboxed_later_turns_checks_off_unless_enabled_by_name() {
        let mut p = hostd::policy::RunnerPolicy::default();
        super::set_caps(&mut p, &[("git".into(), false)]).unwrap();
        p.access = hostd::policy::Access::Sandboxed;
        assert!(!p.caps().checks, "checks follow the access level unless the owner set them");
        assert!(!p.caps().git, "the owner's own choice stays");
        super::set_caps(&mut p, &[("checks".into(), true)]).unwrap();
        assert!(p.caps().checks);
        let saved: hostd::policy::RunnerPolicy = serde_json::from_str(r#"{"access":"sandboxed","capabilities":{"git":false}}"#).unwrap();
        assert!(!saved.caps().git && !saved.caps().checks && saved.caps().copies, "a file naming only some capabilities loads");
    }
}
