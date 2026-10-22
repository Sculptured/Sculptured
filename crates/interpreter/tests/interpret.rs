use model::ActionClass;
use serde_json::json;
use std::path::Path;

const CWD: &str = "/home/u/project";

#[test]
fn quoted_separators_stay_in_arguments_and_real_separators_start_commands() {
    for quoted in ["'cargo test; git push -f'", "\"cargo test || git push -f\""] {
        let command = format!("echo {quoted}");
        assert_eq!(
            interpreter::shell::classify_command(&command),
            ActionClass::ShellOther
        );
        assert!(interpreter::shell::risks(&command, Some(Path::new(CWD))).is_empty());
    }
    for separator in ["&&", "||", ";", "|", "\n"] {
        let command = format!("echo 'a && b' {separator} cargo test");
        assert_eq!(
            interpreter::shell::classify_command(&command),
            ActionClass::ShellTestRun
        );
        let command = format!("echo 'git push -f' {separator} git push -f origin main");
        let flags = interpreter::shell::risks(&command, Some(Path::new(CWD)));
        assert_eq!(codes(&flags), ["force-push"]);
        assert_eq!(flags[0].text, "force-pushed to origin main");
    }
}

fn codes(flags: &[model::RiskFlag]) -> Vec<&str> {
    flags.iter().map(|f| f.code.as_str()).collect()
}

#[test]
fn shell_commands_are_classified() {
    let cases = [
        ("cargo test -p model", ActionClass::ShellTestRun),
        ("cd web && npx vitest run", ActionClass::ShellTestRun),
        (
            "RUST_LOG=debug cargo nextest run",
            ActionClass::ShellTestRun,
        ),
        ("npm run test:unit", ActionClass::ShellTestRun),
        ("python -m pytest -q", ActionClass::ShellTestRun),
        (
            "npx playwright test e2e/inbox.spec.ts",
            ActionClass::ShellTestRun,
        ),
        ("make test", ActionClass::ShellTestRun),
        ("npm run build", ActionClass::ShellBuild),
        ("cargo build --release", ActionClass::ShellBuild),
        ("make", ActionClass::ShellBuild),
        ("npx tsc --noEmit -p .", ActionClass::ShellBuild),
        ("npm ci", ActionClass::ShellInstall),
        ("sudo apt-get install -y jq", ActionClass::ShellInstall),
        ("pip install requests", ActionClass::ShellInstall),
        ("git status", ActionClass::ShellGit),
        ("git add -A && git commit -m 'x'", ActionClass::ShellGit),
        ("ls -la", ActionClass::ShellOther),
        ("/bin/bash -lc 'cargo test'", ActionClass::ShellTestRun),
        ("git stash && cargo test", ActionClass::ShellTestRun),
    ];
    for (cmd, want) in cases {
        assert_eq!(interpreter::shell::classify_command(cmd), want, "{cmd}");
    }
    assert_eq!(
        interpreter::shell::unwrap_shell("/bin/bash -lc 'ls -1'"),
        "ls -1"
    );
    assert_eq!(interpreter::shell::unwrap_shell("ls -1"), "ls -1");
}

