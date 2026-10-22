//! Agents' own copies: each agent works in a git worktree on its own branch (`sculptured/<team>/<agent>`)
//! under a copies folder, so agents running at the same time never write the same files. A spawned
//! agent branches from its parent's branch. Merging commits the copy's uncommitted work on its branch
//! first; removing keeps the branch. On a filesystem without file modes or symlinks (exFAT) the copy is
//! told to ignore them before checkout.
use crate::LocalError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentCopy {
    pub seat: String,
    pub branch: String,
    pub path: String,
    /// Commits on the agent's branch that the project's current branch does not have.
    pub ahead: u32,
    /// Files changed in the copy and not yet committed.
    pub uncommitted: u32,
    pub running: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AgentCopies {
    pub enabled: bool,
    /// The project's current branch, which merges go into.
    pub into: String,
    pub copies: Vec<AgentCopy>,
}

/// A team agent and whether it is running now (from the caller's own records).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopySeat {
    pub seat: String,
    pub running: bool,
}

/// `team.fuselage@c172` → `fuselage`; anything git refuses in a branch name becomes `-`.
fn short(team: &str, seat: &str) -> String {
    let s = seat.strip_suffix(&format!("@{team}")).unwrap_or(seat);
    let s = s.strip_prefix("team.").unwrap_or(s);
    let clean: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    clean.trim_matches('-').to_string()
}

/// Whether the team and agent (or item) names leave something usable once cleaned for git and folders;
/// an empty one would point at every team's copies.
#[must_use]
pub fn valid_name(team: &str, seat: &str) -> bool {
    !short(team, team).is_empty() && !short(team, seat).is_empty()
}

#[must_use]
pub fn branch_of(team: &str, seat: &str) -> String {
    format!("sculptured/{}/{}", short(team, team), short(team, seat))
}

/// Where `seat`'s copy lives under `copies_root`.
#[must_use]
pub fn copy_dir(copies_root: &Path, team: &str, seat: &str) -> PathBuf {
    copies_root.join(short(team, team)).join(short(team, seat))
}

/// Windows: files have no Unix modes to keep.
#[cfg(windows)]
fn keeps_modes(_dir: &Path) -> bool {
    false
}

/// True when files made in `dir` keep the mode they are given (not exFAT/FAT/NTFS mounts).
#[cfg(unix)]
fn keeps_modes(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let probe = dir.join(".sculptured-mode-probe");
    let ok = std::fs::write(&probe, b"").is_ok()
        && std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o644)).is_ok()
        && std::fs::metadata(&probe).is_ok_and(|m| m.permissions().mode() & 0o111 == 0);
    let _ = std::fs::remove_file(&probe);
    ok
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "--no-pager",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "Install Git to give agents their own copies".to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(600)
            .collect::<String>()
            .trim()
            .to_string())
    }
}

/// Held while git writes to one project's repository (making or removing a worktree, merging): git refuses
/// concurrent writers ("could not lock config file"), and agents and items start in parallel. Different
/// repositories never wait for each other.
struct RepoWrite(PathBuf);

static WRITING: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
static FREED: std::sync::Condvar = std::sync::Condvar::new();

fn repo_writes(root: &Path) -> RepoWrite {
    let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut busy = WRITING
        .lock()
        .expect("repository write registry mutex is not poisoned");
    while busy.contains(&key) {
        busy = FREED
            .wait(busy)
            .expect("repository write registry mutex is not poisoned");
    }
    busy.push(key.clone());
    RepoWrite(key)
}

impl Drop for RepoWrite {
    fn drop(&mut self) {
        // Leave poisoned state alone; Drop can run while an earlier panic is unwinding.
        if let Ok(mut busy) = WRITING.lock() {
            busy.retain(|key| *key != self.0);
        }
        FREED.notify_all();
    }
}

fn unavailable(m: impl Into<String>) -> LocalError {
    LocalError::conflict("copy_unavailable", m)
}

