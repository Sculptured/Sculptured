//! One structured call to the user's own Claude Code or Codex: the prompt on stdin, a JSON Schema
//! for the answer, no tools, read-only, no saved session, in a fresh empty folder. Used to
//! generate teams (`controld::generate`).
use crate::{Context, LocalError};
use serde_json::Value;

pub const MAX_PROMPT: usize = 32 * 1024;

/// A model name the CLI may be given: no spaces, never a flag.
#[must_use]
pub fn valid_model(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 80
        && !m.starts_with('-')
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c))
}

fn invalid(m: impl Into<String>) -> LocalError {
    LocalError::bad("generate_invalid", m)
}

/// At most 2 KB of what the CLI said, for the error the user sees.
fn tail(s: &str) -> String {
    let s = s.trim();
    let start = s
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| s.len() - i <= 2048)
        .unwrap_or(0);
    s[start..].to_string()
}

/// Whether the CLI said it needs a login. The user's own description is removed first (Codex
/// echoes it), and only phrases a CLI uses for that count, not any mention of "login".
fn signed_out(text: &str, prompt: &str) -> bool {
    let t = text.replace(prompt, "").to_lowercase();
    [
        "not logged in",
        "please run /login",
        "run /login",
        "codex login",
        "invalid api key",
        "401 unauthorized",
        "not signed in",
        "login required",
        "authentication failed",
        "oauth token has expired",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// How long one call may take, on any computer (the hosted link waits the same).
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Runs one structured call; the answer is the CLI's JSON object for `schema`.
pub async fn run(
    cx: &Context,
    cli: &str,
    model: Option<&str>,
    prompt: &str,
    schema: &Value,
) -> Result<Value, LocalError> {
    run_within(cx, cli, model, prompt, schema, TIMEOUT).await
}

/// `run`, stopped (and the CLI killed) after `limit`.
pub async fn run_within(
    cx: &Context,
    cli: &str,
    model: Option<&str>,
    prompt: &str,
    schema: &Value,
    limit: std::time::Duration,
) -> Result<Value, LocalError> {
    let (program, name) = match cli {
        "claude" => (&cx.claude, "Claude Code"),
        "codex" => (&cx.codex, "Codex"),
        other => {
            return Err(invalid(format!(
                "Unknown CLI {other:?}: use claude or codex"
            )));
        }
    };
    if prompt.len() > MAX_PROMPT {
        return Err(invalid(format!(
            "The description is too long (at most {} KB)",
            MAX_PROMPT / 1024
        )));
    }
    if let Some(m) = model.filter(|m| !valid_model(m)) {
        return Err(invalid(format!("{m:?} is not a model name")));
    }
    // A fresh, empty folder: the CLI sees only the prompt, never the user's files.
    let dir = tempfile::tempdir()
        .map_err(|e| LocalError::failed("generate_failed", format!("No temporary folder: {e}")))?;
    let answer = dir.path().join("answer.json");
    let mut args: Vec<String> = Vec::new();
    match cli {
        "claude" => {
            args.extend(["-p", "--output-format", "json", "--json-schema"].map(String::from));
            args.push(schema.to_string());
            // No built-in tools, no MCP servers, no user settings or hooks, no skills: only the prompt.
            args.extend(
                [
                    "--tools",
                    "",
                    "--no-session-persistence",
                    "--strict-mcp-config",
                    "--setting-sources",
                    "project,local",
                    "--disable-slash-commands",
                ]
                .map(String::from),
            );
            if let Some(m) = model {
                args.extend(["--model".into(), m.to_string()]);
            }
        }
        _ => {
            let schema_file = dir.path().join("schema.json");
            std::fs::write(&schema_file, schema.to_string())
                .map_err(|e| LocalError::failed("generate_failed", e.to_string()))?;
            args.extend(["exec", "--output-schema"].map(String::from));
            args.push(schema_file.display().to_string());
            // Read-only, no MCP servers. (Codex's read-only sandbox can still read files.)
            args.extend(
                [
                    "--sandbox",
                    "read-only",
                    "--ephemeral",
                    "--skip-git-repo-check",
                    "-c",
                    "mcp_servers={}",
                    "-o",
                ]
                .map(String::from),
            );
            args.push(answer.display().to_string());
            if let Some(m) = model {
                args.extend(["-m".into(), m.to_string()]);
            }
            args.push("-".into());
        }
    }
    let started = tokio::process::Command::new(crate::programs::resolve(program))
        .args(&args)
        .current_dir(dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn();
    let mut child = started.map_err(|e| {
        LocalError::conflict(
            "cli_missing",
            format!("{name} could not be started on this computer: {e}"),
        )
    })?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let talk = async move {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(prompt.as_bytes()).await;
        drop(stdin);
        child.wait_with_output().await
    };
    // Dropping the call on timeout drops the child, which kills it (kill_on_drop).
    let out = match tokio::time::timeout(limit, talk).await {
        Err(_) => {
            return Err(LocalError::new(
                504,
                "generate_timeout",
                format!(
                    "{name} did not answer in {} minutes",
                    limit.as_secs().div_ceil(60).max(1)
                ),
            ));
        }
        Ok(r) => r.map_err(|e| LocalError::failed("generate_failed", e.to_string()))?,
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let said = format!("{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        if signed_out(&said, prompt) {
            return Err(LocalError::conflict(
                "cli_not_signed_in",
                format!(
                    "{name} is not signed in on this computer. Sign in from Computers, or run it once in a terminal."
                ),
            ));
        }
        return Err(LocalError::new(
            502,
            "generate_failed",
            format!("{name} stopped without an answer: {}", tail(&said)),
        ));
    }
    let text = if cli == "claude" {
        stdout
    } else {
        std::fs::read_to_string(&answer).unwrap_or_default()
    };
    let unusable = || {
        LocalError::new(
            502,
            "generate_failed",
            format!("{name}'s answer could not be read: {}", tail(&text)),
        )
    };
    let parsed: Value = serde_json::from_str(text.trim()).map_err(|_| unusable())?;
    if cli == "claude" && parsed.get("is_error").and_then(Value::as_bool) == Some(true) {
        let result = parsed
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if signed_out(result, prompt) {
            return Err(LocalError::conflict(
                "cli_not_signed_in",
                format!(
                    "{name} is not signed in on this computer. Sign in from Computers, or run it once in a terminal."
                ),
            ));
        }
        return Err(LocalError::new(
            502,
            "generate_failed",
            format!("{name} answered with an error: {}", tail(result)),
        ));
    }
    let v = if cli == "claude" {
        match parsed.get("structured_output") {
            Some(v) if v.is_object() => v.clone(),
            _ => parsed
                .get("result")
                .and_then(Value::as_str)
                .and_then(|r| serde_json::from_str::<Value>(r.trim()).ok())
                .filter(Value::is_object)
                .ok_or_else(unusable)?,
        }
    } else {
        parsed
    };
    if !v.is_object() {
        return Err(unusable());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// A fake CLI: `body` is shell; it may use $LOG (a folder that outlives the call).
    fn fake(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\nLOG={}\n{body}\n",
                dir.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        for _ in 0..100 {
            match std::process::Command::new(&p).arg("--version").output() {
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(20))
                }
                _ => break,
            }
        }
        p
    }

    fn cx(dir: &Path, claude: PathBuf, codex: PathBuf) -> Context {
        Context {
            home: dir.into(),
            copies_root: dir.join("copies"),
            claude,
            codex,
            codex_home: dir.join(".codex"),
        }
    }

    #[tokio::test]
    async fn claude_runs_one_structured_call_with_tools_off_and_the_prompt_on_stdin() {
        let d = tempfile::tempdir().unwrap();
        let c = fake(
            d.path(),
            "claude",
            r#"printf '%s\n' "$@" > $LOG/args; cat > $LOG/stdin; pwd > $LOG/cwd; echo '{"type":"result","structured_output":{"ok":true}}'"#,
        );
        let v = run(
            &cx(d.path(), c, d.path().join("no")),
            "claude",
            Some("claude-opus-5-5"),
            "make a docs team",
            &serde_json::json!({"type":"object"}),
        )
        .await
        .unwrap();
        assert_eq!(v, serde_json::json!({"ok": true}));
        let args = std::fs::read_to_string(d.path().join("args")).unwrap();
        for a in [
            "-p",
            "--json-schema",
            "--tools",
            "--no-session-persistence",
            "--model",
            "claude-opus-5-5",
        ] {
            assert!(args.lines().any(|l| l == a), "{a} in {args}");
        }
        assert_eq!(
            std::fs::read_to_string(d.path().join("stdin")).unwrap(),
            "make a docs team"
        );
        let cwd = std::fs::read_to_string(d.path().join("cwd")).unwrap();
        assert!(
            !Path::new(cwd.trim()).exists(),
            "ran in a temporary folder, removed after: {cwd}"
        );
    }

    #[tokio::test]
    async fn codex_answers_through_its_last_message_file() {
        let d = tempfile::tempdir().unwrap();
        let x = fake(
            d.path(),
            "codex",
            r#"printf '%s\n' "$@" > $LOG/args; cat > /dev/null; out=""; prev=""; for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done; echo '{"ok":1}' > "$out""#,
        );
        let v = run(
            &cx(d.path(), d.path().join("no"), x),
            "codex",
            None,
            "p",
            &serde_json::json!({"type":"object"}),
        )
        .await
        .unwrap();
        assert_eq!(v, serde_json::json!({"ok": 1}));
        let args = std::fs::read_to_string(d.path().join("args")).unwrap();
        assert!(
            args.contains("--output-schema")
                && args.contains("read-only")
                && args.contains("--ephemeral"),
            "{args}"
        );
    }

    #[tokio::test]
    async fn models_and_prompts_cannot_become_flags() {
        assert!(
            !valid_model("--x")
                && !valid_model("a b")
                && !valid_model(&"m".repeat(81))
                && !valid_model("")
        );
        assert!(valid_model("claude-opus-5-5") && valid_model("gpt-6-sol"));
        let d = tempfile::tempdir().unwrap();
        let c = cx(d.path(), d.path().join("no"), d.path().join("no"));
        let s = serde_json::json!({});
        assert_eq!(
            run(&c, "claude", None, &"x".repeat(MAX_PROMPT + 1), &s)
                .await
                .unwrap_err()
                .code,
            "generate_invalid"
        );
        assert_eq!(
            run(
                &c,
                "claude",
                Some("--dangerously-skip-permissions"),
                "p",
                &s
            )
            .await
            .unwrap_err()
            .code,
            "generate_invalid"
        );
        assert_eq!(
            run(&c, "bash", None, "p", &s).await.unwrap_err().code,
            "generate_invalid"
        );
    }

    #[tokio::test]
    async fn an_unusable_answer_is_an_error_with_the_raw_text() {
        let d = tempfile::tempdir().unwrap();
        let c = fake(d.path(), "claude", "cat > /dev/null; echo 'not json'");
        let e = run(
            &cx(d.path(), c, d.path().join("no")),
            "claude",
            None,
            "p",
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "generate_failed");
        assert!(e.message.contains("not json"), "{}", e.message);
    }

    #[tokio::test]
    async fn signed_out_is_said_plainly() {
        let d = tempfile::tempdir().unwrap();
        let c = fake(
            d.path(),
            "claude",
            "cat > /dev/null; echo 'Invalid API key · Please run /login'; exit 1",
        );
        let e = run(
            &cx(d.path(), c, d.path().join("no")),
            "claude",
            None,
            "p",
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "cli_not_signed_in", "{}", e.message);
    }

    #[tokio::test]
    async fn claude_loads_no_mcp_servers_user_settings_or_skills_and_codex_no_mcp_servers() {
        let d = tempfile::tempdir().unwrap();
        let c = fake(
            d.path(),
            "claude",
            r#"printf '%s\n' "$@" > $LOG/cargs; cat > /dev/null; echo '{"structured_output":{}}'"#,
        );
        let x = fake(
            d.path(),
            "codex",
            r#"printf '%s\n' "$@" > $LOG/xargs; cat > /dev/null; out=""; prev=""; for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done; echo '{}' > "$out""#,
        );
        let cx = cx(d.path(), c, x);
        run(&cx, "claude", None, "p", &serde_json::json!({}))
            .await
            .unwrap();
        run(&cx, "codex", None, "p", &serde_json::json!({}))
            .await
            .unwrap();
        let ca = std::fs::read_to_string(d.path().join("cargs")).unwrap();
        for a in [
            "--strict-mcp-config",
            "--setting-sources",
            "--disable-slash-commands",
        ] {
            assert!(ca.lines().any(|l| l == a), "{a} in {ca}");
        }
        assert!(
            std::fs::read_to_string(d.path().join("xargs"))
                .unwrap()
                .lines()
                .any(|l| l == "mcp_servers={}")
        );
    }

    #[tokio::test]
    async fn a_description_about_logins_is_not_mistaken_for_being_signed_out() {
        let d = tempfile::tempdir().unwrap();
        // Codex echoes the prompt to stderr, then fails for another reason.
        let x = fake(
            d.path(),
            "codex",
            "cat >&2; echo 'stream error: rate limit reached' >&2; exit 1",
        );
        let e = run(
            &cx(d.path(), d.path().join("no"), x),
            "codex",
            None,
            "Build a login page and a catalog in Astro",
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "generate_failed", "{}", e.message);
    }

    #[tokio::test]
    async fn claude_reporting_an_error_result_about_its_key_is_signed_out() {
        let d = tempfile::tempdir().unwrap();
        let c = fake(
            d.path(),
            "claude",
            r#"cat > /dev/null; echo '{"type":"result","is_error":true,"result":"Invalid API key · Please run /login"}'"#,
        );
        let e = run(
            &cx(d.path(), c, d.path().join("no")),
            "claude",
            None,
            "p",
            &serde_json::json!({}),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "cli_not_signed_in", "{}", e.message);
    }

    #[tokio::test]
    async fn a_cli_that_never_answers_is_stopped() {
        let d = tempfile::tempdir().unwrap();
        let c = fake(d.path(), "claude", "cat > /dev/null; sleep 30");
        let began = std::time::Instant::now();
        let e = run_within(
            &cx(d.path(), c, d.path().join("no")),
            "claude",
            None,
            "p",
            &serde_json::json!({}),
            std::time::Duration::from_millis(500),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "generate_timeout");
        assert!(began.elapsed() < std::time::Duration::from_secs(5));
    }
}
