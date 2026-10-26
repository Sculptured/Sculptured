use model::{
    AgentAdvanced, AgentKind, ConnectionKind, DraftAgent, DraftConnection, DraftGroup, DraftIssue,
    ElementKind, ElementRef, IssueSeverity, ModelDefaults, PermissionPosture, ResumeBehaviour,
    SoftHard, TeamDraft, TeamInput,
};

use rigspec::draft::{GENERIC_AGENT, GENERIC_GROUP, GENERIC_TEAM, RESTRICTED_POLICY};
use rigspec::{Diagnostic, EdgeKind, Runtime, Severity};

fn agent(id: &str, name: &str, kind: AgentKind, folder: Option<&str>) -> DraftAgent {
    DraftAgent {
        id: id.into(),
        group_id: None,
        name: name.into(),
        kind,
        job: String::new(),
        folder: folder.map(Into::into),
        x: 0,
        y: 0,
        advanced: AgentAdvanced::default(),
        connection: None,
    }
}

fn link(id: &str, from: &str, to: &str) -> DraftConnection {
    DraftConnection {
        id: id.into(),
        from: from.into(),
        to: to.into(),
        kind: ConnectionKind::HandsWorkTo,
    }
}

fn team(agents: Vec<DraftAgent>, connections: Vec<DraftConnection>) -> TeamDraft {
    TeamDraft {
        claude_setup: Default::default(),
        protection: Default::default(),
        safety: vec![],
        components: vec![],
        budget: Default::default(),
        input: None,
        schedules: vec![],
        decision_log: false,
        limits: Default::default(),
        workflow: None,
        folder: None,
        id: "my-team".into(),
        name: "My team".into(),
        groups: vec![],
        agents,
        connections,
        models: ModelDefaults::default(),
        revision: 1,
    }
}

fn pair() -> TeamDraft {
    team(
        vec![
            agent("builder", "Builder", AgentKind::Claude, Some("/tmp")),
            agent("reviewer", "Reviewer", AgentKind::Practice, None),
        ],
        vec![link("c1", "builder", "reviewer")],
    )
}

fn find<'a>(issues: &'a [DraftIssue], code: &str) -> &'a DraftIssue {
    issues
        .iter()
        .find(|i| i.code == code)
        .unwrap_or_else(|| panic!("no {code} in {issues:?}"))
}

#[test]
fn a_pair_compiles_with_ids_from_draft_ids_and_names_as_labels() {
    let b = rigspec::from_draft(&pair()).unwrap();
    let ids: Vec<_> = b.rig.seats.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["team.builder@my-team", "team.reviewer@my-team"]);
    assert_eq!(b.rig.seats[0].label.as_deref(), Some("Builder"));
    assert_eq!(b.rig.seats[0].runtime, Runtime::ClaudeCode);
    assert_eq!(b.rig.seats[0].cwd, "/tmp");
    assert_eq!(b.rig.seats[1].runtime, Runtime::Stub);
    assert_eq!(
        b.rig.seats[1].cwd, ".",
        "a Practice agent without a folder runs in the home folder"
    );
    assert_eq!(b.rig.edges[0].kind, EdgeKind::DelegatesTo);
    assert_eq!(
        b.rig.launch_layers,
        vec![
            vec!["team.builder@my-team".to_string()],
            vec!["team.reviewer@my-team".to_string()]
        ]
    );
    assert!(b.yaml.contains("name: my-team"), "{}", b.yaml);
    assert!(!b.yaml.contains("null"), "{}", b.yaml);
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
}

#[test]
fn renaming_keeps_seat_ids() {
    let mut d = pair();
    let before: Vec<String> = rigspec::from_draft(&d)
        .unwrap()
        .rig
        .seats
        .into_iter()
        .map(|s| s.id)
        .collect();
    d.name = "Renamed".into();
    d.agents[0].name = "Lead builder".into();
    let after = rigspec::from_draft(&d).unwrap();
    let ids: Vec<String> = after.rig.seats.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids, before);
    assert_eq!(after.rig.seats[0].label.as_deref(), Some("Lead builder"));
}

