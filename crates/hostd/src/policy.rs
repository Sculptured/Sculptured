//! What a connected computer lets the server do, decided on the computer (`runner.json`, written by
//! `sculptured connect` / `sculptured allow`). Launches outside the allowed folders, and any server-supplied
//! program, environment or extra arguments, are refused here. Allowed folders decide where an agent
//! *starts*; what it may do once running is the `access` level: `full` (the CLIs' full-access modes, the
//! default) or `sandboxed`. The server can lower access but never raise it.
use host_proto::LaunchParams;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How much agents may do on this computer, chosen by its owner at `sculptured connect`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// Agents act as you do on this computer (the CLIs' own full-access modes). The default.
    #[default]
    Full,
    /// Claude asks before risky actions; Codex writes only inside its folder. The server cannot lift it.
    Sandboxed,
}

/// What else, beyond running agents, a hosted Sculptured may do on this computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub git: bool,
    pub copies: bool,
    pub checks: bool,
    pub cli_sign_in: bool,
    pub plan_usage: bool,
    /// One tool-less Claude Code / Codex call that drafts a team (no files, no tools).
    pub generate: bool,
    /// Reports of where a team run's tokens went, from the CLIs' session files: sizes, causes, command
    /// lines (common secrets masked) and file names; not file contents or agents' messages.
    pub usage_reports: bool,
}

/// The owner's own choices; a capability left unset follows the access level's default, so switching
/// to sandboxed later still turns checks off unless the owner turned them on by name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityChoices {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copies: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_sign_in: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_usage: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_reports: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerPolicy {
    #[serde(default)]
    pub allowed_folders: Vec<PathBuf>,
    #[serde(default)]
    pub access: Access,
    /// Unset ones: the defaults for `access` (everything; checks off when sandboxed).
    #[serde(default)]
    pub capabilities: CapabilityChoices,
}

