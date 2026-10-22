//! Git for a project folder: status, diff and a fixed set of actions. Read requests never run hooks or
//! external diff tools. Paths must be repository-relative, without `..` or `.git`; actions are a
//! fixed list, never arbitrary git arguments.
use crate::LocalError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path as FsPath;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GitFile {
    pub path: String,
    pub index: String,
    pub worktree: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GitSnapshot {
    pub repository: bool,
    pub branch: String,
    pub head: String,
    pub files: Vec<GitFile>,
    pub log: String,
    pub fingerprint: String,
}

fn fail(s: impl Into<String>) -> LocalError {
    LocalError::bad("git_failed", s)
}

/// One action on a repository, as the Git page sends it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitAction {
    pub action: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub message: String,
    /// The status fingerprint the user looked at; a changed repository is refused.
    #[serde(default)]
    pub fingerprint: String,
}

pub async fn git(path: &FsPath, args: &[&str]) -> Result<String, LocalError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.current_dir(path)
        .args([
            "--no-pager",
            "--literal-pathspecs",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd
        .spawn()
        .map_err(|_| fail("Install Git to use repository controls"))?;
    let out = child
        .stdout
        .take()
        .expect("Git stdout is piped by this function");
    let err = child
        .stderr
        .take()
        .expect("Git stderr is piped by this function");
    let run = async {
        let read = |mut r: tokio::io::Take<_>| async move {
            let mut bytes = Vec::new();
            r.read_to_end(&mut bytes).await.map(|_| bytes)
        };
        let (o, e, status) = tokio::join!(
            read(out.take(1_000_001)),
            async {
                let mut b = Vec::new();
                err.take(16001).read_to_end(&mut b).await.map(|_| b)
            },
            child.wait()
        );
        let o = o.map_err(|_| fail("Cannot read Git output"))?;
        let e = e.map_err(|_| fail("Cannot read Git error"))?;
        let status = status.map_err(|_| fail("Git did not exit cleanly"))?;
        if !status.success() {
            return Err(fail(
                String::from_utf8_lossy(&e)
                    .chars()
                    .take(1000)
                    .collect::<String>(),
            ));
        }
        if o.len() > 1_000_000 {
            return Err(fail(
                "Git output exceeds 1 MB; use the terminal for this repository",
            ));
        }
        Ok(String::from_utf8_lossy(&o).into_owned())
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), run)
        .await
        .map_err(|_| fail("Git timed out"))?
}
async fn root(path: &FsPath) -> Result<(), LocalError> {
    let top = git(path, &["rev-parse", "--show-toplevel"]).await?;
    if FsPath::new(top.trim()) != path {
        return Err(fail("Project folder must be the repository root"));
    }
    Ok(())
}
fn parse_files(raw: &str) -> Vec<GitFile> {
    let mut it = raw.split('\0').filter(|s| !s.is_empty());
    let mut files = vec![];
    while let Some(row) = it.next() {
        if row.len() < 4 || !row.is_char_boundary(3) {
            continue;
        }
        let index = &row[..1];
        let worktree = &row[1..2];
        files.push(GitFile {
            index: index.into(),
            worktree: worktree.into(),
            path: row[3..].into(),
        });
        if index == "R" || index == "C" || worktree == "R" || worktree == "C" {
            it.next();
        }
    }
    files
}
pub async fn snapshot(path: &FsPath) -> Result<GitSnapshot, LocalError> {
    if !path.join(".git").exists() {
        return Ok(GitSnapshot {
            repository: false,
            branch: String::new(),
            head: String::new(),
            files: vec![],
            log: String::new(),
            fingerprint: String::new(),
        });
    }
    root(path).await?;
    let raw = git(
        path,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
    )
    .await?;
    let head = git(path, &["rev-parse", "--verify", "HEAD"])
        .await
        .unwrap_or_default();
    let branch = git(path, &["symbolic-ref", "--short", "-q", "HEAD"])
        .await
        .unwrap_or_else(|_| "Detached HEAD".into());
    let log = git(path, &["log", "-8", "--format=%h %s"])
        .await
        .unwrap_or_default();
    let index = git(path, &["ls-files", "--stage", "-z"]).await?;
    let fingerprint = hex::encode(Sha256::digest(format!("{head}\0{raw}\0{index}")));
    Ok(GitSnapshot {
        repository: true,
        head: head.trim().into(),
        branch: branch.trim().into(),
        files: parse_files(&raw),
        log,
        fingerprint,
    })
}

fn safe_path(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && !s.contains('\0')
        && FsPath::new(s)
            .components()
            .all(|c| matches!(c,std::path::Component::Normal(v) if v!=".git"))
}