/// The folder `seat` should run in: its own copy of the repository that holds `cwd` (made on first
/// launch, reused after), at the same place inside it. Refused, in plain words, when there is no
/// repository to copy: the launch is refused rather than run in the shared folder.
pub fn prepare(
    copies_root: &Path,
    team: &str,
    seat: &str,
    parent: Option<&str>,
    cwd: &Path,
) -> Result<PathBuf, LocalError> {
    prepare_with(copies_root, team, seat, parent, cwd, crate::clone::reflink)
}

/// The copy made from a clone of the project (sharing its disk), then brought to exactly its branch:
/// git rewrites only the files that differ, and the project's own uncommitted work is removed, while
/// ignored build output (`node_modules`, `target`) stays, already built and costing nothing.
fn from_clone(
    root: &Path,
    dir: &Path,
    copies_root: &Path,
    clone: crate::clone::Cloner,
) -> Result<(), String> {
    if dir == root || !dir.starts_with(copies_root) {
        return Err("the copy is not in the copies folder".into());
    }
    crate::clone::clone_tree(root, dir, copies_root, clone).map_err(|e| e.to_string())?;
    git(dir, &["read-tree", "HEAD"])?;
    let _ = git(dir, &["update-index", "-q", "--refresh"]);
    git(dir, &["reset", "-q", "--hard"])?;
    git(dir, &["clean", "-fdq"])?;
    Ok(())
}

pub(crate) fn prepare_with(
    copies_root: &Path,
    team: &str,
    seat: &str,
    parent: Option<&str>,
    cwd: &Path,
    clone: crate::clone::Cloner,
) -> Result<PathBuf, LocalError> {
    if !valid_name(team, seat) {
        return Err(unavailable(format!(
            "{team} / {seat} has no letters or digits to name a copy with."
        )));
    }
    let root = git(cwd, &["rev-parse", "--show-toplevel"]).map_err(|_| {
        unavailable(format!("{} is not in a git repository, so agents can't get their own copies. Turn on Git for the project folder (Git page), or switch off own copies.", cwd.display()))
    })?;
    let root = PathBuf::from(root);
    let inner = cwd
        .canonicalize()
        .ok()
        .and_then(|c| {
            c.strip_prefix(root.canonicalize().ok()?)
                .ok()
                .map(Path::to_path_buf)
        })
        .unwrap_or_default();
    let dir = copy_dir(copies_root, team, seat);
    // Reusing a copy writes nothing, so it never waits; making one takes the lock and looks again.
    let _writing = (!dir.join(".git").exists()).then(|| repo_writes(&root));
    if !dir.join(".git").exists() {
        if git(&root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
            return Err(unavailable(
                "The project's git repository has no commits yet. Make a first commit (Git page) so agents have something to copy.",
            ));
        }
        let _ = git(&root, &["worktree", "prune"]);
        let copy_parent = dir
            .parent()
            .expect("a copy is nested under the team's copies directory");
        std::fs::create_dir_all(copy_parent)
            .map_err(|e| unavailable(format!("Can't make {}: {e}", dir.display())))?;
        let branch = branch_of(team, seat);
        let d = dir.display().to_string();
        // Without file modes or symlinks (exFAT), check out only after the copy is told to ignore them.
        let plain = !keeps_modes(copy_parent);
        // On a filesystem that clones files, the copy shares the project's disk instead of a checkout.
        let cow = !plain && crate::clone::can_clone(&root, copy_parent, clone);
        let mut add = vec!["worktree", "add"];
        if plain || cow {
            add.push("--no-checkout");
        }
        let made = if git(
            &root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ],
        )
        .is_ok()
        {
            git(&root, &[&add[..], &[d.as_str(), branch.as_str()]].concat())
        } else {
            let start = parent
                .map(|p| branch_of(team, p))
                .filter(|p| {
                    git(
                        &root,
                        &[
                            "rev-parse",
                            "--verify",
                            "--quiet",
                            &format!("refs/heads/{p}"),
                        ],
                    )
                    .is_ok()
                })
                .unwrap_or_else(|| "HEAD".into());
            git(
                &root,
                &[
                    &add[..],
                    &["-b", branch.as_str(), d.as_str(), start.as_str()],
                ]
                .concat(),
            )
        };
        made.map_err(|e| unavailable(format!("Couldn't make this agent's own copy: {e}")))?;
        if plain {
            // Per-copy settings, so the project's own checkout is unaffected.
            let ready = git(&dir, &["config", "extensions.worktreeConfig", "true"])
                .and_then(|_| git(&dir, &["config", "--worktree", "core.fileMode", "false"]))
                .and_then(|_| git(&dir, &["config", "--worktree", "core.symlinks", "false"]))
                .and_then(|_| git(&dir, &["reset", "-q", "--hard", "HEAD"]));
            ready.map_err(|e| {
                unavailable(format!(
                    "Couldn't set up this agent's copy for its drive: {e}"
                ))
            })?;
        } else if cow && from_clone(&root, &dir, copies_root, clone).is_err() {
            // A clone that went wrong is finished as an ordinary checkout.
            git(&dir, &["reset", "-q", "--hard", "HEAD"])
                .map_err(|e| unavailable(format!("Couldn't check out this agent's copy: {e}")))?;
        }
    }
    // The copy itself when `cwd` is the repository root (no trailing "/").
    let at = if inner.as_os_str().is_empty() {
        dir
    } else {
        dir.join(inner)
    };
    std::fs::create_dir_all(&at)
        .map_err(|e| unavailable(format!("Can't make {}: {e}", at.display())))?;
    Ok(at)
}