impl RunnerPolicy {
    pub fn load(path: &Path) -> Result<RunnerPolicy, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => serde_json::from_str(&t)
                .map_err(|e| format!("{} is not valid: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RunnerPolicy::default()),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("json.tmp");
        crate::private::write_file(
            &tmp,
            serde_json::to_string_pretty(self)
                .expect("serializes")
                .as_bytes(),
        )
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    pub fn allow(&mut self, folder: &Path) -> Result<PathBuf, String> {
        let c = localops::folders::plain(
            folder
                .canonicalize()
                .map_err(|e| format!("{} can't be used: {e}", folder.display()))?,
        );
        if !c.is_dir() {
            return Err(format!("{} is not a folder", c.display()));
        }
        if !self.allowed_folders.contains(&c) {
            self.allowed_folders.push(c.clone())
        }
        Ok(c)
    }

    /// The permission posture to launch with: what the server asked, never more than this computer allows.
    #[must_use]
    pub fn posture(&self, asked: model::PermissionPosture) -> model::PermissionPosture {
        match self.access {
            Access::Full => asked,
            Access::Sandboxed => model::PermissionPosture::Restricted,
        }
    }

    #[must_use]
    pub fn sandboxed(&self) -> bool {
        self.access == Access::Sandboxed
    }

    #[must_use]
    pub fn caps(&self) -> Capabilities {
        let c = &self.capabilities;
        Capabilities {
            git: c.git.unwrap_or(true),
            copies: c.copies.unwrap_or(true),
            checks: c.checks.unwrap_or(!self.sandboxed()),
            cli_sign_in: c.cli_sign_in.unwrap_or(true),
            plan_usage: c.plan_usage.unwrap_or(true),
            generate: c.generate.unwrap_or(true),
            usage_reports: c.usage_reports.unwrap_or(true),
        }
    }

    /// A path a request names, resolved like an agent's folder: absolute, or under the first allowed
    /// folder; it must lie inside an allowed folder after resolving `..` and symlinks.
    pub fn resolve(&self, path: &str) -> Result<PathBuf, localops::LocalError> {
        self.check(&LaunchParams {
            cwd: path.to_string(),
            ..Default::default()
        })
        .map_err(|why| localops::LocalError::forbidden("not_allowed_here", why))
    }

    /// The allowed folder that holds `path` (already resolved).
    pub fn root_of(&self, path: &Path) -> Option<&Path> {
        self.allowed_folders
            .iter()
            .map(PathBuf::as_path)
            .find(|r| path.starts_with(r))
    }

    /// The request with its paths resolved, if this computer allows it; else why not.
    pub fn check_op(
        &self,
        op: &localops::LocalOp,
    ) -> Result<localops::LocalOp, localops::LocalError> {
        use localops::LocalOp as Op;
        let caps = self.caps();
        let off = |what: &str, flag: &str| {
            Err(localops::LocalError::forbidden(
                "not_allowed_here",
                format!(
                    "{what} is turned off on this computer (sculptured connect … --enable {flag})"
                ),
            ))
        };
        let p = |s: &str| self.resolve(s).map(|p| p.display().to_string());
        Ok(match op.clone() {
            Op::Describe | Op::ListFolder { path: None, .. } | Op::Clis => op.clone(),
            Op::ListFolder {
                path: Some(path),
                files,
            } => Op::ListFolder {
                path: Some(p(&path)?),
                files,
            },
            Op::CreateFolder { parent, name } => Op::CreateFolder {
                parent: p(&parent)?,
                name,
            },
            Op::GitStatus { .. } | Op::GitDiff { .. } | Op::GitAct { .. } if !caps.git => {
                return off("Git", "git");
            }
            Op::GitStatus { repo } => Op::GitStatus { repo: p(&repo)? },
            Op::GitDiff { repo, path, staged } => Op::GitDiff {
                repo: p(&repo)?,
                path,
                staged,
            },
            Op::GitAct { repo, action } => Op::GitAct {
                repo: p(&repo)?,
                action,
            },
            Op::CopyPrepare { .. }
            | Op::CopiesRead { .. }
            | Op::CopyMerge { .. }
            | Op::CopyRemove { .. }
                if !caps.copies =>
            {
                return off("Agents' own copies", "copies");
            }
            Op::CopyPrepare {
                team,
                seat,
                parent,
                cwd,
            } => Op::CopyPrepare {
                team,
                seat,
                parent,
                cwd: self.whole_repo(p(&cwd)?)?,
            },
            Op::CopiesRead {
                project,
                team,
                seats,
                enabled,
            } => Op::CopiesRead {
                project: project
                    .map(|x| p(&x).and_then(|x| self.whole_repo(x)))
                    .transpose()?,
                team,
                seats,
                enabled,
            },
            Op::CopyMerge {
                project,
                team,
                seat,
            } => Op::CopyMerge {
                project: self.whole_repo(p(&project)?)?,
                team,
                seat,
            },
            Op::CopyRemove {
                project,
                team,
                seat,
            } => Op::CopyRemove {
                project: self.whole_repo(p(&project)?)?,
                team,
                seat,
            },
            Op::ItemCopyPrepare { .. } | Op::ItemMerge { .. } if !caps.copies => {
                return off("Copies of work items", "copies");
            }
            Op::ItemCopyPrepare {
                project,
                team,
                task,
            } => Op::ItemCopyPrepare {
                project: self.whole_repo(p(&project)?)?,
                team,
                task,
            },
            Op::ItemMerge {
                project,
                team,
                task,
                into,
            } => Op::ItemMerge {
                project: self.whole_repo(p(&project)?)?,
                team,
                task,
                into,
            },
            // An item's copy is found by the runner itself (`item`), so only the project folder is a path here.
            Op::RunCheck { .. } if !caps.checks => return off("Checks", "checks"),
            Op::RunCheck {
                cwd,
                item,
                command,
                timeout_s,
            } => Op::RunCheck {
                cwd: p(&cwd)?,
                item,
                command,
                timeout_s,
            },
            Op::RunDispatch { .. } if !caps.checks => {
                return off("Checks (and workflow dispatch commands)", "checks");
            }
            Op::RunDispatch {
                cwd,
                command,
                timeout_s,
            } => Op::RunDispatch {
                cwd: p(&cwd)?,
                command,
                timeout_s,
            },
            Op::CliLogin { .. } | Op::CliLoginCode { .. } if !caps.cli_sign_in => {
                return off(
                    "Signing in to Claude or Codex from the browser",
                    "cli-sign-in",
                );
            }
            Op::CliLogin { name } => Op::CliLogin { name },
            Op::CliLoginCode { name, code } => Op::CliLoginCode { name, code },
            Op::PlanUsage if !caps.plan_usage => return off("Reading plan usage", "plan-usage"),
            Op::Generate { .. } if !caps.generate => {
                return off("Generating teams with AI", "generate");
            }
            Op::Generate { .. } => op.clone(),
            Op::PlanUsage => Op::PlanUsage,
            Op::AnalyzeRun { .. } if !caps.usage_reports => {
                return off("Reports of where tokens went", "usage-reports");
            }
            Op::AnalyzeRun { .. } => op.clone(),
            Op::ProjectMap { path, shared } => Op::ProjectMap {
                path: p(&path)?,
                shared,
            },
            Op::KeepLesson { path, text } => Op::KeepLesson {
                path: p(&path)?,
                text,
            },
        })
    }

    /// `folder` (resolved), if the git repository holding it lies inside an allowed folder: a copy, and
    /// merging it back, covers the whole repository, not just the folder. Not a repository: left to
    /// `localops`, which says so.
    pub fn whole_repo(&self, folder: String) -> Result<String, localops::LocalError> {
        let Ok(root) = localops::copies::project_root(Path::new(&folder)) else {
            return Ok(folder);
        };
        let root = root
            .canonicalize()
            .map(localops::folders::plain)
            .unwrap_or(root);
        if self.root_of(&root).is_none() {
            return Err(localops::LocalError::forbidden(
                "not_allowed_here",
                format!(
                    "{folder} is inside the git repository {}, which reaches outside the folders this computer allows. Agents' own copies cover the whole repository: allow it with `sculptured allow {}`, or switch off own copies.",
                    root.display(),
                    root.display()
                ),
            ));
        }
        Ok(folder)
    }

    /// What this computer allows, for the Hello and the Computers page.
    #[must_use]
    pub fn info(&self) -> host_proto::HostPolicyInfo {
        let c = self.caps();
        let caps = [
            ("git", c.git),
            ("copies", c.copies),
            ("checks", c.checks),
            ("cli_sign_in", c.cli_sign_in),
            ("plan_usage", c.plan_usage),
            ("generate", c.generate),
            ("usage_reports", c.usage_reports),
        ];
        host_proto::HostPolicyInfo {
            allowed_folders: self
                .allowed_folders
                .iter()
                .map(|f| f.display().to_string())
                .collect(),
            access: if self.sandboxed() {
                "sandboxed".into()
            } else {
                "full".into()
            },
            capabilities: caps
                .iter()
                .filter(|(_, on)| *on)
                .map(|(n, _)| n.to_string())
                .collect(),
            platform: std::env::consts::OS.into(),
        }
    }

    /// The folder the agent will run in, or why not.
    pub fn check(&self, p: &LaunchParams) -> Result<PathBuf, String> {
        // The server may give an agent its model connection's key, and nothing else of its environment.
        if p.program.is_some() || !host_proto::model_env_allowed(&p.env) || !p.extra_args.is_empty()
        {
            return Err("refused: the server asked to change the program, its environment or its arguments; this computer only runs its own Claude Code / Codex".into());
        }
        let first = self.allowed_folders.first().ok_or(
            "No folders are allowed on this computer yet. Run `sculptured allow <folder>` on it.",
        )?;
        let asked = Path::new(&p.cwd);
        let joined = if asked.is_absolute() {
            asked.to_path_buf()
        } else {
            first.join(asked)
        };
        // Plain form (`C:\…`, not `\\?\C:\…`) on Windows, as the allowed folders are kept.
        let real = localops::folders::plain(
            joined
                .canonicalize()
                .map_err(|_| format!("{} does not exist on this computer", joined.display()))?,
        );
        if self
            .allowed_folders
            .iter()
            .any(|root| real.starts_with(root))
        {
            Ok(real)
        } else {
            Err(format!(
                "{} is not in a folder this computer allows. Allowed: {}. Add one with `sculptured allow <folder>`.",
                real.display(),
                self.allowed_folders
                    .iter()
                    .map(|f| f.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }
}