#[test]
fn tools_are_classified_with_their_paths() {
    let cases = [
        (
            "Read",
            json!({ "file_path": "/home/u/project/a.rs" }),
            ActionClass::FileRead,
            vec!["/home/u/project/a.rs"],
        ),
        (
            "Edit",
            json!({ "file_path": "src/b.rs", "old_string": "a", "new_string": "b" }),
            ActionClass::FileEdit,
            vec!["src/b.rs"],
        ),
        (
            "Write",
            json!({ "file_path": "c.md", "content": "" }),
            ActionClass::FileEdit,
            vec!["c.md"],
        ),
        (
            "NotebookEdit",
            json!({ "notebook_path": "n.ipynb" }),
            ActionClass::FileEdit,
            vec!["n.ipynb"],
        ),
        (
            "Grep",
            json!({ "pattern": "x", "path": "src" }),
            ActionClass::FileRead,
            vec!["src"],
        ),
        (
            "WebFetch",
            json!({ "url": "https://x.dev" }),
            ActionClass::Web,
            vec![],
        ),
        (
            "webSearch",
            json!({ "query": "x" }),
            ActionClass::Web,
            vec![],
        ),
        (
            "Agent",
            json!({ "prompt": "go" }),
            ActionClass::SubagentSpawn,
            vec![],
        ),
        (
            "Task",
            json!({ "prompt": "go" }),
            ActionClass::SubagentSpawn,
            vec![],
        ),
        (
            "Skill",
            json!({ "skill": "pdf" }),
            ActionClass::Skill { name: "pdf".into() },
            vec![],
        ),
        (
            "mcp__rig__report",
            json!({}),
            ActionClass::Mcp {
                server: "rig".into(),
                tool: "report".into(),
            },
            vec![],
        ),
        (
            "commandExecution",
            json!({ "command": "/bin/bash -lc 'cargo test'" }),
            ActionClass::ShellTestRun,
            vec![],
        ),
        (
            "fileChange",
            json!({ "changes": [{ "path": "/home/u/project/x.txt", "kind": "add" }] }),
            ActionClass::FileEdit,
            vec!["/home/u/project/x.txt"],
        ),
        ("TodoWrite", json!({}), ActionClass::Other, vec![]),
    ];
    for (tool, input, class, paths) in cases {
        let (c, p) = interpreter::classify(tool, &input);
        assert_eq!(c, class, "{tool}");
        assert_eq!(
            p,
            paths.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "{tool}"
        );
    }
}

#[test]
fn risky_commands_are_flagged_in_plain_words() {
    let cwd = Some(Path::new(CWD));
    let bash = |c: &str| json!({ "command": c });
    let f = interpreter::consequence("Bash", &bash("git push --force origin main"), cwd);
    assert_eq!(codes(&f), ["force-push"]);
    assert_eq!(f[0].text, "force-pushed to origin main");
    assert_eq!(
        interpreter::consequence("Bash", &bash("git push -f"), cwd)[0].text,
        "force-pushed to the default remote"
    );
    assert_eq!(
        codes(&interpreter::consequence(
            "Bash",
            &bash("git reset --hard HEAD~1"),
            cwd
        )),
        ["discard-changes"]
    );
    assert_eq!(
        codes(&interpreter::consequence(
            "Bash",
            &bash("git clean -fdx"),
            cwd
        )),
        ["discard-changes"]
    );
    let rm = interpreter::consequence("Bash", &bash("rm -rf /tmp/build ../other"), cwd);
    assert_eq!(
        codes(&rm),
        ["outside-folder-delete", "outside-folder-delete"]
    );
    assert_eq!(
        rm[0].text,
        "deleted /tmp/build and everything in it, outside the agent's folder"
    );
    assert!(
        interpreter::consequence("Bash", &bash("rm -rf build target/debug"), cwd).is_empty(),
        "inside the folder"
    );
    assert_eq!(
        codes(&interpreter::consequence(
            "Bash",
            &bash("sudo apt-get install -y jq"),
            cwd
        )),
        ["sudo"]
    );
    let post = interpreter::consequence(
        "Bash",
        &bash("curl -X POST https://api.example.com/v1/x -d '{}'"),
        cwd,
    );
    assert_eq!(
        (post[0].code.as_str(), post[0].text.as_str()),
        ("network-write", "network write to api.example.com")
    );
    assert!(
        interpreter::consequence("Bash", &bash("curl -s https://example.com"), cwd).is_empty(),
        "a read is not a write"
    );
    assert_eq!(
        interpreter::consequence("Bash", &bash("rsync -a dist/ deploy@web1:/srv/site"), cwd)[0]
            .text,
        "copied files to web1"
    );
    assert_eq!(
        interpreter::consequence("Bash", &bash("chmod -R 777 ."), cwd)[0].text,
        "made . writable by everyone, recursively"
    );
    let edit = interpreter::consequence("Edit", &json!({ "file_path": "/etc/hosts" }), cwd);
    assert_eq!(
        (edit[0].code.as_str(), edit[0].text.as_str()),
        (
            "outside-folder-edit",
            "edited /etc/hosts, outside the agent's folder"
        )
    );
    assert!(interpreter::consequence("Edit", &json!({ "file_path": "src/a.rs" }), cwd).is_empty());
    assert_eq!(
        codes(&interpreter::consequence(
            "commandExecution",
            &bash("/bin/bash -lc 'git push --force'"),
            cwd
        )),
        ["force-push"]
    );
}

