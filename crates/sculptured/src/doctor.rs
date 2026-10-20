//! `sculptured doctor`: what this computer has for its agents, in plain lines: Claude Code and Codex
//! (installed, signed in), each runner's allowed folders, the built-in components' programs, and the
//! component approvals and secrets kept here (names only, never values). It changes nothing. An idea
//! from Gas Town's `gt doctor`.
use std::path::{Path, PathBuf};

/// One line of the report: what was checked, whether it is fine (None = only information), and why.
#[derive(Debug, PartialEq)]
pub struct Check {
    pub what: String,
    pub ok: Option<bool>,
    pub detail: String,
}

fn check(what: impl Into<String>, ok: Option<bool>, detail: impl Into<String>) -> Check {
    Check { what: what.into(), ok, detail: detail.into() }
}

pub async fn run(dirs: Vec<PathBuf>) -> Result<(), String> {
    let mut checks = Vec::new();
    for name in ["claude", "codex"] {
        let program = hostd::supervisor::find_program(name).unwrap_or_else(|| PathBuf::from(name));
        let s = localops::clis::status(name, &program).await;
        let label = if name == "claude" { "Claude Code" } else { "Codex" };
        checks.push(match (s.installed, s.signed_in) {
            (false, _) => check(label, Some(false), "not installed"),
            (true, Some(false)) => check(label, Some(false), format!("{} installed, not signed in", s.version.unwrap_or_default())),
            (true, signed) => check(label, Some(true), format!("{}{}", s.version.unwrap_or_default(), if signed == Some(true) { s.account.map(|a| format!(", signed in ({a})")).unwrap_or_else(|| ", signed in".into()) } else { String::new() })),
        });
    }
    for c in model::component::builtins() {
        let found = hostd::supervisor::find_program(&c.program);
        checks.push(check(format!("{} ({})", c.name, c.program), found.is_some().then_some(true), match found {
            Some(p) => p.display().to_string(),
            None => format!("not installed; only needed if a team gives it to an agent: {}", c.install),
        }));
    }
    let origin = dirs.iter().find_map(|d| crate::connect::Credentials::load(d).ok().flatten()).map(|c| c.origin);
    checks.extend(where_it_runs(origin.as_deref()).await);
    for d in &dirs {
        checks.extend(runner_checks(d));
    }
    println!("{}", report(&checks));
    Ok(())
}

/// Whether the runner keeps running after you log out, what this computer can reach, and its proxy
/// settings: what a shared computer such as a cluster's login node needs to know.
async fn where_it_runs(origin: Option<&str>) -> Vec<Check> {
    let quiet = |cmd: &str, args: &[&str]| std::process::Command::new(cmd).args(args).output().ok();
    let systemd_user = quiet("systemctl", &["--user", "show-environment"]).is_some_and(|o| o.status.success());
    let user = std::env::var("USER").unwrap_or_default();
    let linger = quiet("loginctl", &["show-user", &user, "-p", "Linger", "--value"])
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes");
    let mut out = vec![check("This computer", None, ""), keeps_running(std::env::consts::OS, systemd_user, linger, origin.unwrap_or("<your Sculptured address>"))];
    let client = crate::connect::http();
    let mut targets = vec![("Anthropic (Claude Code)".to_string(), "api.anthropic.com".to_string()), ("OpenAI (Codex)".to_string(), "api.openai.com".to_string())];
    if let Some(host) = origin.and_then(|o| reqwest::Url::parse(o).ok()).and_then(|u| u.host_str().map(str::to_string)) {
        targets.insert(0, ("Your Sculptured account".into(), host));
    }
    for (what, host) in targets {
        let got = client.head(format!("https://{host}/")).timeout(std::time::Duration::from_secs(8)).send().await;
        out.push(reach(&what, &host, got.map(|_| ()).map_err(|e| if e.is_timeout() { "timed out".to_string() } else { "no connection".to_string() })));
    }
    let names = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "NO_PROXY"];
    let values: Vec<(&str, Option<String>)> = names.iter().map(|n| (*n, std::env::var(n).ok())).collect();
    out.push(proxies(&values.iter().map(|(n, v)| (*n, v.as_deref())).collect::<Vec<_>>()));
    out
}

/// Linux: a user service runs after logout only with lingering; without user services, tmux (or the
/// site's own way, such as scrontab) keeps it. macOS and Windows keep their login item as they are.
pub fn keeps_running(os: &str, systemd_user: bool, linger: Option<bool>, origin: &str) -> Check {
    let what = "Keeps running after you log out";
    if os != "linux" {
        return check(what, None, "while you are logged in, through the login item `sculptured connect --service` sets up");
    }
    let tmux = format!("tmux new -d -s sculptured 'sculptured connect {origin}'");
    match (systemd_user, linger) {
        (true, Some(true)) => check(what, Some(true), "yes: user services run here after you log out (lingering is on)"),
        (true, _) => check(what, None, format!("no: the runner stops when you log out (fine on your own computer). On a shared server, run `loginctl enable-linger` if it is allowed there, or keep it in tmux: {tmux}")),
        (false, _) => check(what, Some(false), format!("no user services here (common on clusters). Keep the runner in tmux: {tmux}, or use your site's scheduled jobs (such as scrontab), only where the site allows long-running programs")),
    }
}

/// One address this computer needs: reached, or why agents here cannot work.
pub fn reach(what: &str, host: &str, got: Result<(), String>) -> Check {
    match got {
        Ok(()) => check(format!("Reaches {what} ({host})"), Some(true), "reachable"),
        Err(e) => check(format!("Reaches {what} ({host})"), Some(false), format!("{e}: agents here cannot work. Compute nodes often have no internet; ask your site about a proxy")),
    }
}

