//! The Claude Code and Codex CLIs on this computer: installed, version, signed in (as this OS user),
//! and a sign-in started from the browser, which works from any browser, not only this computer's:
//! Codex signs in with a one-time code entered on its page, and Claude Code takes back the code its
//! page shows (`login_code`). The CLI runs its own sign-in; nothing stores credentials.
use crate::LocalError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CliStatus {
    /// `claude` or `codex`.
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    /// None when the CLI did not say.
    pub signed_in: Option<bool>,
    /// The account it names (an email, or the sign-in method), verbatim.
    pub account: Option<String>,
    /// The CLI's own status output, verbatim (evidence).
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LoginStarted {
    /// The sign-in link the CLI printed, if any.
    pub url: Option<String>,
    /// A one-time code to enter on the sign-in page (Codex's sign-in from another device).
    #[serde(default)]
    pub code: Option<String>,
    /// The CLI takes a code pasted back from its sign-in page (`login_code`): Claude Code shows one
    /// when the page cannot reach this computer.
    #[serde(default)]
    pub takes_code: bool,
    /// What the CLI printed so far, without colours (evidence).
    pub output: String,
}

/// Sign-ins under way that take a pasted code: the CLI's input, by CLI name, with the sign-in's number.
static PENDING: LazyLock<Mutex<HashMap<String, (u64, ChildStdin)>>> =
    LazyLock::new(Default::default);

/// Runs `program args` with a timeout; Some((success, stdout + stderr)) when it ran at all.
async fn run(program: &std::path::Path, args: &[&str]) -> Option<(bool, String)> {
    let out = Command::new(crate::programs::resolve(program))
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(Duration::from_secs(15), out)
        .await
        .ok()?
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Some((out.status.success(), text.trim().to_string()))
}

pub async fn status(name: &str, program: &Path) -> CliStatus {
    let Some((_, version)) = run(program, &["--version"]).await else {
        let detail = format!("{} was not found", program.display());
        return CliStatus {
            name: name.into(),
            installed: false,
            version: None,
            signed_in: None,
            account: None,
            detail,
        };
    };
    let args: &[&str] = if name == "claude" {
        &["auth", "status"]
    } else {
        &["login", "status"]
    };
    let (ok, detail) = run(program, args)
        .await
        .unwrap_or((false, "status did not answer".into()));
    let (signed_in, account) = if name == "claude" {
        claude_auth(&detail)
    } else {
        codex_auth(ok, &detail)
    };
    CliStatus {
        name: name.into(),
        installed: true,
        version: Some(version),
        signed_in,
        account,
        detail,
    }
}

/// `claude auth status` prints JSON with `loggedIn` and `email` (or the auth method).
fn claude_auth(text: &str) -> (Option<bool>, Option<String>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return (None, None);
    };
    let account = v["email"]
        .as_str()
        .or(v["authMethod"].as_str())
        .map(str::to_string);
    (v["loggedIn"].as_bool(), account)
}

/// `codex login status` exits 0 with "Logged in using …" when signed in.
fn codex_auth(ok: bool, text: &str) -> (Option<bool>, Option<String>) {
    let line = text.lines().find(|l| l.starts_with("Logged in"));
    match (ok, line) {
        (true, Some(l)) => (
            Some(true),
            l.strip_prefix("Logged in using ").map(str::to_string),
        ),
        (false, _) if text.contains("Not logged in") => (Some(false), None),
        (false, _) => (None, None),
        (true, None) => (Some(true), None),
    }
}

/// Starts the CLI's sign-in and returns the link (and code) it prints within a few seconds. The CLI
/// keeps running until the sign-in completes (or after 10 minutes is stopped); the page re-reads the status.
pub async fn login(name: &str, program: &Path) -> Result<LoginStarted, LocalError> {
    match name {
        "claude" => start(name, program, &["auth", "login", "--claudeai"]).await,
        "codex" => {
            let s = start(name, program, &["login", "--device-auth"]).await?;
            // A workspace may turn sign-in codes off; then only the browser sign-in is left.
            if s.url.is_none() && s.output.contains("not enabled") {
                start(name, program, &["login"]).await
            } else {
                Ok(s)
            }
        }
        _ => Err(LocalError::not_found(
            "not_found",
            format!("no CLI {name:?}"),
        )),
    }
}