#[test]
fn groups_become_pods_and_cross_group_connections_are_top_level_edges() {
    let mut d = pair();
    d.groups = vec![
        DraftGroup {
            id: "build".into(),
            label: "Build".into(),
            x: 0,
            y: 0,
            w: 300,
            h: 300,
            models: ModelDefaults::default(),
        },
        DraftGroup {
            id: "review".into(),
            label: "Review".into(),
            x: 400,
            y: 0,
            w: 300,
            h: 300,
            models: ModelDefaults::default(),
        },
    ];
    d.agents[0].group_id = Some("build".into());
    d.agents[1].group_id = Some("review".into());
    let b = rigspec::from_draft(&d).unwrap();
    assert_eq!(
        b.spec
            .pods
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["build", "review"]
    );
    assert_eq!(b.spec.edges[0].from, "build.builder");
    assert_eq!(b.spec.edges[0].to, "review.reviewer");
    assert_eq!(b.rig.seats[1].id, "review.reviewer@my-team");
}

#[test]
fn a_missing_folder_is_pinned_to_the_agent_in_plain_words() {
    let mut d = pair();
    d.agents[1].kind = AgentKind::Claude;
    let issues = rigspec::from_draft(&d).unwrap_err();
    let i = find(&issues, "folder-missing");
    assert_eq!(i.message, "Reviewer needs a folder to work in");
    assert_eq!(
        i.element,
        ElementRef {
            kind: ElementKind::Agent,
            id: "reviewer".into()
        }
    );
    assert_eq!(i.severity, IssueSeverity::Error);
    d.agents[1].folder = Some("relative/path".into());
    assert_eq!(
        find(&rigspec::validate_draft(&d), "folder-not-absolute").message,
        "Reviewer's folder must be a full path, such as /home/you/project or C:\\Users\\you\\project"
    );
    // A connected Windows computer's folders are full paths too.
    for windows in [
        r"C:\Users\Ana\Documents",
        "D:/work/app",
        r"\\server\share\app",
    ] {
        d.agents[1].folder = Some(windows.into());
        assert!(
            !rigspec::validate_draft(&d)
                .iter()
                .any(|i| i.code == "folder-not-absolute"),
            "{windows} is a full path"
        );
    }
    for relative in [r"Documents\app", "C:", "C:relative"] {
        d.agents[1].folder = Some(relative.into());
        assert!(
            rigspec::validate_draft(&d)
                .iter()
                .any(|i| i.code == "folder-not-absolute"),
            "{relative} is not a full path"
        );
    }
}

#[test]
fn a_self_loop_says_so() {
    let mut d = pair();
    d.agents[0].name = "Planner".into();
    d.connections = vec![link("loop", "builder", "builder")];
    let i = find(&rigspec::validate_draft(&d), "connection-self").clone();
    assert_eq!(i.message, "Planner's connection loops back to itself");
    assert_eq!(
        i.element,
        ElementRef {
            kind: ElementKind::Connection,
            id: "loop".into()
        }
    );
    let issues = rigspec::validate_draft(&d);
    assert!(
        issues.iter().all(|i| i.code != "connection-loop"),
        "a self-loop is reported once, as connection-self, never as connection-loop: {issues:?}"
    );
}

#[test]
fn a_loop_nothing_hands_work_into_flags_every_connection_on_it() {
    let mut d = pair();
    d.connections.push(link("back", "reviewer", "builder"));
    let issues = rigspec::validate_draft(&d);
    let loops: Vec<_> = issues
        .iter()
        .filter(|i| i.code == "connection-loop")
        .collect();
    assert_eq!(loops.len(), 2, "{issues:?}");
    let ids: Vec<&str> = loops.iter().map(|i| i.element.id.as_str()).collect();
    assert_eq!(ids, ["c1", "back"], "each connection on the ring is pinned");
    assert!(
        loops.iter().all(
            |i| i.element.kind == ElementKind::Connection && i.severity == IssueSeverity::Error
        )
    );
    assert_eq!(
        loops[0].message,
        "Builder → Reviewer is part of a loop (Builder → Reviewer → Builder) that no other agent hands work into, so the work has no starting point; connect an agent outside the loop to it, or remove one connection"
    );
}

#[test]
fn a_loop_with_a_starting_point_is_fine() {
    let mut d = pair();
    d.agents.push(DraftAgent {
        id: "lead".into(),
        name: "Lead".into(),
        ..d.agents[0].clone()
    });
    d.connections.push(link("back", "reviewer", "builder"));
    d.connections.push(link("in", "lead", "builder"));
    let issues = rigspec::validate_draft(&d);
    assert!(
        issues.iter().all(|i| i.code != "connection-loop"),
        "{issues:?}"
    );
}