/// The repository root of the project folder.
pub fn project_root(folder: &Path) -> Result<PathBuf, LocalError> {
    git(folder, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .map_err(|e| LocalError::bad("git_failed", e))
}

/// The copies that exist for `seats`, and what each has that the project does not. Empty when the
/// project folder is missing or not a repository.
#[must_use]
pub fn read(
    project: Option<&Path>,
    copies_root: &Path,
    team: &str,
    seats: &[CopySeat],
    enabled: bool,
) -> AgentCopies {
    let Some(root) = project.and_then(|p| project_root(p).ok()) else {
        return AgentCopies {
            enabled,
            into: String::new(),
            copies: vec![],
        };
    };
    let into = git(&root, &["branch", "--show-current"]).unwrap_or_default();
    let mut copies = vec![];
    for s in seats {
        let dir = copy_dir(copies_root, team, &s.seat);
        if !dir.join(".git").exists() {
            continue;
        }
        let branch = branch_of(team, &s.seat);
        let ahead = git(&root, &["rev-list", "--count", &format!("HEAD..{branch}")])
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let uncommitted = git(&dir, &["status", "--porcelain"])
            .map(|s| s.lines().count() as u32)
            .unwrap_or(0);
        copies.push(AgentCopy {
            seat: s.seat.clone(),
            branch,
            path: dir.display().to_string(),
            ahead,
            uncommitted,
            running: s.running,
        });
    }
    AgentCopies {
        enabled,
        into,
        copies,
    }
}

fn stopped_copy(copies_root: &Path, team: &str, seat: &CopySeat) -> Result<PathBuf, LocalError> {
    if seat.running {
        return Err(LocalError::conflict(
            "agent_running",
            "Stop this agent first, so its copy is not changing while you merge or remove it.",
        ));
    }
    let dir = copy_dir(copies_root, team, &seat.seat);
    if !dir.join(".git").exists() {
        return Err(LocalError::not_found(
            "no_copy",
            "This agent has no own copy",
        ));
    }
    Ok(dir)
}

/// Commits the copy's uncommitted work on the agent's branch, then merges the branch into the project's
/// current branch. A conflicted merge is undone and reported; nothing is left half-merged.
pub fn merge(
    project: &Path,
    copies_root: &Path,
    team: &str,
    seat: &CopySeat,
) -> Result<(), LocalError> {
    let dir = stopped_copy(copies_root, team, seat)?;
    let root = project_root(project)?;
    let failed = |e: String| LocalError::conflict("merge_failed", e);
    commit_work(&dir, &short(team, &seat.seat)).map_err(failed)?;
    let branch = branch_of(team, &seat.seat);
    match merge_branch(&root, &branch)? {
        MergeOutcome::Merged { .. } => Ok(()),
        MergeOutcome::Conflict(e) => Err(failed(format!(
            "Merging {branch} did not apply cleanly, so nothing was merged: {e}"
        ))),
    }
}

/// Commits whatever the copy has changed, so a merge takes all of it (Sculptured signs when git has no user).
fn commit_work(dir: &Path, who_worked: &str) -> Result<(), String> {
    if git(dir, &["status", "--porcelain"])?.is_empty() {
        return Ok(());
    }
    git(dir, &["add", "-A"])?;
    let who: &[&str] = if git(dir, &["config", "user.email"]).is_ok() {
        &[]
    } else {
        &[
            "-c",
            "user.name=Sculptured",
            "-c",
            "user.email=sculptured@localhost",
        ]
    };
    let msg = format!("Work by {who_worked} (committed by Sculptured before merging)");
    git(dir, &[who, &["commit", "-q", "-m", &msg]].concat()).map(|_| ())
}

/// The name a work item's copy is kept under, as if it were an agent: branch `sculptured/<team>/item-<task>`.
#[must_use]
pub fn item_seat(task: &str) -> String {
    format!("item-{task}")
}

#[must_use]
pub fn item_dir(copies_root: &Path, team: &str, task: &str) -> PathBuf {
    copy_dir(copies_root, team, &item_seat(task))
}

/// Where every item copy of `team` lives (agents get access to it).
#[must_use]
pub fn items_root(copies_root: &Path, team: &str) -> PathBuf {
    copies_root.join(short(team, team))
}

/// The project's current branch.
pub fn current_branch(project: &Path) -> Result<String, LocalError> {
    git(
        &project_root(project)?,
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )
    .map_err(unavailable)
}

#[derive(Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    /// What changed (`git diff --stat`).
    Merged { summary: String },
    /// git's explanation; nothing was merged and the copy is kept.
    Conflict(String),
}