/// Passes the code from the CLI's sign-in page to the sign-in under way.
pub async fn login_code(name: &str, code: &str) -> Result<(), LocalError> {
    let code = code.trim();
    if code.is_empty() || code.len() > 1024 || code.chars().any(char::is_control) {
        return Err(LocalError::bad(
            "bad_code",
            "Paste the code exactly as the sign-in page shows it, on one line.",
        ));
    }
    let taken = PENDING.lock().expect("pending sign-ins").remove(name);
    let Some((n, mut input)) = taken else {
        return Err(LocalError::conflict(
            "no_sign_in",
            "That sign-in has ended. Press Sign in to start again.",
        ));
    };
    input
        .write_all(format!("{code}\n").as_bytes())
        .await
        .map_err(|e| LocalError::conflict("no_sign_in", format!("The sign-in stopped: {e}")))?;
    input.flush().await.ok();
    PENDING
        .lock()
        .expect("pending sign-ins")
        .entry(name.to_string())
        .or_insert((n, input));
    Ok(())
}

async fn start(name: &str, program: &Path, args: &[&str]) -> Result<LoginStarted, LocalError> {
    let mut child = spawn(program, args)
        .await
        .map_err(|e| LocalError::conflict("cli_missing", format!("{name} could not start: {e}")))?;
    let (out, err) = (
        child.stdout.take().expect("piped"),
        child.stderr.take().expect("piped"),
    );
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if let Some(input) = child.stdin.take() {
        PENDING
            .lock()
            .expect("pending sign-ins")
            .insert(name.to_string(), (n, input));
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    for stream in [
        Box::new(out) as Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        Box::new(err),
    ] {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let _ = tx.send(plain(&l));
            }
        });
    }
    // The output ends when both streams do (the CLI exited), not at the deadline.
    drop(tx);
    let key = name.to_string();
    tokio::spawn(async move {
        let _ = tokio::time::timeout(Duration::from_secs(600), child.wait()).await;
        let mut pending = PENDING.lock().expect("pending sign-ins");
        if pending.get(&key).is_some_and(|(m, _)| *m == n) {
            pending.remove(&key);
        }
    });
    let mut s = first_link(&mut rx).await;
    s.code = one_time_code(&s.output);
    // The prompt has no line end, so it shows up only once the CLI exits; Claude Code always offers it.
    s.takes_code = name == "claude";
    Ok(s)
}

