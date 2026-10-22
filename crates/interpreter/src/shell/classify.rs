//! Classifies commands by their observed purpose.
use super::lex;
use model::ActionClass;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Other,
    Git,
    Install,
    Build,
    Test,
}

fn tool_kind(name: &str, next: &str) -> Kind {
    match name {
        "vitest" | "jest" | "mocha" | "pytest" | "ctest" => Kind::Test,
        "playwright" if next == "test" => Kind::Test,
        "tsc" | "webpack" | "esbuild" => Kind::Build,
        "vite" if next == "build" => Kind::Build,
        _ => Kind::Other,
    }
}

fn kind(w: &[String]) -> Option<Kind> {
    let a = |i: usize| w.get(i).map(String::as_str).unwrap_or("");
    let first = a(0).rsplit('/').next().unwrap_or("");
    Some(match first {
        "cd" | "pushd" | "popd" => return None,
        "cargo" | "npm" | "pnpm" | "yarn" | "bun" | "go" => package_kind(first, w),
        "npx" | "pnpx" | "bunx" => tool_kind(a(1), a(2)),
        "python" | "python3" if a(1) == "-m" && a(2) == "pytest" => Kind::Test,
        "python" | "python3" if a(1) == "-m" && a(2) == "pip" && a(3) == "install" => Kind::Install,
        "make" if w[1..].iter().any(|x| x == "test" || x == "check") => Kind::Test,
        "make" | "cmake" | "ninja" => Kind::Build,
        "mvn" | "gradle" | "./gradlew" | "gradlew" if w[1..].iter().any(|x| x == "test") => {
            Kind::Test
        }
        "mvn" | "gradle" | "./gradlew" | "gradlew" => Kind::Build,
        "git" => Kind::Git,
        "pip" | "pip3" if a(1) == "install" => Kind::Install,
        "uv" if a(1) == "add" || (a(1) == "pip" && a(2) == "install") => Kind::Install,
        "apt" | "apt-get" | "brew" | "dnf" | "yum" | "pacman"
            if matches!(a(1), "install" | "-S" | "add") =>
        {
            Kind::Install
        }
        other => tool_kind(other, a(1)),
    })
}

/// The kind of a shell command: a test run if any part runs tests, else a build, an install, git, or other.
#[must_use]
pub fn classify_command(cmd: &str) -> ActionClass {
    let best = lex::segments(&lex::unwrap_shell(cmd))
        .iter()
        .filter_map(|s| kind(&s.words))
        .max()
        .unwrap_or(Kind::Other);
    match best {
        Kind::Test => ActionClass::ShellTestRun,
        Kind::Build => ActionClass::ShellBuild,
        Kind::Install => ActionClass::ShellInstall,
        Kind::Git => ActionClass::ShellGit,
        Kind::Other => ActionClass::ShellOther,
    }
}

fn package_kind(program: &str, words: &[String]) -> Kind {
    let a = |index: usize| words.get(index).map(String::as_str).unwrap_or("");
    match program {
        "cargo" => match a(1) {
            "test" | "nextest" => Kind::Test,
            "build" | "check" | "clippy" | "doc" => Kind::Build,
            "install" | "add" => Kind::Install,
            _ => Kind::Other,
        },
        "npm" | "pnpm" | "yarn" | "bun" => match a(1) {
            "test" | "t" => Kind::Test,
            "run" if a(2).starts_with("test") || a(2) == "e2e" => Kind::Test,
            "run" if a(2).starts_with("build") => Kind::Build,
            "build" => Kind::Build,
            "install" | "i" | "ci" | "add" => Kind::Install,
            _ => Kind::Other,
        },
        "go" => match a(1) {
            "test" => Kind::Test,
            "build" | "vet" => Kind::Build,
            "get" | "install" => Kind::Install,
            _ => Kind::Other,
        },
        _ => Kind::Other,
    }
}
