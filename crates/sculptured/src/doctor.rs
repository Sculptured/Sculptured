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
    for d in &dirs {
        checks.extend(runner_checks(d));
    }
    println!("{}", report(&checks));
    Ok(())
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
}
