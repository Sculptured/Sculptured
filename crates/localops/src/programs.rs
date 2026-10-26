//! Finding the Claude Code and Codex CLIs on any system (Linux, macOS, Windows), at the moment they are
//! run: a CLI installed after the runner started is found without restarting it.
use std::path::PathBuf;

/// This user's home folder (`HOME` on Unix, the profile folder on Windows).
#[must_use]
pub fn home() -> Option<PathBuf> {
    std::env::home_dir().filter(|h| !h.as_os_str().is_empty())
}

/// The program to run for a CLI such as `claude`: its full path when it is on PATH or where its installers
/// put it, else the bare name. On Windows this also finds `claude.cmd` / `codex.cmd` (npm's launchers)
/// through PATHEXT, which a bare name would miss. A login service may start with a PATH that lacks the
/// user's own folders (installed after logging in, or set only in a shell profile), hence the known places.
pub fn program(name: &str) -> PathBuf {
    program_in(
        name,
        std::env::var_os("PATH").as_deref(),
        home().as_deref(),
        std::env::var_os("APPDATA").map(PathBuf::from).as_deref(),
    )
}

/// `program` with the home and (Windows) roaming app-data folders given.
#[must_use]
pub fn program_in(
    name: &str,
    path: Option<&std::ffi::OsStr>,
    home: Option<&std::path::Path>,
    appdata: Option<&std::path::Path>,
) -> PathBuf {
    if let Some(found) = path.and_then(|p| find_in(name, p)) {
        return found;
    }
    let mut places: Vec<PathBuf> = Vec::new();
    if let Some(h) = home {
        if cfg!(windows) {
            places.push(h.join(".local").join("bin").join(format!("{name}.exe")));
        } else {
            // `.opencode/bin` is where OpenCode's own installer puts it.
            for dir in [
                ".local/bin",
                ".npm-global/bin",
                ".bun/bin",
                ".volta/bin",
                ".opencode/bin",
            ] {
                places.push(h.join(dir).join(name));
            }
        }
    }
    if cfg!(windows) {
        if let Some(a) = appdata {
            places.push(a.join("npm").join(format!("{name}.cmd")));
        }
    } else {
        places.extend(
            ["/opt/homebrew/bin", "/usr/local/bin"]
                .iter()
                .map(|d| PathBuf::from(d).join(name)),
        );
    }
    places
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// The program to run for `program`: a bare name (`claude`) is looked up now (`program`); a path is kept.
#[must_use]
pub fn resolve(program: &std::path::Path) -> PathBuf {
    match program.to_str() {
        Some(name) if program.components().count() == 1 && !name.contains(['/', '\\']) => {
            crate::programs::program(name)
        }
        _ => program.to_path_buf(),
    }
}

/// `name` on PATH, as a full path (Windows: with an extension from PATHEXT, `.exe` and `.cmd` first).
#[must_use]
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    find_in(name, &std::env::var_os("PATH")?)
}

fn find_in(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into());
        pathext
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(path).find_map(|dir| {
        exts.iter()
            .map(|e| dir.join(format!("{name}{e}")))
            .find(|f| f.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cli_off_the_path_is_found_where_its_installer_puts_it() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join(".local").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let file = if cfg!(windows) {
            "claude.exe"
        } else {
            "claude"
        };
        std::fs::write(bin.join(file), b"").unwrap();
        assert_eq!(
            program_in("claude", None, Some(home.path()), None),
            bin.join(file),
            "not on PATH: the native installer's folder"
        );
        assert_eq!(
            program_in("codex", None, Some(home.path()), None),
            PathBuf::from("codex"),
            "nowhere: the bare name, so the error names it"
        );
    }

    #[test]
    fn a_path_is_kept_and_a_bare_name_is_looked_up_when_used() {
        assert_eq!(
            resolve(std::path::Path::new("/opt/x/claude")),
            PathBuf::from("/opt/x/claude")
        );
        assert_eq!(
            resolve(std::path::Path::new("sculptured-no-such-cli")),
            PathBuf::from("sculptured-no-such-cli"),
            "nowhere: the bare name, so the error names it"
        );
        if let Some(sh) = find_on_path("sh") {
            assert_eq!(resolve(std::path::Path::new("sh")), sh, "found on PATH now");
        }
    }

    #[test]
    fn windows_long_paths_read_as_people_write_them() {
        assert_eq!(
            crate::folders::plain(PathBuf::from(r"\\?\C:\Users\Quickemu")),
            PathBuf::from(r"C:\Users\Quickemu")
        );
        assert_eq!(
            crate::folders::plain(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\?\UNC\server\share"),
            "network shares keep their form"
        );
        assert_eq!(
            crate::folders::plain(PathBuf::from("/home/a")),
            PathBuf::from("/home/a")
        );
    }
}
