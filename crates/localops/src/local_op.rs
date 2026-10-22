use crate::{analyze, copies, git};

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
    CreateFolder {
        parent: String,
        name: String,
    },
    GitStatus {
        repo: String,
    },
    GitDiff {
        repo: String,
        path: String,
        staged: bool,
    },
    GitAct {
        repo: String,
        action: git::GitAction,
    },
    CopyPrepare {
        team: String,
        seat: String,
        parent: Option<String>,
        cwd: String,
    },
    CopiesRead {
        project: Option<String>,
        team: String,
        seats: Vec<copies::CopySeat>,
        enabled: bool,
    },
    CopyMerge {
        project: String,
        team: String,
        seat: copies::CopySeat,
    },
    CopyRemove {
        project: String,
        team: String,
        seat: copies::CopySeat,
    },
    /// A work item's own copy of the project (`copies: per-item`); answers its folder.
    ItemCopyPrepare {
        project: String,
        team: String,
        task: String,
    },
    /// Merges an item's branch into the project's current branch; answers `{merged, summary}`.
    ItemMerge {
        project: String,
        team: String,
        task: String,
        into: Option<String>,
    },
    /// A workflow check stage: the command in the item's copy (when `item`), else in `cwd`.
    RunCheck {
        cwd: String,
        item: Option<ItemRef>,
        command: String,
        timeout_s: u32,
    },
    /// A workflow's dispatch command in `cwd`; answers its whole stdout (`command::CommandOutput`). An
    /// older runner refuses it as unknown, so its output is never silently cut to a check's tail.
    RunDispatch {
        cwd: String,
        command: String,
        timeout_s: u32,
    },
    Clis,
    CliLogin {
        name: String,
    },
    /// The code from the sign-in page, for the sign-in under way (`clis::login_code`).
    CliLoginCode {
        name: String,
        code: String,
    },
    PlanUsage,
    /// One structured, tool-less call to the user's Claude Code or Codex (`generate`).
    Generate {
        cli: String,
        model: Option<String>,
        prompt: String,
        schema: serde_json::Value,
    },
    /// Where a team run's tokens went, from this computer's Claude Code and Codex session files and the
    /// run's links (a filtered `sculptured.session/v1`); answers a `mais.report/v1` report.
    AnalyzeRun {
        sessions: Vec<analyze::RunSession>,
        export: String,
    },
    /// The project map of a folder (`map`), also written to its `.sculptured/map.md`.
    ProjectMap {
        path: String,
        /// Values the team's contracts say these files hold; each is checked. Absent from an older server.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        shared: Vec<model::SharedValue>,
    },
    /// Keeps a confirmed lesson in the folder's `.sculptured/lessons.md`.
    KeepLesson {
        path: String,
        text: String,
    },
}

/// A work item of a team, named rather than given as a path: the computer finds its copy itself.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ItemRef {
    pub team: String,
    pub task: String,
}
