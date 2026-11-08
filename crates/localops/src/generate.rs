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
    let schema_file = dir.path().join("schema.json");
    if cli == "codex" {
        std::fs::write(&schema_file, schema.to_string())
            .map_err(|e| LocalError::failed("generate_failed", e.to_string()))?;
    }
    let args = cli_args(cli, model, schema, &schema_file, &answer);
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
    let said = Said {
        success: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        answer_file: std::fs::read_to_string(&answer).unwrap_or_default(),
    };
    read_answer(cli, prompt, &said)
}

/// The CLI's arguments for one structured call. Codex reads the schema from `schema_file` and writes
/// its answer to `answer`.
fn cli_args(
    cli: &str,
    model: Option<&str>,
    schema: &Value,
    schema_file: &std::path::Path,
    answer: &std::path::Path,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if cli == "claude" {
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
        return args;
    }
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
    args
}

/// What a finished CLI left behind: whether it exited cleanly, its output, and Codex's answer file.
struct Said {
    success: bool,
    stdout: String,
    stderr: String,
    answer_file: String,
}

fn not_signed_in(name: &str) -> LocalError {
    LocalError::conflict(
        "cli_not_signed_in",
        format!(
            "{name} is not signed in on this computer. Sign in from Computers, or run it once in a terminal."
        ),
    )
}

