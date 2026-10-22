//! Recognizes risks from command words and working directory changes.
use super::lex;
use model::RiskFlag;
use std::path::Path;

fn positionals(args: &[String]) -> Vec<&str> {
    args.iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect()
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?;
    Some(host.split(':').next()?.to_string()).filter(|h| !h.is_empty())
}

fn curl_writes(args: &[String]) -> bool {
    let write_method = |m: &str| {
        matches!(
            m.to_ascii_uppercase().as_str(),
            "POST" | "PUT" | "PATCH" | "DELETE"
        )
    };
    args.iter().enumerate().any(|(i, a)| match a.as_str() {
        "-X" | "--request" => args.get(i + 1).is_some_and(|m| write_method(m)),
        "-d" | "-F" | "--form" | "-T" | "--upload-file" => true,
        a if a.starts_with("--data") => true,
        a if a.starts_with("-X") && a.len() > 2 => write_method(&a[2..]),
        _ => false,
    })
}

/// Where the command's later parts run, after a `cd` earlier in the same command.
enum Dir {
    /// No `cd` yet: the agent's folder.
    Start,
    /// The folder a `cd` named, as typed (absolute, `~…`, or relative to the agent's folder).
    At(String),
    /// A `cd` to a folder that cannot be read from the words (`cd -`, `cd "$OUT"`, `popd`).
    Unknown,
}

impl Dir {
    /// `path` as seen from the agent's folder, or None when the folder it is in is unknown.
    fn resolve(&self, path: &str) -> Option<String> {
        if path.starts_with('/')
            || path.starts_with('~')
            || path.starts_with("$HOME")
            || path.starts_with("${HOME}")
        {
            return Some(path.to_string());
        }
        match self {
            Dir::Start => Some(path.to_string()),
            Dir::At(dir) => Some(format!("{}/{path}", dir.trim_end_matches('/'))),
            Dir::Unknown => None,
        }
    }

    fn cd(&self, args: &[String]) -> Dir {
        if args.iter().any(|a| a == "-") {
            return Dir::Unknown;
        }
        match positionals(args).first().copied() {
            None => Dir::At("~".into()),
            Some(to)
                if to.starts_with('$')
                    && !(to.starts_with("$HOME") || to.starts_with("${HOME}")) =>
            {
                Dir::Unknown
            }
            Some(to) => self.resolve(to).map(Dir::At).unwrap_or(Dir::Unknown),
        }
    }
}

/// Risks of a shell command, before or after it ran. A `cd` earlier in the same command is followed,
/// so `cd /tmp && rm -rf build` deletes `/tmp/build`; a `cd` the words cannot place flags nothing after it.
#[must_use]
pub fn risks(cmd: &str, cwd: Option<&Path>) -> Vec<RiskFlag> {
    let mut out = Vec::new();
    let mut dir = Dir::Start;
    for segment in lex::segments(&lex::unwrap_shell(cmd)) {
        if segment.sudo {
            out.push(crate::flag(
                "sudo",
                "ran a command as administrator (sudo)".into(),
            ));
        }
        let Some((program, args)) = segment.words.split_first() else {
            continue;
        };
        let program = program.rsplit('/').next().unwrap_or(program);
        match program {
            "cd" | "pushd" => dir = dir.cd(args),
            "popd" => dir = Dir::Unknown,
            _ => out.extend(command_risks(program, args, &dir, cwd)),
        }
    }
    out
}

fn command_risks(program: &str, args: &[String], dir: &Dir, cwd: Option<&Path>) -> Vec<RiskFlag> {
    match program {
        "git" => git_risk(args).into_iter().collect(),
        "rm" => delete_risks(args, dir, cwd),
        "chmod" => permission_risks(args),
        "curl" if curl_writes(args) => vec![network_risk(args)],
        "wget"
            if args.iter().any(|arg| {
                arg.starts_with("--post") || (arg.starts_with("--method=") && !arg.ends_with("GET"))
            }) =>
        {
            vec![network_risk(args)]
        }
        "scp" | "rsync" => copy_risk(args).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn git_risk(args: &[String]) -> Option<RiskFlag> {
    match args.first()?.as_str() {
        "push" => force_push_risk(&args[1..]),
        "reset" if args.iter().any(|arg| arg == "--hard") => Some(crate::flag(
            "discard-changes",
            "discarded uncommitted changes (git reset --hard)".into(),
        )),
        "clean"
            if args
                .iter()
                .any(|arg| arg.starts_with('-') && !arg.starts_with("--") && arg.contains('f')) =>
        {
            Some(crate::flag(
                "discard-changes",
                "deleted untracked files (git clean)".into(),
            ))
        }
        _ => None,
    }
}

fn force_push_risk(args: &[String]) -> Option<RiskFlag> {
    let forced = args.iter().any(|arg| {
        arg == "-f" || arg.starts_with("--force") || (arg.starts_with('+') && arg.len() > 1)
    });
    if !forced {
        return None;
    }
    let targets = positionals(args);
    let text = if targets.is_empty() {
        "force-pushed to the default remote".to_string()
    } else {
        format!("force-pushed to {}", targets.join(" "))
    };
    Some(crate::flag("force-push", text))
}

fn delete_risks(args: &[String], dir: &Dir, cwd: Option<&Path>) -> Vec<RiskFlag> {
    let recursive = args.iter().any(|arg| {
        arg == "--recursive"
            || (arg.starts_with('-')
                && !arg.starts_with("--")
                && (arg.contains('r') || arg.contains('R')))
    });
    if !recursive {
        return Vec::new();
    }
    positionals(args)
        .into_iter()
        .filter_map(|target| dir.resolve(target))
        .filter(|target| crate::outside(target, cwd))
        .map(|target| {
            crate::flag(
                "outside-folder-delete",
                format!("deleted {target} and everything in it, outside the agent's folder"),
            )
        })
        .collect()
}

fn permission_risks(args: &[String]) -> Vec<RiskFlag> {
    if !args.iter().any(|arg| arg == "-R" || arg == "--recursive") {
        return Vec::new();
    }
    let positionals = positionals(args);
    let Some((mode, targets)) = positionals.split_first() else {
        return Vec::new();
    };
    if !(*mode == "777" || mode.contains("o+w") || mode.contains("a+w") || *mode == "a+rwx") {
        return Vec::new();
    }
    targets
        .iter()
        .map(|target| {
            crate::flag(
                "world-writable",
                format!("made {target} writable by everyone, recursively"),
            )
        })
        .collect()
}

fn network_risk(args: &[String]) -> RiskFlag {
    let host = args
        .iter()
        .find_map(|arg| host_of(arg))
        .unwrap_or_else(|| "an unknown host".into());
    crate::flag("network-write", format!("network write to {host}"))
}

fn copy_risk(args: &[String]) -> Option<RiskFlag> {
    let remote = positionals(args).into_iter().rev().find(|arg| {
        !arg.contains("://")
            && !arg.starts_with('/')
            && !arg.starts_with('.')
            && arg
                .split_once(':')
                .is_some_and(|(host, _)| !host.is_empty())
    })?;
    let host = remote
        .split_once(':')
        .map(|(host, _)| host.rsplit('@').next().unwrap_or(host))
        .unwrap_or(remote);
    Some(crate::flag(
        "network-write",
        format!("copied files to {host}"),
    ))
}