/// Starts the CLI, trying again for a moment while its file is busy (being written, as when it updates itself).
async fn spawn(program: &Path, args: &[&str]) -> std::io::Result<tokio::process::Child> {
    let mut tries = 0;
    loop {
        let r = Command::new(crate::programs::resolve(program))
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        match r {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 20 => {
                tries += 1
            }
            r => return r,
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The line after "one-time code", when it looks like one (`ABCD-12345`).
fn one_time_code(output: &str) -> Option<String> {
    let mut lines = output
        .lines()
        .skip_while(|l| !l.contains("one-time code"))
        .skip(1)
        .map(str::trim)
        .filter(|l| !l.is_empty());
    let code = lines.next()?;
    let shaped = (4..=24).contains(&code.len())
        && code.contains('-')
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    shaped.then(|| code.to_string())
}

/// `line` without terminal colour codes (`ESC [ … letter`).
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                let matching_chars = chars.by_ref();
                for c in matching_chars {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Collects output until a sign-in link appears (then a moment more, for a code) or 8 seconds pass.
/// A `https://` link wins over the CLI's own `http://localhost` callback address.
async fn first_link(rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> LoginStarted {
    let mut output = String::new();
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let (mut url, mut local) = (None, None);
    while let Ok(Some(line)) = tokio::time::timeout_at(deadline, rx.recv()).await {
        if url.is_none() {
            url = line
                .split_whitespace()
                .find(|w| w.starts_with("https://"))
                .map(str::to_string);
            if url.is_some() {
                deadline = tokio::time::Instant::now() + Duration::from_millis(800);
            }
        }
        if local.is_none() {
            local = line
                .split_whitespace()
                .find(|w| w.starts_with("http://localhost"))
                .map(|w| w.trim_end_matches('.').to_string());
        }
        output.push_str(&line);
        output.push('\n');
    }
    let url = url.or(local);
    LoginStarted {
        url,
        code: None,
        takes_code: false,
        output,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_cli_status_in_its_own_words() {
        let claude = r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "a@b.c"}"#;
        assert_eq!(claude_auth(claude), (Some(true), Some("a@b.c".into())));
        assert_eq!(claude_auth("not json"), (None, None));
        assert_eq!(
            codex_auth(true, "Logged in using ChatGPT"),
            (Some(true), Some("ChatGPT".into()))
        );
        assert_eq!(codex_auth(false, "Not logged in"), (Some(false), None));
    }

    /// A fake CLI: a shell script in its own folder.
    #[cfg(unix)]
    fn fake(dir: &Path, body: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("cli");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_signs_in_with_a_one_time_code_entered_from_any_browser() {
        let d = tempfile::tempdir().unwrap();
        let args = d.path().join("args");
        let cli = fake(
            d.path(),
            &format!(
                "echo \"$@\" > {}\nprintf 'Follow these steps to sign in with ChatGPT using device code authorization:\\n\\n1. Open this link in your browser and sign in to your account\\n   \\033[94mhttps://auth.openai.com/codex/device\\033[0m\\n\\n2. Enter this one-time code (expires in 15 minutes)\\n   \\033[94mABCD-12345\\033[0m\\n'\nsleep 3",
                args.display()
            ),
        );
        let s = login("codex", &cli).await.unwrap();
        assert_eq!(
            s.url.as_deref(),
            Some("https://auth.openai.com/codex/device")
        );
        assert_eq!(s.code.as_deref(), Some("ABCD-12345"));
        assert!(!s.takes_code);
        assert_eq!(
            std::fs::read_to_string(args).unwrap().trim(),
            "login --device-auth"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_falls_back_to_the_browser_sign_in_where_codes_are_off() {
        let d = tempfile::tempdir().unwrap();
        let cli = fake(
            d.path(),
            "if [ \"$2\" = --device-auth ]; then echo 'device code login is not enabled for this Codex server. Use the browser login or verify the server URL.' >&2; exit 1; fi\necho 'Starting local login server on http://localhost:1455.'\necho 'If your browser did not open, navigate to this URL to authenticate:'\necho 'https://auth.openai.com/oauth/authorize?client_id=x'\nsleep 3",
        );
        let s = login("codex", &cli).await.unwrap();
        assert_eq!(
            s.url.as_deref(),
            Some("https://auth.openai.com/oauth/authorize?client_id=x")
        );
        assert_eq!(s.code, None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_takes_the_code_its_sign_in_page_shows() {
        let d = tempfile::tempdir().unwrap();
        let got = d.path().join("got");
        let cli = fake(
            d.path(),
            &format!(
                "echo \"Browser didn't open? Use the url below to sign in\"\necho\necho 'https://claude.ai/oauth/authorize?code=true&x=1'\necho\nprintf 'Paste code here if prompted > '\nread c\necho \"$c\" > {}",
                got.display()
            ),
        );
        let s = login("claude", &cli).await.unwrap();
        assert_eq!(
            s.url.as_deref(),
            Some("https://claude.ai/oauth/authorize?code=true&x=1")
        );
        assert!(s.takes_code);
        assert!(
            login_code("claude", "two\nlines").await.is_err(),
            "one line only"
        );
        assert!(
            login_code("claude", &"x".repeat(2000)).await.is_err(),
            "too long"
        );
        login_code("claude", " abc#def ").await.unwrap();
        for _ in 0..50 {
            if got.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(std::fs::read_to_string(&got).unwrap().trim(), "abc#def");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let e = login_code("claude", "again").await.unwrap_err();
        assert!(
            format!("{e:?}").contains("no_sign_in"),
            "the sign-in has ended: {e:?}"
        );
    }
}