/// The answer in what the CLI said, or the error the user sees.
fn read_answer(cli: &str, prompt: &str, said: &Said) -> Result<Value, LocalError> {
    let name = if cli == "claude" {
        "Claude Code"
    } else {
        "Codex"
    };
    if !said.success {
        let all = format!("{}\n{}", said.stdout, said.stderr);
        if signed_out(&all, prompt) {
            return Err(not_signed_in(name));
        }
        return Err(LocalError::new(
            502,
            "generate_failed",
            format!("{name} stopped without an answer: {}", tail(&all)),
        ));
    }
    let text = if cli == "claude" {
        &said.stdout
    } else {
        &said.answer_file
    };
    let unusable = || {
        LocalError::new(
            502,
            "generate_failed",
            format!("{name}'s answer could not be read: {}", tail(text)),
        )
    };
    let parsed: Value = serde_json::from_str(text.trim()).map_err(|_| unusable())?;
    if cli == "claude" && parsed.get("is_error").and_then(Value::as_bool) == Some(true) {
        let result = parsed
            .get("result")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if signed_out(result, prompt) {
            return Err(not_signed_in(name));
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

    fn cx(dir: &Path, claude: PathBuf, codex: PathBuf) -> Context {
        Context {
            home: dir.into(),
            copies_root: dir.join("copies"),
            claude,
            codex,
            codex_home: dir.join(".codex"),
        }
    }

    fn ok(stdout: &str) -> Said {
        Said {
            success: true,
            stdout: stdout.into(),
            stderr: String::new(),
            answer_file: String::new(),
        }
    }

    fn failed(stdout: &str, stderr: &str) -> Said {
        Said {
            success: false,
            stdout: stdout.into(),
            stderr: stderr.into(),
            answer_file: String::new(),
        }
    }

    #[test]
    fn claude_runs_one_structured_call_with_no_tools_settings_mcp_or_skills() {
        let schema = serde_json::json!({"type":"object"});
        let args = cli_args(
            "claude",
            Some("claude-opus-5-5"),
            &schema,
            Path::new("/s"),
            Path::new("/a"),
        );
        for a in [
            "-p",
            "--json-schema",
            "--tools",
            "--no-session-persistence",
            "--strict-mcp-config",
            "--setting-sources",
            "--disable-slash-commands",
            "--model",
            "claude-opus-5-5",
        ] {
            assert!(args.iter().any(|l| l == a), "{a} in {args:?}");
        }
        assert!(args.contains(&schema.to_string()));
    }

    #[test]
    fn codex_runs_read_only_without_mcp_servers_and_answers_in_a_file() {
        let args = cli_args(
            "codex",
            Some("gpt-6-sol"),
            &serde_json::json!({}),
            Path::new("/tmp/x/schema.json"),
            Path::new("/tmp/x/answer.json"),
        );
        let has = |a: &str| args.iter().any(|l| l == a);
        assert!(
            has("--output-schema")
                && has("/tmp/x/schema.json")
                && has("read-only")
                && has("--ephemeral")
                && has("mcp_servers={}")
                && has("/tmp/x/answer.json")
                && has("gpt-6-sol"),
            "{args:?}"
        );
        assert_eq!(
            args.last().map(String::as_str),
            Some("-"),
            "the prompt on stdin"
        );
    }

    #[test]
    fn claudes_structured_output_or_json_result_is_the_answer() {
        let v = read_answer(
            "claude",
            "p",
            &ok(r#"{"type":"result","structured_output":{"ok":true}}"#),
        )
        .unwrap();
        assert_eq!(v, serde_json::json!({"ok": true}));
        let v = read_answer("claude", "p", &ok(r#"{"result":"{\"ok\":2}"}"#)).unwrap();
        assert_eq!(v, serde_json::json!({"ok": 2}));
    }

    #[test]
    fn codex_answers_through_its_last_message_file() {
        let said = Said {
            answer_file: r#"{"ok":1}"#.into(),
            ..ok("anything on stdout")
        };
        assert_eq!(
            read_answer("codex", "p", &said).unwrap(),
            serde_json::json!({"ok": 1})
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
    async fn a_missing_cli_is_said_plainly() {
        let d = tempfile::tempdir().unwrap();
        let c = cx(d.path(), d.path().join("no-claude"), d.path().join("no"));
        let e = run(&c, "claude", None, "p", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(e.code, "cli_missing", "{}", e.message);
    }

    #[test]
    fn an_unusable_answer_is_an_error_with_the_raw_text() {
        let e = read_answer("claude", "p", &ok("not json")).unwrap_err();
        assert_eq!(e.code, "generate_failed");
        assert!(e.message.contains("not json"), "{}", e.message);
    }

    #[test]
    fn signed_out_is_said_plainly() {
        let said = failed("Invalid API key · Please run /login", "");
        let e = read_answer("claude", "p", &said).unwrap_err();
        assert_eq!(e.code, "cli_not_signed_in", "{}", e.message);
    }

    #[test]
    fn a_description_about_logins_is_not_mistaken_for_being_signed_out() {
        // Codex echoes the prompt to stderr, then fails for another reason.
        let prompt = "Build a login page and a catalog in Astro";
        let said = failed("", &format!("{prompt}\nstream error: rate limit reached"));
        let e = read_answer("codex", prompt, &said).unwrap_err();
        assert_eq!(e.code, "generate_failed", "{}", e.message);
    }

    #[test]
    fn claude_reporting_an_error_result_about_its_key_is_signed_out() {
        let said = ok(
            r#"{"type":"result","is_error":true,"result":"Invalid API key · Please run /login"}"#,
        );
        let e = read_answer("claude", "p", &said).unwrap_err();
        assert_eq!(e.code, "cli_not_signed_in", "{}", e.message);
    }

    fn on_path(name: &str) -> Option<PathBuf> {
        let found = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join(name))
                .find(|p| p.is_file())
        });
        if found.is_none() {
            eprintln!(
                "skipped: the {name} CLI is not on PATH; live tests need it installed and signed in"
            );
        }
        found
    }

    fn answer_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": { "word": { "type": "string" } },
            "required": ["word"],
            "additionalProperties": false
        })
    }

    /// Live: the real Claude Code answers one structured call with an object for the schema.
    #[tokio::test]
    #[ignore = "live: real claude CLI on haiku; run with --ignored"]
    async fn live_claude_answers_one_structured_call() {
        let Some(claude) = on_path("claude") else {
            return;
        };
        let d = tempfile::tempdir().unwrap();
        let v = run(
            &cx(d.path(), claude, d.path().join("no")),
            "claude",
            Some("haiku"),
            "Answer with any single English word.",
            &answer_schema(),
        )
        .await
        .unwrap();
        assert!(v["word"].is_string(), "{v}");
    }

    /// Live: the real Codex answers one structured call through its last-message file.
    #[tokio::test]
    #[ignore = "live: real codex CLI on gpt-6-luna; run with --ignored"]
    async fn live_codex_answers_one_structured_call() {
        let Some(codex) = on_path("codex") else {
            return;
        };
        let d = tempfile::tempdir().unwrap();
        let v = run(
            &cx(d.path(), d.path().join("no"), codex),
            "codex",
            Some("gpt-6-luna"),
            "Answer with any single English word.",
            &answer_schema(),
        )
        .await
        .unwrap();
        assert!(v["word"].is_string(), "{v}");
    }

    /// Live: a real CLI that has not answered by the limit is stopped, and that is said.
    #[tokio::test]
    #[ignore = "live: real claude CLI on haiku; run with --ignored"]
    async fn live_a_cli_that_has_not_answered_in_time_is_stopped() {
        let Some(claude) = on_path("claude") else {
            return;
        };
        let d = tempfile::tempdir().unwrap();
        let began = std::time::Instant::now();
        let e = run_within(
            &cx(d.path(), claude, d.path().join("no")),
            "claude",
            Some("haiku"),
            "Write a 2000-word essay about rivers.",
            &serde_json::json!({}),
            std::time::Duration::from_millis(300),
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, "generate_timeout");
        assert!(began.elapsed() < std::time::Duration::from_secs(10));
    }
}