#[test]
fn names_connections_and_emptiness_are_checked() {
    let mut d = pair();
    d.agents[1].name = " builder ".into();
    d.connections.push(link("ghost", "builder", "nobody"));
    d.connections.push(link("c2", "builder", "reviewer"));
    let issues = rigspec::validate_draft(&d);
    assert_eq!(
        find(&issues, "agent-name-duplicate").message,
        "Two agents are called builder; give each its own name"
    );
    assert_eq!(find(&issues, "connection-dangling").element.id, "ghost");
    assert_eq!(
        find(&issues, "connection-duplicate").message,
        "Builder already hands work to builder; remove the extra connection"
    );
    assert_eq!(
        find(&issues, "connection-duplicate").element,
        ElementRef {
            kind: ElementKind::Connection,
            id: "c2".into()
        },
        "the extra (second) connection is flagged, not the first"
    );
    assert_eq!(
        issues
            .iter()
            .filter(|i| i.code == "connection-duplicate")
            .count(),
        1,
        "{issues:?}"
    );
    assert_eq!(
        find(&issues, "connection-dangling").message,
        "This connection points to an agent that is no longer in the team"
    );
    let mut nameless = pair();
    nameless.agents[1].name = "   ".into();
    let missing = find(&rigspec::validate_draft(&nameless), "agent-name-missing").clone();
    assert_eq!(missing.message, "An agent needs a name");
    assert_eq!(
        missing.element,
        ElementRef {
            kind: ElementKind::Agent,
            id: "reviewer".into()
        }
    );

    let empty = team(vec![], vec![]);
    assert_eq!(
        find(&rigspec::validate_draft(&empty), "team-empty").message,
        "Add at least one agent to start the team"
    );
    let mut unnamed = pair();
    unnamed.name = "  ".into();
    assert_eq!(
        find(&rigspec::validate_draft(&unnamed), "team-name-missing")
            .element
            .kind,
        ElementKind::Team
    );
    assert_eq!(
        find(&rigspec::validate_draft(&unnamed), "team-name-missing").message,
        "Give the team a name"
    );
}

#[test]
fn codex_compiles_without_a_warning() {
    let mut d = pair();
    d.agents[1].kind = AgentKind::Codex;
    d.agents[1].folder = Some("/tmp".into());
    let b = rigspec::from_draft(&d).unwrap();
    assert!(
        !b.warnings.iter().any(|w| w.code == "codex-not-available"),
        "{:?}",
        b.warnings
    );
    assert_eq!(b.rig.seats[1].runtime, rigspec::Runtime::Codex);
}

#[test]
fn restricted_maps_to_an_enforced_policy() {
    let mut d = pair();
    d.agents[0].advanced.permission = PermissionPosture::Restricted;
    d.agents[0].advanced.resume = ResumeBehaviour::RelaunchFresh;
    d.agents[0].advanced.model = Some("opus".into());
    let b = rigspec::from_draft(&d).unwrap();
    assert_eq!(
        b.rig.seats[0].permission_policy.as_deref(),
        Some(RESTRICTED_POLICY)
    );
    assert_eq!(b.rig.seats[0].model.as_deref(), Some("opus"));
    assert_eq!(
        b.rig.seats[0].restore_policy,
        rigspec::RestorePolicy::RelaunchFresh
    );
    assert!(
        b.warnings
            .iter()
            .all(|w| w.code != "restricted-not-enforced"),
        "{:?}",
        b.warnings
    );
}

