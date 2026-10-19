//! What Sculptured does on the computer that runs a team: folders, git, agents' own copies, checks,
//! CLI sign-in and plan usage. controld calls it in-process (local mode); a connected computer's
//! runner calls it for its owner's hosted Sculptured (after its own policy check).
pub mod checks;
pub mod command;
pub mod clis;
pub mod copies;
pub mod folders;
pub mod map;
mod clone;
pub mod programs;
pub mod generate;
pub mod git;
pub mod plan;
pub mod analyze;

use std::path::{Path, PathBuf};

/// A refusal or failure, with the HTTP status and code controld answers with. Serializable, so a
/// connected computer's answer crosses the wire unchanged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[error("{message}")]
pub struct LocalError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl LocalError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self { status, code: code.into(), message: message.into() }
    }
    pub fn bad(code: &str, m: impl Into<String>) -> Self { Self::new(400, code, m) }
    pub fn forbidden(code: &str, m: impl Into<String>) -> Self { Self::new(403, code, m) }
    pub fn not_found(code: &str, m: impl Into<String>) -> Self { Self::new(404, code, m) }
    pub fn conflict(code: &str, m: impl Into<String>) -> Self { Self::new(409, code, m) }
    pub fn failed(code: &str, m: impl Into<String>) -> Self { Self::new(500, code, m) }
}

/// One request for work on the computer, as it crosses the wire (tagged by `op`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum LocalOp {
    /// What this computer allows (answered by a connected computer's runner; `{}` here).
    Describe,
    /// `files`: also the folder's file names (an older runner ignores it and lists folders only).
    ListFolder {
        path: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        files: bool,
    },
    CreateFolder { parent: String, name: String },
    GitStatus { repo: String },
    GitDiff { repo: String, path: String, staged: bool },
    GitAct { repo: String, action: git::GitAction },
    CopyPrepare { team: String, seat: String, parent: Option<String>, cwd: String },
    CopiesRead { project: Option<String>, team: String, seats: Vec<copies::CopySeat>, enabled: bool },
    CopyMerge { project: String, team: String, seat: copies::CopySeat },
    CopyRemove { project: String, team: String, seat: copies::CopySeat },
    /// A work item's own copy of the project (`copies: per-item`); answers its folder.
    ItemCopyPrepare { project: String, team: String, task: String },
    /// Merges an item's branch into the project's current branch; answers `{merged, summary}`.
    ItemMerge { project: String, team: String, task: String, into: Option<String> },
    /// A workflow check stage: the command in the item's copy (when `item`), else in `cwd`.
    RunCheck { cwd: String, item: Option<ItemRef>, command: String, timeout_s: u32 },
    /// A workflow's dispatch command in `cwd`; answers its whole stdout (`command::CommandOutput`). An
    /// older runner refuses it as unknown, so its output is never silently cut to a check's tail.
    RunDispatch { cwd: String, command: String, timeout_s: u32 },
    Clis,
    CliLogin { name: String },
    /// The code from the sign-in page, for the sign-in under way (`clis::login_code`).
    CliLoginCode { name: String, code: String },
    PlanUsage,
    /// One structured, tool-less call to the user's Claude Code or Codex (`generate`).
    Generate { cli: String, model: Option<String>, prompt: String, schema: serde_json::Value },
    /// Where a team run's tokens went, from this computer's Claude Code and Codex session files and the
    /// run's links (a filtered `sculptured.session/v1`); answers a `mais.report/v1` report.
    AnalyzeRun { sessions: Vec<analyze::RunSession>, export: String },
    /// The project map of a folder (`map`), also written to its `.sculptured/map.md`.
    ProjectMap {
        path: String,
        /// Values the team's contracts say these files hold; each is checked. Absent from an older server.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        shared: Vec<model::SharedValue>,
    },
    /// Keeps a confirmed lesson in the folder's `.sculptured/lessons.md`.
    KeepLesson { path: String, text: String },
}