/// Commits the item copy's work and merges its branch into the project's current branch (which must be
/// `into` when given), then removes the copy; the branch is kept. A conflict is undone and reported.
pub fn merge_item(
    project: &Path,
    copies_root: &Path,
    team: &str,
    task: &str,
    into: Option<&str>,
) -> Result<MergeOutcome, LocalError> {
    if !valid_name(team, &item_seat(task)) {
        return Err(unavailable(format!(
            "{team} has no letters or digits to name a copy with."
        )));
    }
    let dir = item_dir(copies_root, team, task);
    let root = project_root(project)?;
    let branch = branch_of(team, &item_seat(task));
    if !dir.join(".git").exists() {
        // Merged before, and the result was lost (a restart): the branch is already in.
        let merged = git(&root, &["merge-base", "--is-ancestor", &branch, "HEAD"]).is_ok();
        return if merged {
            Ok(MergeOutcome::Merged {
                summary: format!("{branch} was already merged."),
            })
        } else {
            Err(LocalError::not_found(
                "no_copy",
                format!("Item {task} has no copy to merge"),
            ))
        };
    }
    let current = current_branch(&root)?;
    if let Some(into) = into.filter(|b| *b != current) {
        return Err(LocalError::conflict(
            "wrong_branch",
            format!(
                "The project folder is on {current}; this merge stage merges into {into}. Switch branches on the Git page."
            ),
        ));
    }
    commit_work(&dir, &item_seat(task)).map_err(|e| LocalError::conflict("merge_failed", e))?;
    let out = merge_branch(&root, &branch)?;
    if matches!(out, MergeOutcome::Merged { .. }) {
        let _writing = repo_writes(&root);
        let _ = git(
            &root,
            &["worktree", "remove", "--force", &dir.display().to_string()],
        );
    }
    Ok(out)
}

