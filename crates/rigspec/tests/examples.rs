//! The six example teams: three Claude teams and their Codex copies. Each compiles with no issue,
//! and each `-codex` copy is its original on Codex, with its own ports and run folder. Nothing here
//! starts a team: they are the user's real teams and spend the user's money.
use model::{AgentKind, TeamDraft};
use std::collections::BTreeSet;
use std::path::PathBuf;

const TEAMS: [&str; 3] = ["airplane-c172", "live-textbook", "math-workbench"];

fn load(name: &str) -> TeamDraft {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/teams")
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Every number from 8700 to 8999 named in a Job: the agents' own HTTP ports.
fn ports(d: &TeamDraft) -> BTreeSet<u32> {
    d.agents
        .iter()
        .flat_map(|a| {
            a.job
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|w| w.parse::<u32>().ok())
                .collect::<Vec<_>>()
        })
        .filter(|p| (8700..9000).contains(p))
        .collect()
}

/// The `<name>` of every `runs/<name>/` named in a Job.
fn run_folders(d: &TeamDraft) -> BTreeSet<String> {
    d.agents
        .iter()
        .flat_map(|a| {
            a.job
                .match_indices("runs/")
                .filter_map(|(i, _)| a.job[i + 5..].split_once('/').map(|(n, _)| n.to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn every_example_team_compiles_without_an_issue() {
    let flattened_teams = TEAMS
        .iter()
        .flat_map(|t| [t.to_string(), format!("{t}-codex")]);
    for name in flattened_teams {
        let d = load(&name);
        assert_eq!(d.id, name, "a file is named after its team id");
        let issues = rigspec::validate_draft(&d);
        assert!(issues.is_empty(), "{name}: {issues:?}");
    }
}

#[test]
fn each_codex_copy_is_its_original_on_codex() {
    for t in TEAMS {
        let (orig, copy) = (load(t), load(&format!("{t}-codex")));
        assert_eq!(copy.name, format!("{} (Codex)", orig.name));
        assert_eq!(copy.revision, orig.revision);
        assert_eq!(
            serde_json::to_value(&copy.groups).unwrap(),
            serde_json::to_value(&orig.groups).unwrap(),
            "{t}: groups"
        );
        assert_eq!(
            serde_json::to_value(&copy.connections).unwrap(),
            serde_json::to_value(&orig.connections).unwrap(),
            "{t}: connections"
        );
        assert_eq!(copy.agents.len(), orig.agents.len(), "{t}");
        let paired_agents = copy.agents.iter().zip(&orig.agents);
        for (c, o) in paired_agents {
            assert_eq!(
                (&c.id, &c.group_id, &c.name, &c.folder, c.x, c.y),
                (&o.id, &o.group_id, &o.name, &o.folder, o.x, o.y),
                "{t}"
            );
            assert_eq!(
                serde_json::to_value(&c.advanced).unwrap(),
                serde_json::to_value(&o.advanced).unwrap(),
                "{t}/{}",
                c.id
            );
            assert_eq!(
                o.kind,
                AgentKind::Claude,
                "{t}/{}: the original is the Claude team",
                o.id
            );
            assert_eq!(c.kind, AgentKind::Codex, "{t}/{}", c.id);
            assert!(
                c.job.contains("Each agent is a separate Codex session"),
                "{t}/{}",
                c.id
            );
            for word in ["Claude Code", "Task tool", "subagent"] {
                assert!(!c.job.contains(word), "{t}/{}: {word}", c.id);
            }
        }
        let (mine, theirs) = (ports(&copy), ports(&orig));
        assert_eq!(
            mine,
            theirs.iter().map(|p| p + 100).collect::<BTreeSet<_>>(),
            "{t}: every port moves by 100"
        );
        assert!(
            mine.is_disjoint(&theirs),
            "{t}: no port shared with the Claude team"
        );
        let folders = run_folders(&orig)
            .iter()
            .map(|f| format!("{f}-codex"))
            .collect::<BTreeSet<_>>();
        assert_eq!(run_folders(&copy), folders, "{t}: its own run folder");
    }
}