/// A work item of a team, named rather than given as a path: the computer finds its copy itself.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ItemRef {
    pub team: String,
    pub task: String,
}

/// Where the work happens on this computer.
#[derive(Clone, Debug)]
pub struct Context {
    /// The home folder (folder browsing is rooted here).
    pub home: PathBuf,
    /// Where agents' own copies live (`<team>/<agent>` under it).
    pub copies_root: PathBuf,
    pub claude: PathBuf,
    pub codex: PathBuf,
    /// `$CODEX_HOME`, else `~/.codex`.
    pub codex_home: PathBuf,
}

fn json<T: serde::Serialize>(v: T) -> serde_json::Value {
    serde_json::to_value(v).expect("serializes")
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, LocalError> + Send + 'static) -> Result<T, LocalError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| LocalError::failed("internal", format!("task failed: {e}")))?
}

/// Does one request and returns its answer as JSON (the same shapes the HTTP API returns).
pub async fn execute(cx: &Context, op: LocalOp) -> Result<serde_json::Value, LocalError> {
    let cx = cx.clone();
    match op {
        LocalOp::Describe => Ok(serde_json::json!({})),
        LocalOp::ListFolder { path, files } => blocking(move || Ok(json(folders::listing(&folders::Home::of(&cx.home)?, path.as_deref(), files)?))).await,
        LocalOp::CreateFolder { parent, name } => blocking(move || Ok(json(folders::create(&folders::Home::of(&cx.home)?, &parent, &name)?))).await,
        LocalOp::GitStatus { repo } => Ok(json(git::snapshot(Path::new(&repo)).await?)),
        LocalOp::GitDiff { repo, path, staged } => Ok(serde_json::json!({ "text": git::diff(Path::new(&repo), &path, staged).await? })),
        LocalOp::GitAct { repo, action } => Ok(json(git::act(Path::new(&repo), &action).await?)),
        LocalOp::CopyPrepare { team, seat, parent, cwd } => {
            blocking(move || Ok(json(copies::prepare(&cx.copies_root, &team, &seat, parent.as_deref(), Path::new(&cwd))?.display().to_string()))).await
        }
        LocalOp::CopiesRead { project, team, seats, enabled } => {
            blocking(move || Ok(json(copies::read(project.as_deref().map(Path::new), &cx.copies_root, &team, &seats, enabled)))).await
        }
        LocalOp::CopyMerge { project, team, seat } => blocking(move || copies::merge(Path::new(&project), &cx.copies_root, &team, &seat).map(|_| serde_json::json!({}))).await,
        LocalOp::CopyRemove { project, team, seat } => blocking(move || copies::remove(Path::new(&project), &cx.copies_root, &team, &seat).map(|_| serde_json::json!({}))).await,
        LocalOp::ItemCopyPrepare { project, team, task } => {
            blocking(move || Ok(json(copies::prepare(&cx.copies_root, &team, &copies::item_seat(&task), None, Path::new(&project))?.display().to_string()))).await
        }
        LocalOp::ItemMerge { project, team, task, into } => blocking(move || {
            Ok(match copies::merge_item(Path::new(&project), &cx.copies_root, &team, &task, into.as_deref())? {
                copies::MergeOutcome::Merged { summary } => serde_json::json!({ "merged": true, "summary": summary }),
                copies::MergeOutcome::Conflict(why) => serde_json::json!({ "merged": false, "summary": why }),
            })
        })
        .await,
        LocalOp::RunCheck { cwd, item, command, timeout_s } => {
            let cwd = match item {
                Some(i) => copies::item_dir(&cx.copies_root, &i.team, &i.task).display().to_string(),
                None => cwd,
            };
            if !Path::new(&cwd).is_dir() {
                return Err(LocalError::conflict("no_folder", format!("Could not run the check: the folder {cwd} does not exist on this computer.")));
            }
            Ok(json(checks::execute_within(&command, &cwd, std::time::Duration::from_secs(timeout_s.into())).await))
        }
        LocalOp::RunDispatch { cwd, command, timeout_s } => {
            if !Path::new(&cwd).is_dir() {
                return Err(LocalError::conflict("no_folder", format!("The folder {cwd} does not exist on this computer.")));
            }
            Ok(json(command::capture(&command, &cwd, std::time::Duration::from_secs(timeout_s.into())).await))
        }
        LocalOp::Clis => {
            let (a, b) = tokio::join!(clis::status("claude", &cx.claude), clis::status("codex", &cx.codex));
            Ok(json(vec![a, b]))
        }
        LocalOp::CliLogin { name } => {
            let program = match name.as_str() { "claude" => cx.claude.clone(), "codex" => cx.codex.clone(), _ => return Err(LocalError::not_found("not_found", format!("no CLI {name:?}"))) };
            Ok(json(clis::login(&name, &program).await?))
        }
        LocalOp::CliLoginCode { name, code } => clis::login_code(&name, &code).await.map(|()| serde_json::json!({})),
        LocalOp::PlanUsage => blocking(move || Ok(json(plan::codex_files(&cx.codex_home)))).await,
        LocalOp::Generate { cli, model, prompt, schema } => generate::run(&cx, &cli, model.as_deref(), &prompt, &schema).await,
        LocalOp::AnalyzeRun { sessions, export } => blocking(move || analyze::run(&cx, &sessions, &export)).await,
        LocalOp::ProjectMap { path, shared } => blocking(move || Ok(json(map::build_checked(Path::new(&path), &shared)?))).await,
        LocalOp::KeepLesson { path, text } => blocking(move || Ok(serde_json::json!({ "kept": map::keep_lesson(Path::new(&path), &text)? }))).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn one_executor_serves_every_op_with_plain_json() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("proj")).unwrap();
        let fake = home.path().join("claude");
        std::fs::write(&fake, "#!/bin/sh\ncase \"$1\" in --version) echo 9.9;; auth) echo '{\"loggedIn\":true,\"email\":\"a@x\"}';; esac\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A test thread forking while the script was open for writing makes exec fail with "text file busy" for a moment.
        for _ in 0..100 {
            match std::process::Command::new(&fake).arg("--version").output() {
                Err(e) if e.raw_os_error() == Some(26) => std::thread::sleep(std::time::Duration::from_millis(20)),
                _ => break,
            }
        }
        let cx = Context { home: home.path().into(), copies_root: home.path().join("copies"), claude: fake.clone(), codex: home.path().join("no-codex"), codex_home: home.path().join(".codex") };
        let l = execute(&cx, LocalOp::ListFolder { path: None, files: false }).await.unwrap();
        assert_eq!(l["entries"][0]["name"], "proj");
        let s = execute(&cx, LocalOp::GitStatus { repo: home.path().join("proj").display().to_string() }).await.unwrap();
        assert_eq!(s["repository"], false);
        let clis = execute(&cx, LocalOp::Clis).await.unwrap();
        assert_eq!((clis[0]["name"].as_str(), clis[0]["installed"].as_bool(), clis[0]["signed_in"].as_bool(), clis[0]["account"].as_str()), (Some("claude"), Some(true), Some(true), Some("a@x")));
        assert_eq!(clis[1]["installed"], false);
        assert!(execute(&cx, LocalOp::PlanUsage).await.unwrap().is_null(), "no codex session files: null");
        let wire: LocalOp = serde_json::from_str(r#"{"op":"git-status","repo":"/x"}"#).unwrap();
        assert_eq!(wire, LocalOp::GitStatus { repo: "/x".into() });
        let e = execute(&cx, LocalOp::ListFolder { path: Some("/".into()), files: false }).await.unwrap_err();
        assert_eq!(e.status, 403);
    }
}