/// Merges `branch` into the project's current branch, one merge at a time in this process. It never starts
/// while the project is mid-merge, rebase or cherry-pick (the user's own work), and it undoes only a merge
/// it started that stopped on conflicts. Any other refusal by git is an error that changed nothing.
fn merge_branch(root: &Path, branch: &str) -> Result<MergeOutcome, LocalError> {
    let _writing = repo_writes(root);
    let failed = |e: String| LocalError::conflict("merge_failed", e);
    for state in [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
    ] {
        if root
            .join(git(root, &["rev-parse", "--git-path", state]).map_err(failed)?)
            .exists()
        {
            return Err(LocalError::conflict(
                "project_busy",
                "The project folder is in the middle of a merge, rebase or cherry-pick. Finish or abort it on the Git page first; nothing was merged.",
            ));
        }
    }
    let before = git(root, &["rev-parse", "HEAD"]).map_err(failed)?;
    if let Err(e) = git(root, &["merge", "--no-ff", "--no-edit", branch]) {
        let unmerged = git(root, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
        let ours = git(root, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).ok()
            == git(root, &["rev-parse", branch]).ok();
        if !unmerged.is_empty() && ours {
            let _ = git(root, &["merge", "--abort"]);
            return Ok(MergeOutcome::Conflict(e));
        }
        return Err(failed(format!(
            "git did not merge {branch}, and nothing was changed: {e}"
        )));
    }
    let after = git(root, &["rev-parse", "HEAD"]).map_err(failed)?;
    if after == before {
        return Ok(MergeOutcome::Merged {
            summary: format!("Nothing to merge: {branch} is already in this branch."),
        });
    }
    let summary = git(root, &["diff", "--stat", &before, &after])
        .unwrap_or_default()
        .chars()
        .take(2000)
        .collect();
    Ok(MergeOutcome::Merged { summary })
}

/// Removes the copy's folder once it has no uncommitted work. Its branch, and every commit on it, is kept.
pub fn remove(
    project: &Path,
    copies_root: &Path,
    team: &str,
    seat: &CopySeat,
) -> Result<(), LocalError> {
    let dir = stopped_copy(copies_root, team, seat)?;
    let root = project_root(project)?;
    let changed = git(&dir, &["status", "--porcelain"])
        .map(|s| s.lines().count())
        .unwrap_or(0);
    if changed > 0 {
        return Err(LocalError::conflict(
            "uncommitted",
            format!(
                "This copy has {changed} uncommitted file(s). Merge it first so the work is kept."
            ),
        ));
    }
    let _writing = repo_writes(&root);
    git(
        &root,
        &["worktree", "remove", "--force", &dir.display().to_string()],
    )
    .map_err(|e| LocalError::conflict("remove_failed", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=T", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Stands in for a filesystem that clones (this one may not): a plain copy shows what lands where.
    fn copy_file(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::copy(from, to).map(|_| ())
    }

    #[test]
    fn a_cloned_copy_is_exactly_its_branch_plus_the_projects_build_output() {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, root) = (tmp.path().join("proj"), tmp.path().join("copies"));
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        sh(&proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(proj.join("src/a.txt"), "committed").unwrap();
        sh(&proj, &["add", "-A"]);
        sh(&proj, &["commit", "-qm", "first"]);
        // The project's own state: a built output, an uncommitted edit and an untracked draft.
        std::fs::create_dir_all(proj.join("target/debug")).unwrap();
        std::fs::write(proj.join("target/debug/app"), "built").unwrap();
        std::fs::write(proj.join("src/a.txt"), "edited, not committed").unwrap();
        std::fs::write(proj.join("draft.md"), "the person's draft").unwrap();
        let at = prepare_with(&root, "t", "team.a@t", None, &proj, copy_file).unwrap();
        assert_eq!(
            std::fs::read_to_string(at.join("src/a.txt")).unwrap(),
            "committed",
            "tracked files are the branch's"
        );
        assert!(
            !at.join("draft.md").exists(),
            "the project's untracked work stays out"
        );
        assert_eq!(
            std::fs::read_to_string(at.join("target/debug/app")).unwrap(),
            "built",
            "ignored build output comes along"
        );
        assert_eq!(
            sh(&at, &["status", "--porcelain"]),
            "",
            "the copy starts clean"
        );
        assert_eq!(
            sh(&at, &["branch", "--show-current"]),
            branch_of("t", "team.a@t")
        );
        assert_eq!(
            std::fs::read_to_string(proj.join("src/a.txt")).unwrap(),
            "edited, not committed",
            "the project is untouched"
        );
    }

    #[test]
    fn a_copy_is_made_reused_listed_merged_and_removed_and_running_agents_are_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = tmp.path().join("proj");
        let root = tmp.path().join("copies");
        std::fs::create_dir_all(proj.join("src")).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let seat = "team.a@t";
        assert_eq!(
            prepare(&root, "t", seat, None, &proj).unwrap_err().code,
            "copy_unavailable",
            "not a git repository"
        );
        sh(&proj, &["init", "-q", "-b", "main"]);
        assert!(
            prepare(&root, "t", seat, None, &proj)
                .unwrap_err()
                .message
                .contains("no commits")
        );
        std::fs::write(proj.join("src/a.txt"), "a").unwrap();
        sh(&proj, &["add", "-A"]);
        sh(&proj, &["commit", "-qm", "first"]);
        let at = prepare(&root, "t", seat, None, &proj.join("src")).unwrap();
        assert!(at.starts_with(&root) && at.ends_with("src") && at.join("a.txt").exists());
        assert_eq!(
            prepare(&root, "t", seat, None, &proj.join("src")).unwrap(),
            at,
            "reused"
        );
        std::fs::write(at.join("b.txt"), "from the agent").unwrap();
        let running = CopySeat {
            seat: seat.into(),
            running: true,
        };
        let stopped = CopySeat {
            seat: seat.into(),
            running: false,
        };
        let list = read(
            Some(&proj),
            &root,
            "t",
            std::slice::from_ref(&stopped),
            true,
        );
        assert_eq!(
            (
                list.into.as_str(),
                list.copies.len(),
                list.copies[0].uncommitted
            ),
            ("main", 1, 1)
        );
        assert_eq!(
            merge(&proj, &root, "t", &running).unwrap_err().code,
            "agent_running"
        );
        assert_eq!(
            remove(&proj, &root, "t", &stopped).unwrap_err().code,
            "uncommitted"
        );
        merge(&proj, &root, "t", &stopped).unwrap();
        assert_eq!(
            std::fs::read_to_string(proj.join("src/b.txt")).unwrap(),
            "from the agent"
        );
        remove(&proj, &root, "t", &stopped).unwrap();
        assert!(!at.exists());
        assert!(
            sh(&proj, &["branch", "--list", &branch_of("t", seat)]).contains("sculptured/t/a"),
            "the branch is kept"
        );
        assert_eq!(
            merge(&proj, &root, "t", &stopped).unwrap_err().code,
            "no_copy"
        );
    }

    #[test]
    fn an_item_copy_is_made_merged_into_the_current_branch_and_removed_and_a_conflict_changes_nothing()
     {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, root) = (tmp.path().join("proj"), tmp.path().join("copies"));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        sh(&proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "one\n").unwrap();
        sh(&proj, &["add", "-A"]);
        sh(&proj, &["commit", "-qm", "first"]);
        let at = prepare(&root, "t", &item_seat("bug-1"), None, &proj).unwrap();
        assert_eq!(at, item_dir(&root, "t", "bug-1"));
        std::fs::write(at.join("a.txt"), "fixed\n").unwrap();
        match merge_item(&proj, &root, "t", "bug-1", None).unwrap() {
            MergeOutcome::Merged { summary } => assert!(summary.contains("a.txt"), "{summary}"),
            o => panic!("{o:?}"),
        }
        assert_eq!(
            std::fs::read_to_string(proj.join("a.txt")).unwrap(),
            "fixed\n"
        );
        assert!(!at.exists(), "a merged item's copy is removed");
        assert!(
            sh(&proj, &["branch", "--list", "sculptured/t/item-bug-1"]).contains("item-bug-1"),
            "its branch is kept"
        );
        // A conflict: the item and main both change a.txt.
        let at = prepare(&root, "t", &item_seat("bug-2"), None, &proj).unwrap();
        std::fs::write(at.join("a.txt"), "item\n").unwrap();
        std::fs::write(proj.join("a.txt"), "main\n").unwrap();
        sh(&proj, &["commit", "-qam", "main moves"]);
        let head = sh(&proj, &["rev-parse", "HEAD"]);
        assert!(matches!(
            merge_item(&proj, &root, "t", "bug-2", None).unwrap(),
            MergeOutcome::Conflict(_)
        ));
        assert_eq!(sh(&proj, &["rev-parse", "HEAD"]), head, "nothing merged");
        assert!(
            sh(&proj, &["status", "--porcelain"]).is_empty(),
            "working tree left clean"
        );
        assert!(at.exists(), "a conflicted item keeps its copy");
        assert!(
            merge_item(&proj, &root, "t", "bug-2", Some("release"))
                .unwrap_err()
                .message
                .contains("release"),
            "into must be the current branch"
        );
        assert_eq!(
            merge_item(&proj, &root, "t", "never", None)
                .unwrap_err()
                .code,
            "no_copy"
        );
        // Merged before (a restart lost the result): its branch is already in, so it counts as merged.
        assert!(matches!(
            merge_item(&proj, &root, "t", "bug-1", None).unwrap(),
            MergeOutcome::Merged { .. }
        ));
    }

    fn repo_with_item(task: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let (proj, root) = (tmp.path().join("proj"), tmp.path().join("copies"));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        sh(&proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "one\n").unwrap();
        sh(&proj, &["add", "-A"]);
        sh(&proj, &["commit", "-qm", "first"]);
        let at = prepare(&root, "t", &item_seat(task), None, &proj).unwrap();
        (tmp, proj, root, at)
    }

    #[test]
    fn a_merge_never_undoes_the_users_own_merge_in_progress() {
        let (_tmp, proj, root, at) = repo_with_item("x");
        std::fs::write(at.join("b.txt"), "item\n").unwrap();
        // The user is resolving a merge of their own in the project folder.
        sh(&proj, &["checkout", "-qb", "side"]);
        std::fs::write(proj.join("a.txt"), "side\n").unwrap();
        sh(&proj, &["commit", "-qam", "side"]);
        sh(&proj, &["checkout", "-q", "main"]);
        std::fs::write(proj.join("a.txt"), "main\n").unwrap();
        sh(&proj, &["commit", "-qam", "main"]);
        let _ = std::process::Command::new("git")
            .current_dir(&proj)
            .args(["merge", "side"])
            .output();
        std::fs::write(proj.join("a.txt"), "resolved by user\n").unwrap();
        let e = merge_item(&proj, &root, "t", "x", None).unwrap_err();
        assert_eq!(e.code, "project_busy", "{e:?}");
        assert_eq!(
            std::fs::read_to_string(proj.join("a.txt")).unwrap(),
            "resolved by user\n"
        );
        assert!(
            proj.join(".git/MERGE_HEAD").exists(),
            "the user's merge is still in progress"
        );
    }

    #[test]
    fn a_merge_git_refuses_for_another_reason_is_not_called_a_conflict() {
        let (_tmp, proj, root, at) = repo_with_item("y");
        std::fs::write(at.join("a.txt"), "item\n").unwrap();
        std::fs::write(proj.join("a.txt"), "user's unsaved edit\n").unwrap(); // uncommitted, and the merge would overwrite it
        let e = merge_item(&proj, &root, "t", "y", None).unwrap_err();
        assert_eq!(e.code, "merge_failed", "{e:?}");
        assert_eq!(
            std::fs::read_to_string(proj.join("a.txt")).unwrap(),
            "user's unsaved edit\n"
        );
    }

    #[test]
    fn a_merge_with_nothing_to_merge_says_so() {
        let (_tmp, proj, root, _at) = repo_with_item("z");
        std::fs::write(proj.join("c.txt"), "user's own commit\n").unwrap();
        sh(&proj, &["add", "-A"]);
        sh(&proj, &["commit", "-qm", "user"]);
        match merge_item(&proj, &root, "t", "z", None).unwrap() {
            MergeOutcome::Merged { summary } => assert!(
                summary.contains("Nothing to merge") && !summary.contains("c.txt"),
                "{summary}"
            ),
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn a_team_or_item_name_with_nothing_usable_is_refused() {
        let (_tmp, proj, root, _at) = repo_with_item("w");
        assert!(prepare(&root, "@@", &item_seat("w"), None, &proj).is_err());
        assert!(!valid_name("@@", "@@") && valid_name("t", "t"));
    }

    #[test]
    fn copies_of_one_repository_can_be_made_at_the_same_time() {
        let (_tmp, proj, root, _at) = repo_with_item("first");
        for round in 0..20 {
            let made: Vec<_> = (0..4)
                .map(|i| {
                    let (proj, root) = (proj.clone(), root.clone());
                    std::thread::spawn(move || {
                        prepare(
                            &root,
                            "t",
                            &item_seat(&format!("r{round}-{i}")),
                            None,
                            &proj,
                        )
                    })
                })
                .collect();
            for m in made {
                m.join().unwrap().unwrap();
            }
        }
    }

    #[test]
    fn reusing_a_copy_never_waits_for_another_repositorys_git_work() {
        let (_tmp, proj, root, at) = repo_with_item("kept");
        let (held, release) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
        let held_repo = proj.clone();
        let busy = std::thread::spawn(move || {
            let _w = repo_writes(&held_repo);
            held.0.send(()).unwrap();
            let _ = release.1.recv_timeout(std::time::Duration::from_secs(2));
        });
        held.1.recv().unwrap();
        let began = std::time::Instant::now();
        let again = prepare(&root, "t", &item_seat("kept"), None, &proj).unwrap();
        let waited = began.elapsed();
        let _ = release.0.send(());
        busy.join().unwrap();
        assert_eq!(again, at);
        assert!(
            waited < std::time::Duration::from_millis(500),
            "waited {waited:?} on the lock"
        );
    }

    #[test]
    fn git_work_in_one_repository_never_waits_for_another() {
        let (_a, proj_a, _root_a, _at) = repo_with_item("a");
        let (_b, proj_b, root_b, _at) = repo_with_item("b");
        let (held, release) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
        let busy = std::thread::spawn(move || {
            let _w = repo_writes(&proj_a);
            held.0.send(()).unwrap();
            let _ = release.1.recv_timeout(std::time::Duration::from_secs(2));
        });
        held.1.recv().unwrap();
        let began = std::time::Instant::now();
        prepare(&root_b, "t", &item_seat("new"), None, &proj_b).unwrap();
        let waited = began.elapsed();
        let _ = release.0.send(());
        busy.join().unwrap();
        assert!(
            waited < std::time::Duration::from_millis(1500),
            "waited {waited:?} for another repository"
        );
    }

    #[test]
    fn branch_names_are_short_and_safe() {
        assert_eq!(
            branch_of("c172", "team.fuselage@c172"),
            "sculptured/c172/fuselage"
        );
        assert_eq!(
            branch_of("c 172", "team.a/b:c@c 172"),
            "sculptured/c-172/a-b-c"
        );
    }
}