/// The changes to `path` (staged or not).
pub async fn diff(dir: &FsPath, path: &str, staged: bool) -> Result<String, LocalError> {
    if !safe_path(path) {
        return Err(fail("Invalid repository path"));
    }
    root(dir).await?;
    let mut args = vec!["diff", "--no-ext-diff", "--no-textconv"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", path]);
    git(dir, &args).await
}

/// Runs one action (refused when the repository changed since `fingerprint`); returns the new status.
pub async fn act(dir: &FsPath, req: &GitAction) -> Result<GitSnapshot, LocalError> {
    let before = snapshot(dir).await?;
    if req.fingerprint != before.fingerprint {
        return Err(LocalError::conflict(
            "git_changed",
            "Repository changed; review the new status",
        ));
    }
    if req.action == "init" {
        if before.repository {
            return Err(fail("Repository already exists"));
        }
        git(dir, &["init"]).await?;
    } else {
        root(dir).await?;
        match req.action.as_str() {
            "stage" | "unstage" => {
                if req.paths.is_empty()
                    || req.paths.len() > 100
                    || req
                        .paths
                        .iter()
                        .any(|p| !safe_path(p) || !before.files.iter().any(|f| f.path == *p))
                {
                    return Err(fail("Choose up to 100 changed files"));
                }
                let mut args = if req.action == "stage" {
                    vec!["add", "--"]
                } else if before.head.is_empty() {
                    vec!["rm", "--cached", "--"]
                } else {
                    vec!["reset", "-q", "HEAD", "--"]
                };
                args.extend(req.paths.iter().map(String::as_str));
                git(dir, &args).await?;
            }
            "commit" => {
                if req.message.trim().is_empty() || req.message.len() > 2000 {
                    return Err(fail("Enter a commit message"));
                }
                git(
                    dir,
                    &["-c", "commit.gpgsign=false", "commit", "-m", &req.message],
                )
                .await?;
            }
            "branch" => {
                if req.message.len() > 100 || req.message.starts_with('-') {
                    return Err(fail("Invalid branch name"));
                }
                git(dir, &["check-ref-format", "--branch", &req.message]).await?;
                git(dir, &["switch", "-c", &req.message]).await?;
            }
            _ => return Err(fail("Unknown Git action")),
        }
    }
    snapshot(dir).await
}

#[cfg(test)]
mod unit {
    use super::*;
    #[test]
    fn paths_and_renames() {
        assert!(!safe_path("../outside"));
        assert!(!safe_path(".git/config"));
        assert!(!safe_path("/etc/passwd"));
        assert!(safe_path("src/my file.ts"));
        let f = parse_files("R  new.txt\0old.txt\0 M src/test.ts\0");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].path, "new.txt");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sh(dir: &std::path::Path, args: &[&str]) {
        assert!(
            std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    #[tokio::test]
    async fn snapshot_stage_commit_and_refuse_option_like_or_outside_paths() {
        let d = tempfile::tempdir().unwrap();
        let s = snapshot(d.path()).await.unwrap();
        assert!(!s.repository);
        act(
            d.path(),
            &GitAction {
                action: "init".into(),
                paths: vec![],
                message: String::new(),
                fingerprint: s.fingerprint,
            },
        )
        .await
        .unwrap();
        sh(d.path(), &["config", "user.email", "t@t"]);
        sh(d.path(), &["config", "user.name", "T"]);
        std::fs::write(d.path().join("a.txt"), "a").unwrap();
        let s = snapshot(d.path()).await.unwrap();
        assert_eq!(s.files.len(), 1);
        for bad in ["--upload-pack=x", "/etc/passwd", "../x", "-c"] {
            let e = act(
                d.path(),
                &GitAction {
                    action: "stage".into(),
                    paths: vec![bad.into()],
                    message: String::new(),
                    fingerprint: s.fingerprint.clone(),
                },
            )
            .await
            .unwrap_err();
            assert_eq!(
                (e.status, e.code.as_str()),
                (400, "git_failed"),
                "{bad}: {e:?}"
            );
        }
        let e = act(
            d.path(),
            &GitAction {
                action: "rm -rf".into(),
                paths: vec![],
                message: String::new(),
                fingerprint: s.fingerprint.clone(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(e.message, "Unknown Git action");
        let e = act(
            d.path(),
            &GitAction {
                action: "stage".into(),
                paths: vec!["a.txt".into()],
                message: String::new(),
                fingerprint: "stale".into(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!((e.status, e.code.as_str()), (409, "git_changed"));
        let s = act(
            d.path(),
            &GitAction {
                action: "stage".into(),
                paths: vec!["a.txt".into()],
                message: String::new(),
                fingerprint: s.fingerprint,
            },
        )
        .await
        .unwrap();
        let s = act(
            d.path(),
            &GitAction {
                action: "commit".into(),
                paths: vec![],
                message: "first".into(),
                fingerprint: s.fingerprint,
            },
        )
        .await
        .unwrap();
        assert!(s.log.contains("first"));
        assert!(diff(d.path(), "a.txt", false).await.unwrap().is_empty());
        assert!(diff(d.path(), "../x", false).await.is_err());
    }
}