#[test]
fn compiler_paths_map_back_to_draft_elements() {
    let mut d = team(
        vec![
            agent("a", "A", AgentKind::Practice, None),
            agent("b", "B", AgentKind::Practice, None),
            agent("c", "C", AgentKind::Practice, None),
        ],
        vec![link("c-ab", "a", "b"), link("c-bc", "b", "c")],
    );
    d.groups = vec![
        DraftGroup {
            id: "g1".into(),
            label: "One".into(),
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            models: ModelDefaults::default(),
        },
        DraftGroup {
            id: "g2".into(),
            label: "Two".into(),
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            models: ModelDefaults::default(),
        },
    ];
    d.agents[0].group_id = Some("g1".into());
    d.agents[1].group_id = Some("g2".into());
    d.agents[2].group_id = Some("g2".into());
    let diag = |path: &str| Diagnostic {
        path: path.into(),
        severity: Severity::Error,
        message: "compiler words".into(),
    };

    let i = rigspec::draft::map_diagnostic(&d, &diag("pods[1].members[1].runtime"));
    assert_eq!(
        i.element,
        ElementRef {
            kind: ElementKind::Agent,
            id: "c".into()
        }
    );
    assert_eq!(i.message, GENERIC_AGENT);
    assert_eq!(i.detail.as_deref(), Some("compiler words"));
    assert_eq!(i.spec_path.as_deref(), Some("pods[1].members[1].runtime"));
    assert_eq!(
        rigspec::draft::map_diagnostic(&d, &diag("pods[1].edges[0].kind"))
            .element
            .id,
        "c-bc"
    );
    assert_eq!(
        rigspec::draft::map_diagnostic(&d, &diag("edges[0].from"))
            .element
            .id,
        "c-ab"
    );
    assert_eq!(
        rigspec::draft::map_diagnostic(&d, &diag("pods[0].label")).element,
        ElementRef {
            kind: ElementKind::Group,
            id: "g1".into()
        }
    );
    let team_issue = rigspec::draft::map_diagnostic(&d, &diag("line 3:1"));
    assert_eq!(
        (team_issue.element.kind, team_issue.message.as_str()),
        (ElementKind::Team, GENERIC_TEAM)
    );
    assert_eq!(
        rigspec::draft::map_diagnostic(&d, &diag("pods[0].members[0].cwd")).message,
        "A needs a folder to work in"
    );
}

#[test]
fn slugs() {
    assert_eq!(rigspec::draft::slug("My Team!"), "my-team");
    assert_eq!(
        rigspec::draft::slug("  Planner → 2 Builders "),
        "planner-2-builders"
    );
    assert_eq!(rigspec::draft::slug("ÉÉ"), "");
    assert_eq!(rigspec::draft::slug(&"x".repeat(80)).len(), 48);
    let taken = ["my-team", "my-team-2"];
    assert_eq!(
        rigspec::draft::unique_slug("My team", "team", |c| taken.contains(&c)),
        "my-team-3"
    );
    assert_eq!(rigspec::draft::unique_slug("", "team", |_| false), "team");
}

/// Words that must never reach a user-facing `message` (only `detail` may carry them).
fn openrig_words(message: &str) -> Vec<String> {
    const WORDS: [&str; 15] = [
        "seat", "seats", "pod", "pods", "rig", "rigs", "rigspec", "edge", "edges", "member",
        "members", "runtime", "yaml", "profile", "null",
    ];
    let mut found: Vec<String> = message
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|w| WORDS.contains(&w.as_str()))
        .collect();
    if message.contains("agent_ref") {
        found.push("agent_ref".into());
    }
    found
}

#[test]
fn carried_checks_dangling_groups_and_bad_ids_are_plain_issues() {
    // Ledger carry-forward from Task 5: dangling group_id and id segment validity.
    let mut d = pair();
    d.id = "bad id".into();
    d.groups = vec![
        DraftGroup {
            id: "g.1".into(),
            label: "  ".into(),
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            models: ModelDefaults::default(),
        },
        DraftGroup {
            id: "dup".into(),
            label: "One".into(),
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            models: ModelDefaults::default(),
        },
        DraftGroup {
            id: "dup".into(),
            label: "Two".into(),
            x: 0,
            y: 0,
            w: 1,
            h: 1,
            models: ModelDefaults::default(),
        },
    ];
    d.agents[0].id = "a.b".into();
    d.connections.clear();
    d.agents[1].group_id = Some("gone".into());
    let issues = rigspec::validate_draft(&d);

    let team = find(&issues, "team-id-invalid");
    assert_eq!(
        (team.element.kind, team.message.as_str()),
        (ElementKind::Team, GENERIC_TEAM)
    );
    assert!(
        team.detail.is_some(),
        "the technical reason stays in detail"
    );
    let group = find(&issues, "group-id-invalid");
    assert_eq!(
        (group.element.kind, group.message.as_str()),
        (ElementKind::Group, GENERIC_GROUP)
    );
    assert_eq!(
        issues
            .iter()
            .filter(|i| i.code == "group-id-invalid")
            .count(),
        2,
        "{issues:?}"
    );
    assert_eq!(
        find(&issues, "group-name-missing").message,
        "A group needs a name"
    );
    let agent_id = find(&issues, "agent-id-invalid");
    assert_eq!(
        agent_id.element,
        ElementRef {
            kind: ElementKind::Agent,
            id: "a.b".into()
        }
    );
    assert_eq!(agent_id.message, GENERIC_AGENT);
    let gone = find(&issues, "agent-group-missing");
    assert_eq!(
        gone.element,
        ElementRef {
            kind: ElementKind::Agent,
            id: "reviewer".into()
        }
    );
    assert_eq!(gone.message, "Reviewer is in a group that no longer exists");
    assert!(
        issues.iter().all(|i| i.severity == IssueSeverity::Error),
        "{issues:?}"
    );
}