/// Which proxy variables are set (never their values, which can hold a password).
pub fn proxies(vars: &[(&str, Option<&str>)]) -> Check {
    let set: Vec<&str> = vars.iter().filter(|(_, v)| v.is_some_and(|v| !v.is_empty())).map(|(n, _)| *n).collect();
    check("Proxy settings", None, if set.is_empty() { "none".to_string() } else { format!("{} set", set.join(", ")) })
}

/// What one runner's data folder says: its folders, approvals and secrets.
pub fn runner_checks(dir: &Path) -> Vec<Check> {
    let where_ = dir.display().to_string();
    let mut out = vec![check(format!("Runner {where_}"), None, "")];
    match hostd::policy::RunnerPolicy::load(&dir.join("runner.json")) {
        Ok(p) if p.allowed_folders.is_empty() => out.push(check("Allowed folders", Some(false), "none yet: run sculptured allow <folder>")),
        Ok(p) => {
            for f in &p.allowed_folders {
                out.push(check(format!("Allowed folder {}", f.display()), Some(f.is_dir()), if f.is_dir() { "exists" } else { "missing on this computer" }));
            }
        }
        Err(e) => out.push(check("Runner settings", Some(false), e)),
    }
    let hostd = dir.join("hostd");
    let read = |file: &str| std::fs::read_to_string(hostd.join(file)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let approved = read(hostd::supervisor::APPROVED_COMPONENTS_FILE).and_then(|v| v.as_array().map(Vec::len)).unwrap_or(0);
    out.push(check("Components approved here", None, approved.to_string()));
    let secrets: Vec<String> = read(hostd::supervisor::COMPONENT_SECRETS_FILE).and_then(|v| v.as_object().map(|o| o.keys().cloned().collect())).unwrap_or_default();
    out.push(check("Component secrets set here", None, if secrets.is_empty() { "none".to_string() } else { secrets.join(", ") }));
    out
}

pub fn report(checks: &[Check]) -> String {
    checks.iter().map(|c| {
        let mark = match c.ok { Some(true) => "ok  ", Some(false) => "fix ", None => "    " };
        if c.detail.is_empty() { format!("\n{}", c.what) } else { format!("{mark} {}: {}", c.what, c.detail) }
    }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runner_folder_is_reported_with_its_folders_approvals_and_secret_names_only() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        std::fs::write(dir.path().join("runner.json"), serde_json::json!({ "allowed_folders": [dir.path(), gone] }).to_string()).unwrap();
        std::fs::create_dir_all(dir.path().join("hostd")).unwrap();
        std::fs::write(dir.path().join("hostd").join(hostd::supervisor::COMPONENT_SECRETS_FILE), r#"{"OPENFOAM_TOKEN":"s3cret"}"#).unwrap();
        let checks = runner_checks(dir.path());
        assert_eq!(checks[1].ok, Some(true));
        assert_eq!(checks[2], check(format!("Allowed folder {}", gone.display()), Some(false), "missing on this computer"));
        let text = report(&checks);
        assert!(text.contains("Component secrets set here: OPENFOAM_TOKEN") && !text.contains("s3cret"), "{text}");
        assert!(text.contains("fix  Allowed folder"), "{text}");
    }

    #[test]
    fn a_runner_with_no_folders_says_how_to_allow_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("runner.json"), "{}").unwrap();
        assert!(runner_checks(dir.path())[1].detail.contains("sculptured allow"));
    }

    #[test]
    fn a_login_node_without_user_services_is_told_how_to_keep_the_runner_alive() {
        let c = keeps_running("linux", false, None, "https://app.example.com");
        assert_eq!(c.ok, Some(false));
        assert!(c.detail.contains("tmux new -d -s sculptured 'sculptured connect https://app.example.com'") && c.detail.contains("allows"), "{}", c.detail);
        let c = keeps_running("linux", true, Some(false), "https://app.example.com");
        assert_eq!(c.ok, None, "on your own computer that is how it should be");
        assert!(c.detail.contains("loginctl enable-linger") && c.detail.contains("log out"), "{}", c.detail);
        assert_eq!(keeps_running("linux", true, Some(true), "https://a").ok, Some(true));
        assert_eq!(keeps_running("macos", false, None, "https://a").ok, None, "macOS and Windows keep it running after login as they are");
    }

    #[test]
    fn what_this_computer_can_reach_is_named_with_why_it_matters() {
        let ok = reach("Anthropic (Claude Code)", "api.anthropic.com", Ok(()));
        assert_eq!((ok.ok, ok.detail.as_str()), (Some(true), "reachable"));
        let no = reach("Anthropic (Claude Code)", "api.anthropic.com", Err("timed out".into()));
        assert_eq!(no.ok, Some(false));
        assert!(no.detail.contains("timed out") && no.detail.contains("proxy"), "{}", no.detail);
    }

    #[test]
    fn proxy_settings_are_named_never_shown() {
        let c = proxies(&[("HTTPS_PROXY", Some("http://user:pw@proxy:3128")), ("NO_PROXY", Some("localhost")), ("HTTP_PROXY", None)]);
        assert_eq!(c.detail, "HTTPS_PROXY, NO_PROXY set");
        assert!(!c.detail.contains("pw"));
        assert_eq!(proxies(&[("HTTPS_PROXY", None)]).detail, "none");
    }

}