#[test]
fn a_failed_call_has_no_consequence_and_a_forced_update_says_so() {
    let cwd = Some(Path::new(CWD));
    let input = json!({ "command": "git push --force origin main" });
    assert!(interpreter::risks("Bash", &input, "rejected", false, cwd).is_empty());
    let r = interpreter::risks(
        "Bash",
        &input,
        " + 1a2b3c...4d5e6f main -> main (forced update)",
        true,
        cwd,
    );
    assert_eq!(
        r[0].text,
        "force-pushed to origin main: the remote branch was rewritten"
    );
}

#[test]
fn outside_is_lexical_and_unknown_folders_flag_nothing() {
    let cwd = Some(Path::new(CWD));
    assert!(!interpreter::outside("src/../lib.rs", cwd));
    assert!(interpreter::outside("../x", cwd));
    assert!(interpreter::outside("~/notes", cwd));
    assert!(interpreter::outside("$HOME/notes", cwd));
    assert!(
        interpreter::outside("/home/u/project-other/a", cwd),
        "a sibling with the same prefix is outside"
    );
    assert!(!interpreter::outside("/home/u/project/./a", cwd));
    assert!(
        !interpreter::outside("/etc/hosts", None),
        "without the agent's folder nothing is called outside"
    );
}

#[test]
fn a_cd_earlier_in_the_command_moves_where_a_delete_lands() {
    let cwd = Some(Path::new(CWD));
    let texts = |c: &str| {
        interpreter::consequence("Bash", &json!({ "command": c }), cwd)
            .into_iter()
            .map(|f| f.text)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        texts("cd /tmp && rm -rf build"),
        ["deleted /tmp/build and everything in it, outside the agent's folder"]
    );
    assert_eq!(
        texts("cd .. && rm -rf other"),
        ["deleted ../other and everything in it, outside the agent's folder"]
    );
    assert_eq!(
        texts("cd && rm -rf x"),
        ["deleted ~/x and everything in it, outside the agent's folder"]
    );
    assert!(
        texts("cd web && rm -rf dist").is_empty(),
        "still inside the folder"
    );
    assert!(
        texts("cd /tmp; cd /home/u/project && rm -rf build").is_empty(),
        "back inside the folder"
    );
    assert!(
        texts("cd \"$OUT\" && rm -rf build").is_empty(),
        "an unknown folder is not called outside"
    );
    assert!(
        texts("cd - && rm -rf build").is_empty(),
        "the previous folder is unknown"
    );
    assert_eq!(
        codes(&interpreter::consequence(
            "commandExecution",
            &json!({ "command": "/bin/bash -lc 'cd /srv && rm -r site'" }),
            cwd
        )),
        ["outside-folder-delete"]
    );
}

#[test]
fn a_long_flag_keeps_the_start_and_end_of_what_it_names() {
    let cwd = Some(Path::new(CWD));
    let target = format!("/tmp/{}/end-of-path", "a".repeat(5000));
    let f = interpreter::consequence(
        "Bash",
        &json!({ "command": format!("rm -rf {target}") }),
        cwd,
    );
    assert_eq!(codes(&f), ["outside-folder-delete"]);
    assert!(f[0].text.len() <= 1200, "{}", f[0].text.len());
    assert!(
        f[0].text.starts_with("deleted /tmp/aaa"),
        "{}",
        &f[0].text[..40]
    );
    assert!(
        f[0].text
            .ends_with("/end-of-path and everything in it, outside the agent's folder")
    );
    assert!(
        f[0].text
            .contains("bytes not shown; the full command is in the tool call]")
    );
}