#[test]
fn no_message_uses_openrig_words() {
    let mut drafts = vec![pair(), team(vec![], vec![])];
    let mut bad = pair();
    bad.id = "bad id".into();
    bad.name = " ".into();
    bad.groups = vec![DraftGroup {
        id: "g.1".into(),
        label: "".into(),
        x: 0,
        y: 0,
        w: 1,
        h: 1,
        models: ModelDefaults::default(),
    }];
    bad.agents
        .push(agent("x.y", "", AgentKind::Codex, Some("rel")));
    bad.agents
        .push(agent("z", "Builder", AgentKind::Claude, None));
    bad.agents[1].group_id = Some("gone".into());
    bad.agents[0].advanced.permission = PermissionPosture::Restricted;
    bad.connections.push(link("self", "builder", "builder"));
    bad.connections.push(link("ghost", "builder", "nobody"));
    bad.connections.push(link("dup", "builder", "reviewer"));
    bad.connections.push(link("back", "reviewer", "builder"));
    drafts.push(bad);
    let mut warned = pair();
    warned.agents[0].advanced.permission = PermissionPosture::Restricted;
    warned.agents[1].kind = AgentKind::Codex;
    warned.agents[1].folder = Some("/tmp".into());
    drafts.push(warned);

    let mut messages: Vec<String> = drafts
        .iter()
        .flat_map(rigspec::validate_draft)
        .map(|i| i.message)
        .collect();
    let diag = |path: &str, severity| Diagnostic {
        path: path.into(),
        severity,
        message: "seat pod rig edge".into(),
    };
    for path in [
        "pods[0].members[0].runtime",
        "pods[0].members[0].cwd",
        "pods[0].edges[0].kind",
        "edges[0]",
        "pods[0].label",
        "line 1:1",
    ] {
        messages
            .push(rigspec::draft::map_diagnostic(&pair(), &diag(path, Severity::Error)).message);
    }
    messages.push(
        rigspec::draft::map_diagnostic(
            &pair(),
            &diag("pods[0].members[0].permission_policy", Severity::Warning),
        )
        .message,
    );
    assert_eq!(
        openrig_words("A seat in pods[0] via agent_ref"),
        ["seat", "pods", "agent_ref"],
        "the scan itself works"
    );
    assert!(messages.len() > 20, "{messages:?}");
    for m in &messages {
        assert!(
            openrig_words(m).is_empty(),
            "{m:?} uses {:?}",
            openrig_words(m)
        );
    }
}

#[test]
fn validation_and_compilation_are_deterministic() {
    let mut d = pair();
    d.agents[0].name = "".into();
    d.agents
        .push(agent("third", "Builder", AgentKind::Codex, Some("rel")));
    d.agents[1].group_id = Some("gone".into());
    d.connections.push(link("back", "reviewer", "builder"));
    d.connections.push(link("dup", "builder", "reviewer"));
    d.connections.push(link("ghost", "builder", "nobody"));
    d.connections.push(link("self", "third", "third"));
    let first = rigspec::validate_draft(&d);
    assert!(first.len() > 5, "{first:?}");
    assert_eq!(
        rigspec::validate_draft(&d),
        first,
        "same draft, same issues, same order"
    );
    assert_eq!(
        rigspec::validate_draft(&d.clone()),
        first,
        "a clone gives the same issues in the same order"
    );

    let mut ok = pair();
    ok.agents[0].advanced.permission = PermissionPosture::Restricted;
    ok.agents[1].kind = AgentKind::Codex;
    ok.agents[1].folder = Some("/tmp".into());
    ok.connections.push(DraftConnection {
        id: "w".into(),
        from: "reviewer".into(),
        to: "builder".into(),
        kind: ConnectionKind::Watches,
    });
    let a = rigspec::from_draft(&ok).unwrap();
    for b in [
        rigspec::from_draft(&ok).unwrap(),
        rigspec::from_draft(&ok.clone()).unwrap(),
    ] {
        assert_eq!(b.rig, a.rig, "identical compiled team");
        assert_eq!(b.yaml, a.yaml, "identical spec text");
        assert_eq!(
            b.warnings, a.warnings,
            "identical warnings in identical order"
        );
        assert_eq!(
            rigspec::draft::spec_yaml(&b.spec),
            rigspec::draft::spec_yaml(&a.spec)
        );
    }
    assert_eq!(rigspec::validate_draft(&ok), a.warnings);
}

