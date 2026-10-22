use std::path::PathBuf;

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