#[test]
fn an_agent_runs_its_own_model_else_its_groups_else_the_teams() {
    let models = |claude: &str, codex: &str| ModelDefaults {
        claude: Some(claude.into()),
        codex: Some(codex.into()),
        opencode: None,
    };
    let mut d = team(
        vec![
            agent("a", "A", AgentKind::Claude, Some("/tmp")),
            agent("b", "B", AgentKind::Codex, Some("/tmp")),
            agent("c", "C", AgentKind::Claude, Some("/tmp")),
            agent("p", "P", AgentKind::Practice, None),
        ],
        vec![],
    );
    d.models = models("sonnet", "gpt-team");
    d.groups = vec![DraftGroup {
        id: "g".into(),
        label: "G".into(),
        x: 0,
        y: 0,
        w: 300,
        h: 300,
        models: models("opus", " "),
    }];
    for id in ["a", "b"] {
        d.agents.iter_mut().find(|x| x.id == id).unwrap().group_id = Some("g".into());
    }
    d.agents[2].advanced.model = Some("haiku".into());
    let model = |id: &str| d.effective_model(d.agent(id).unwrap());
    assert_eq!(model("a").as_deref(), Some("opus"), "group beats team");
    assert_eq!(
        model("b").as_deref(),
        Some("gpt-team"),
        "a blank group model inherits the team's"
    );
    assert_eq!(
        model("c").as_deref(),
        Some("haiku"),
        "the agent's own model wins"
    );
    assert_eq!(model("p"), None, "Practice agents inherit nothing");
    let spec = rigspec::from_draft(&d).unwrap().spec;
    let member = |id: &str| {
        spec.pods
            .iter()
            .flat_map(|p| &p.members)
            .find(|m| m.id == id)
            .unwrap()
            .model
            .clone()
    };
    assert_eq!(member("a").as_deref(), Some("opus"));
    assert_eq!(member("c").as_deref(), Some("haiku"));
}

#[test]
fn project_folder_is_inherited_without_overwriting_agent_overrides() {
    let mut d = team(
        vec![
            agent("a", "A", AgentKind::Codex, None),
            agent("b", "B", AgentKind::Claude, Some("/home/me/other")),
        ],
        vec![],
    );
    d.folder = Some("/home/me/project".into());
    let build = rigspec::from_draft(&d).unwrap();
    assert_eq!(build.rig.seats[0].cwd, "/home/me/project");
    assert_eq!(build.rig.seats[1].cwd, "/home/me/other");
    d.folder = Some("relative".into());
    assert!(
        rigspec::validate_draft(&d)
            .iter()
            .any(|i| i.element.id == "a" && i.code == "folder-not-absolute")
    );
    d.folder = None;
    assert!(
        rigspec::validate_draft(&d)
            .iter()
            .any(|i| i.element.id == "a" && i.code == "folder-missing")
    );
}

#[test]
fn textbook_template_keeps_source_open_and_groups_the_reusable_review_component() {
    let d = rigspec::textbook::template("book", "Book", true);
    let w = d.workflow.as_ref().unwrap();
    assert!(w.input.is_empty());
    assert_eq!(d.groups.len(), 2);
    assert!(d.agent(&w.dispatcher).unwrap().group_id.is_none());
    assert!(d.agent(&w.collector).unwrap().group_id.is_none());
    for stage in &w.stages {
        assert_eq!(
            d.agent(&stage.agent).unwrap().group_id.as_deref(),
            Some(if stage.id.starts_with("engine") {
                "webgpu-engine"
            } else {
                "mathematical-visualization"
            })
        );
    }
    assert_eq!(
        w.stages.iter().find(|s| s.id == "ui-check").unwrap().routes["revise"].as_deref(),
        Some("visualize")
    );
    assert!(rigspec::from_draft(&d).is_ok());
    let restored =
        rigspec::exchange::import_yaml(&rigspec::exchange::export_yaml(&d), |_| "book".into())
            .unwrap();
    assert_eq!(restored.draft.workflow, d.workflow);
    assert_eq!(restored.draft.groups, d.groups);
}

#[test]
fn an_input_that_points_to_no_agent_is_a_warning() {
    let mut d = pair();
    d.input = Some(TeamInput {
        text: "Goal".into(),
        x: 0,
        y: 0,
        to: vec![],
    });
    let i = find(&rigspec::validate_draft(&d), "input-unconnected").clone();
    assert_eq!(i.severity, IssueSeverity::Warning);
    assert_eq!(
        i.message,
        "The Input isn't connected to an agent, so no agent gets it; drag from the Input to an agent"
    );
    d.input.as_mut().unwrap().to = vec!["builder".into()];
    assert!(
        rigspec::validate_draft(&d)
            .iter()
            .all(|i| i.code != "input-unconnected")
    );
}

#[test]
fn a_budget_with_soft_above_hard_is_an_error_on_the_team_or_the_agent() {
    let mut d = pair();
    assert!(
        rigspec::validate_draft(&d)
            .iter()
            .all(|i| i.code != "budget-invalid")
    );
    d.budget.minutes = Some(SoftHard {
        soft: Some(90),
        hard: 60,
    });
    d.agents[0].advanced.turn_limit = Some(SoftHard {
        soft: None,
        hard: 0,
    });
    let issues = rigspec::validate_draft(&d);
    let bad: Vec<_> = issues
        .iter()
        .filter(|i| i.code == "budget-invalid")
        .map(|i| (i.element.kind, i.message.as_str()))
        .collect();
    assert_eq!(
        bad,
        [
            (
                ElementKind::Team,
                "Time budget: the soft value is above the hard value"
            ),
            (
                ElementKind::Agent,
                "Turn limit: the hard value must be above 0"
            )
        ]
    );
    assert!(rigspec::from_draft(&d).is_err(), "it blocks Start");
}

#[test]
fn an_unknown_or_clashing_component_is_named() {
    let mut a = agent("a", "Writer", AgentKind::Claude, Some("/home/u/p"));
    a.advanced.integrations = vec!["kindex".into(), "nope".into()];
    let mut d = team(vec![a], vec![]);
    let codes = |d: &TeamDraft| {
        rigspec::draft::check_draft(d)
            .into_iter()
            .map(|i| i.code)
            .collect::<Vec<_>>()
    };
    assert!(
        codes(&d).contains(&"component-unknown".to_string()),
        "{:?}",
        codes(&d)
    );
    d.agents[0].advanced.integrations = vec!["kindex".into()];
    assert!(
        !codes(&d).iter().any(|c| c.starts_with("component")),
        "a built-in component is known"
    );
    d.components = vec![model::component::builtins()[0].clone()];
    assert!(
        codes(&d).contains(&"component-invalid".to_string()),
        "a team's own component cannot reuse a built-in id"
    );
}

/// A Claude agent's model connection reaches its compiled seat, so the server knows it at launch.
/// Other kinds ignore it, and an agent without one compiles as before.
#[test]
fn a_claude_agents_model_connection_reaches_its_compiled_seat_and_other_kinds_ignore_it() {
    let mut d = pair();
    d.agents[0].connection = Some("conn-1".into());
    d.agents[1].connection = Some("conn-2".into());
    let b = rigspec::from_draft(&d).unwrap();
    let seat = |id: &str| b.rig.seats.iter().find(|s| s.member == id).unwrap().clone();
    assert_eq!(seat("builder").connection.as_deref(), Some("conn-1"));
    assert_eq!(
        seat("reviewer").connection,
        None,
        "a Practice agent has no model connection"
    );
    let plain = rigspec::from_draft(&pair()).unwrap();
    assert!(plain.rig.seats.iter().all(|s| s.connection.is_none()));
    assert!(!plain.yaml.contains("connection"), "{}", plain.yaml);
}
