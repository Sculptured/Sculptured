use model::{
    AgentAdvanced, AgentKind, ConnectionKind, DEFAULT_GROUP_ID, DraftAgent, DraftConnection,
    DraftGroup, IssueSeverity, ModelDefaults, PermissionPosture, ResumeBehaviour, TeamDraft,
};

use rigspec::exchange::{Imported, META_PREFIX, NOT_IMPORTED};

use rigspec::Severity;
use rigspec::templates::TEMPLATE_IDS;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn every_template_compiles_with_practice_agents() {
    let infos = rigspec::templates::templates();
    assert_eq!(
        infos.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        TEMPLATE_IDS
    );
    for id in TEMPLATE_IDS {
        let d = rigspec::templates::template(id, "t", "T", true).unwrap();
        assert_eq!(d.revision, 0);
        assert!(
            d.agents.iter().all(|a| a.kind == AgentKind::Practice),
            "{id}"
        );
        match id {
            "blank" => assert!(
                rigspec::validate_draft(&d)
                    .iter()
                    .any(|i| i.code == "team-empty")
            ),
            _ => {
                let b = rigspec::from_draft(&d).unwrap_or_else(|e| panic!("{id}: {e:?}"));
                assert_eq!(b.rig.seats.len(), d.agents.len(), "{id}");
            }
        }
    }
    assert!(rigspec::templates::template("nope", "t", "T", false).is_none());
}

#[test]
fn templates_have_the_spec_shapes_and_need_only_folders_without_practice() {
    let solo = rigspec::templates::template("solo", "t", "T", false).unwrap();
    assert_eq!(
        (solo.agents.len(), solo.agents[0].kind),
        (1, AgentKind::Claude)
    );
    let br = rigspec::templates::template("builder-reviewer", "t", "T", false).unwrap();
    assert_eq!(
        br.agents.iter().map(|a| a.kind).collect::<Vec<_>>(),
        [AgentKind::Claude, AgentKind::Codex]
    );
    assert_eq!(
        (
            br.connections[0].from.as_str(),
            br.connections[0].to.as_str()
        ),
        ("builder", "reviewer")
    );
    let pb = rigspec::templates::template("planner-builders", "t", "T", false).unwrap();
    assert_eq!(pb.agents.len(), 3);
    assert!(
        pb.connections
            .iter()
            .all(|c| c.from == "planner" && c.kind == ConnectionKind::HandsWorkTo)
    );
    for d in [solo, br, pb] {
        let codes: Vec<String> = rigspec::validate_draft(&d)
            .into_iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .map(|i| i.code)
            .collect();
        assert!(codes.iter().all(|c| c == "folder-missing"), "{codes:?}");
    }
}

/// Imports `yaml` as team "imported"; also returns the spec name `id_for` was given.
fn import_as_imported(yaml: &str) -> (Imported, String) {
    let mut seen = String::new();
    let back = rigspec::exchange::import_yaml(yaml, |name| {
        seen = name.to_string();
        "imported".to_string()
    })
    .unwrap();
    (back, seen)
}

#[test]
fn export_then_import_round_trips_names_jobs_and_positions() {
    let mut d =
        rigspec::templates::template("builder-reviewer", "my-team", "My team", true).unwrap();
    d.groups = vec![group("build", "Build", 10), group("check", "Check", 300)];
    d.agents[0].group_id = Some("build".into());
    d.agents[1].group_id = Some("check".into());
    d.agents[1].folder = Some("/srv/repo".into());
    d.agents[1].x = 333;
    d.agents[0].advanced.permission = PermissionPosture::Restricted;
    let yaml = rigspec::exchange::export_yaml(&d);
    assert!(
        yaml.lines().last().unwrap().starts_with(META_PREFIX),
        "{yaml}"
    );
    assert!(yaml.contains("label: Builder"));

    let (back, seen) = import_as_imported(&yaml);
    assert_eq!(seen, "my-team");
    let b = back.draft;
    assert_eq!(b.id, "imported");
    assert_eq!(b.name, "My team");
    assert_eq!(
        b.agents
            .iter()
            .map(|a| (a.id.as_str(), a.name.as_str()))
            .collect::<Vec<_>>(),
        [("builder", "Builder"), ("reviewer", "Reviewer")]
    );
    assert_eq!(b.agents[0].job, d.agents[0].job);
    assert_eq!((b.agents[1].x, b.agents[1].y), (333, d.agents[1].y));
    assert_eq!(b.agents[1].folder.as_deref(), Some("/srv/repo"));
    assert_eq!(
        b.agents[0].folder, None,
        "a relative cwd is not a chosen folder"
    );
    assert_eq!(
        b.agents[0].advanced.permission,
        PermissionPosture::Restricted
    );
    assert_eq!(b.groups, d.groups);
    assert_eq!(b.connections.len(), 1);
    assert_eq!(b.connections[0].kind, ConnectionKind::HandsWorkTo);
    assert!(back.dropped.is_empty(), "{:?}", back.dropped);
}

#[test]
fn an_openrig_spec_imports_with_a_layout_computed_once_and_lists_what_was_dropped() {
    let imported =
        rigspec::exchange::import_yaml(&fixture("implementation-pair"), |_| "pair".to_string())
            .unwrap();
    let d = &imported.draft;
    assert_eq!(d.name, "implementation-pair");
    assert_eq!(
        d.agents
            .iter()
            .map(|a| (a.id.as_str(), a.kind))
            .collect::<Vec<_>>(),
        [("impl", AgentKind::Claude), ("qa", AgentKind::Codex)]
    );
    assert!(
        d.agents[0].x < d.agents[1].x,
        "launch order goes left to right"
    );
    assert_eq!(d.groups.len(), 1);
    assert_eq!(d.groups[0].label, "Development Pair");
    let g = &d.groups[0];
    for a in &d.agents {
        assert!(
            a.x >= g.x && a.x + 200 <= g.x + g.w && a.y >= g.y && a.y + 200 <= g.y + g.h,
            "{a:?} inside {g:?}"
        );
    }
    let paths: Vec<&str> = imported.dropped.iter().map(|x| x.path.as_str()).collect();
    assert!(paths.contains(&"summary"), "{paths:?}");
    assert!(paths.contains(&"pods[0].members[0].agent_ref"), "{paths:?}");
    assert!(
        imported
            .dropped
            .iter()
            .all(|x| x.severity == Severity::Warning && x.message == NOT_IMPORTED)
    );
}

#[test]
fn invalid_or_unsupported_specs_return_spec_paths() {
    let bad = fixture("implementation-pair").replace("runtime: codex", "runtime: gpt");
    let errs = rigspec::exchange::import_yaml(&bad, |_| "x".into()).unwrap_err();
    assert!(
        errs.iter().any(|e| e.path == "pods[0].members[1].runtime"),
        "{errs:?}"
    );
    let terminal = fixture("implementation-pair").replace("runtime: codex", "runtime: terminal");
    let errs = rigspec::exchange::import_yaml(&terminal, |_| "x".into()).unwrap_err();
    assert!(
        errs.iter()
            .any(|e| e.path == "pods[0].members[1].runtime" && e.message.contains("terminal")),
        "{errs:?}"
    );
}

#[test]
fn a_broken_layout_comment_falls_back_to_the_computed_layout() {
    let yaml = format!(
        "{}{META_PREFIX}{{not json\n",
        fixture("implementation-pair")
    );
    let d = rigspec::exchange::import_yaml(&yaml, |_| "x".into())
        .unwrap()
        .draft;
    assert!(d.agents[0].x < d.agents[1].x);
}

// ---- Ledger carry-forwards from the Task 6 review ----

fn group(id: &str, label: &str, x: i32) -> DraftGroup {
    DraftGroup {
        id: id.into(),
        label: label.into(),
        x,
        y: 20,
        w: 260,
        h: 260,
        models: ModelDefaults::default(),
    }
}

/// A Practice agent "Helper" at (40, 300) with no folder.
fn helper(group_id: Option<&str>, advanced: AgentAdvanced) -> DraftAgent {
    DraftAgent {
        id: "helper".into(),
        group_id: group_id.map(Into::into),
        name: "Helper".into(),
        kind: AgentKind::Practice,
        job: String::new(),
        folder: None,
        x: 40,
        y: 300,
        advanced,
    }
}

fn round_trip(d: &TeamDraft) -> Imported {
    rigspec::exchange::import_yaml(&rigspec::exchange::export_yaml(d), |_| d.id.clone()).unwrap()
}

#[test]
fn export_never_invents_a_folder_and_import_maps_a_blank_one_back_to_none() {
    let mut d = rigspec::templates::template("builder-reviewer", "t", "T", false).unwrap();
    d.agents[1].folder = Some("/srv/repo".into());
    d.agents.push(helper(None, AgentAdvanced::default()));
    let yaml = rigspec::exchange::export_yaml(&d);
    let spec = rigspec::parse(&yaml).unwrap();
    let cwd = |id: &str| {
        spec.pods[0]
            .members
            .iter()
            .find(|m| m.id == id)
            .unwrap()
            .cwd
            .clone()
    };
    assert_eq!(
        cwd("builder"),
        "",
        "a Claude agent with no folder gets no made-up folder:\n{yaml}"
    );
    assert_eq!(cwd("reviewer"), "/srv/repo");
    assert_eq!(
        cwd("helper"),
        ".",
        "a Practice agent runs in the home folder, as when started"
    );
    let errs = rigspec::compile(&yaml).unwrap_err();
    assert!(
        errs.iter().any(|e| e.path == "pods[0].members[0].cwd"),
        "the exported file cannot run in an invented folder: {errs:?}"
    );

    let back = round_trip(&d);
    assert!(back.dropped.is_empty(), "{:?}", back.dropped);
    let b = back.draft;
    assert_eq!(
        b.agents
            .iter()
            .map(|a| a.folder.as_deref())
            .collect::<Vec<_>>(),
        [None, Some("/srv/repo"), None]
    );
    let codes = |d: &TeamDraft| {
        let mut v: Vec<(String, String)> = rigspec::validate_draft(d)
            .into_iter()
            .map(|i| (i.element.id, i.code))
            .collect();
        v.sort();
        v
    };
    assert_eq!(
        codes(&b),
        codes(&d),
        "the imported draft has the same problems as the exported one"
    );
    assert!(codes(&b).contains(&("builder".to_string(), "folder-missing".to_string())));
}

#[test]
fn a_blank_folder_in_any_spec_imports_as_no_folder() {
    let yaml = fixture("implementation-pair").replacen("cwd: \".\"", "cwd: \"\"", 1);
    let imported = rigspec::exchange::import_yaml(&yaml, |_| "x".into()).unwrap();
    assert_eq!(imported.draft.agents[0].folder, None);
    assert!(
        imported.dropped.iter().all(|x| !x.path.ends_with(".cwd")),
        "{:?}",
        imported.dropped
    );
}

#[test]
fn a_relative_folder_is_reported_as_not_imported() {
    let yaml = fixture("implementation-pair").replacen("cwd: \".\"", "cwd: \"../app\"", 1);
    let imported = rigspec::exchange::import_yaml(&yaml, |_| "x".into()).unwrap();
    assert_eq!(imported.draft.agents[0].folder, None);
    let cwd: Vec<&rigspec::Diagnostic> = imported
        .dropped
        .iter()
        .filter(|x| x.path.ends_with(".cwd"))
        .collect();
    assert_eq!(cwd.len(), 1, "{:?}", imported.dropped);
    assert_eq!(
        (
            cwd[0].path.as_str(),
            cwd[0].severity,
            cwd[0].message.as_str()
        ),
        ("pods[0].members[0].cwd", Severity::Warning, NOT_IMPORTED)
    );
}

#[test]
fn empty_drawn_groups_keep_their_names_order_and_boxes() {
    let mut d = rigspec::templates::template("builder-reviewer", "t", "T", true).unwrap();
    d.groups = vec![
        group("ideas", "Ideas for later", 600),
        group("build", "Build", 10),
        group("spare", "Spare", 900),
    ];
    d.agents[0].group_id = Some("build".into());
    d.agents[1].group_id = Some("build".into());
    let yaml = rigspec::exchange::export_yaml(&d);
    assert_eq!(
        rigspec::parse(&yaml).unwrap().pods.len(),
        1,
        "the spec holds only the group with agents"
    );
    let back = round_trip(&d);
    assert!(back.dropped.is_empty(), "{:?}", back.dropped);
    assert_eq!(back.draft.groups, d.groups);
    assert_eq!(back.draft.agents, d.agents);
}

#[test]
fn ungrouped_agents_and_a_drawn_main_group_round_trip_unchanged() {
    for id in ["solo", "builder-reviewer", "planner-builders"] {
        let d = rigspec::templates::template(id, "t", "T", true).unwrap();
        let back = round_trip(&d);
        assert!(back.dropped.is_empty(), "{id}: {:?}", back.dropped);
        assert_eq!(
            back.draft.groups, d.groups,
            "{id}: no zero-size group appears"
        );
        assert_eq!(back.draft.agents, d.agents, "{id}");
        assert_eq!(back.draft.connections.len(), d.connections.len(), "{id}");
    }
    // The drawn "Main group" keeps the default group id (`team`).
    let mut d = rigspec::templates::template("builder-reviewer", "t", "T", true).unwrap();
    d.groups = vec![group(DEFAULT_GROUP_ID, "Main group", 10)];
    d.agents[1].group_id = Some(DEFAULT_GROUP_ID.into());
    let b = round_trip(&d).draft;
    assert_eq!(b.groups, d.groups);
    assert_eq!(
        b.agents.iter().map(|a| d.pod_of(a)).collect::<Vec<_>>(),
        [DEFAULT_GROUP_ID, DEFAULT_GROUP_ID]
    );
}

#[test]
fn an_unreadable_layout_comment_is_reported() {
    let yaml = format!(
        "{}{META_PREFIX}{{not json\n",
        fixture("implementation-pair")
    );
    let dropped = rigspec::exchange::import_yaml(&yaml, |_| "x".into())
        .unwrap()
        .dropped;
    let layout: Vec<&rigspec::Diagnostic> = dropped.iter().filter(|x| x.path == "layout").collect();
    assert_eq!(layout.len(), 1, "{dropped:?}");
    assert_eq!(layout[0].severity, Severity::Warning);
    let clean = rigspec::exchange::import_yaml(&fixture("implementation-pair"), |_| "x".into())
        .unwrap()
        .dropped;
    assert!(clean.iter().all(|x| x.path != "layout"), "{clean:?}");
}

// ---- Task 7 review, fix round 1 ----

/// Characters the YAML parser reads as line breaks inside a comment.
const YAML_BREAKS: [char; 3] = ['\u{2028}', '\u{2029}', '\u{85}'];

fn top_level_keys(yaml: &str) -> Vec<String> {
    let v: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).unwrap();
    v.as_mapping()
        .unwrap()
        .keys()
        .map(|k| k.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn text_in_the_layout_comment_cannot_add_spec_keys() {
    for c in YAML_BREAKS {
        let job = format!("hello{c}summary: injected #");
        let name = format!("Team 🙂{c}startup: {{run: \"rm -rf ~\"}} #");
        for (what, job, name) in [
            ("job", job.clone(), "My team".to_string()),
            ("name", String::new(), name.clone()),
        ] {
            let mut d = rigspec::templates::template("solo", "t", "T", true).unwrap();
            d.agents[0].job = job.clone();
            d.name = name.clone();
            let yaml = rigspec::exchange::export_yaml(&d);
            assert!(
                yaml.lines().last().unwrap().is_ascii(),
                "the comment line is ASCII: {yaml}"
            );
            let spec =
                rigspec::parse(&yaml).unwrap_or_else(|e| panic!("{c:?} in {what}: {e:?}\n{yaml}"));
            assert!(
                spec.summary.is_none() && spec.startup.is_none(),
                "{c:?} in {what} injected a key:\n{yaml}"
            );
            for k in top_level_keys(&yaml) {
                assert!(
                    ["version", "name", "pods", "edges"].contains(&k.as_str()),
                    "{c:?} in {what} injected {k:?}"
                );
            }
            let back = rigspec::exchange::import_yaml(&yaml, |_| "t".into()).unwrap();
            assert!(
                back.dropped.is_empty(),
                "{c:?} in {what}: {:?}",
                back.dropped
            );
            assert_eq!(back.draft.agents[0].job, job, "{c:?} in {what}");
            assert_eq!(back.draft.name, name.trim(), "{c:?} in {what}");
        }
    }
}

#[test]
fn connection_ends_models_and_resume_survive_a_round_trip() {
    let mut d = rigspec::templates::template("builder-reviewer", "t", "T", true).unwrap();
    d.groups = vec![group("build", "Build", 10), group("check", "Check", 300)];
    d.agents[0].group_id = Some("build".into());
    d.agents[1].group_id = Some("check".into());
    let opus = AgentAdvanced {
        data_access: None,
        authority: None,
        skills: vec![],
        integrations: vec![],
        turn_limit: None,
        minute_limit: None,
        model: Some("opus".into()),
        permission: PermissionPosture::Unrestricted,
        resume: ResumeBehaviour::RelaunchFresh,
        token_limit: None,
        check: None,
        briefing: None,
    };
    d.agents.push(helper(Some("build"), opus));
    d.agents[1].advanced.resume = ResumeBehaviour::CheckpointOnly;
    d.connections.push(DraftConnection {
        id: "c2".into(),
        from: "helper".into(),
        to: "builder".into(),
        kind: ConnectionKind::ReportsTo,
    });
    d.connections.push(DraftConnection {
        id: "c3".into(),
        from: "reviewer".into(),
        to: "helper".into(),
        kind: ConnectionKind::Watches,
    });
    let b = round_trip(&d).draft;
    let ends = |d: &TeamDraft| {
        let mut v: Vec<(String, String, ConnectionKind)> = d
            .connections
            .iter()
            .map(|c| (c.from.clone(), c.to.clone(), c.kind))
            .collect();
        v.sort_by(|x, y| (&x.0, &x.1).cmp(&(&y.0, &y.1)));
        v
    };
    assert_eq!(
        ends(&b),
        ends(&d),
        "builder → reviewer and reviewer → helper cross groups"
    );
    // Import lists agents group by group, so compare by id.
    let advanced = |d: &TeamDraft| {
        let mut v: Vec<(String, AgentAdvanced)> = d
            .agents
            .iter()
            .map(|a| (a.id.clone(), a.advanced.clone()))
            .collect();
        v.sort_by(|x, y| x.0.cmp(&y.0));
        v
    };
    assert_eq!(advanced(&b), advanced(&d));
    assert_eq!(
        b.agent("helper").unwrap().advanced.model.as_deref(),
        Some("opus")
    );
}

/// Rewrites the export's layout comment with `edit`.
fn edit_comment(yaml: &str, edit: impl FnOnce(&mut serde_json::Value)) -> String {
    let (spec, json) = yaml.rsplit_once(META_PREFIX).unwrap();
    let mut meta: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    edit(&mut meta);
    format!(
        "{spec}{META_PREFIX}{}\n",
        serde_json::to_string(&meta).unwrap()
    )
}

fn inside(a: &DraftAgent, g: &DraftGroup) -> bool {
    a.x >= g.x && a.x + 200 <= g.x + g.w && a.y >= g.y && a.y + 200 <= g.y + g.h
}

#[test]
fn a_comment_that_misses_a_group_with_agents_is_laid_out_again() {
    let mut d = rigspec::templates::template("builder-reviewer", "t", "T", true).unwrap();
    d.groups = vec![group("build", "Build", 10)];
    d.agents[0].group_id = Some("build".into());
    d.agents[1].group_id = Some("build".into());
    let yaml = edit_comment(&rigspec::exchange::export_yaml(&d), |m| {
        m["groups"] = serde_json::json!([])
    });
    let b = rigspec::exchange::import_yaml(&yaml, |_| "t".into())
        .unwrap()
        .draft;
    assert_eq!(b.groups.len(), 1);
    let g = &b.groups[0];
    assert!(g.w > 0 && g.h > 0, "no zero-size box: {g:?}");
    assert!(
        b.agents.iter().all(|a| inside(a, g)),
        "{:?} inside {g:?}",
        b.agents
    );
}

#[test]
fn bad_groups_in_the_layout_comment_are_reported_not_drawn() {
    let mut d = rigspec::templates::template("solo", "t", "T", true).unwrap();
    d.groups = vec![
        group("good", "Good", 10),
        group("wide", "Wide", 300),
        group("tall", "Tall", 600),
    ];
    let yaml = edit_comment(&rigspec::exchange::export_yaml(&d), |m| {
        m["groups"][1]["w"] = serde_json::json!(-5);
        m["groups"][2]["h"] = serde_json::json!(-1);
        m["groups"].as_array_mut().unwrap().push(
            serde_json::json!({"id": "bad id!", "label": "Bad", "x": 0, "y": 0, "w": 10, "h": 10}),
        );
    });
    let back = rigspec::exchange::import_yaml(&yaml, |_| "t".into()).unwrap();
    assert_eq!(
        back.draft
            .groups
            .iter()
            .map(|g| g.id.as_str())
            .collect::<Vec<_>>(),
        ["good"]
    );
    let paths: Vec<&str> = back.dropped.iter().map(|x| x.path.as_str()).collect();
    assert_eq!(
        paths,
        ["layout.groups[1]", "layout.groups[2]", "layout.groups[3]"]
    );
    assert!(back.dropped.iter().all(|x| x.severity == Severity::Warning));
    // A bad box for a group that has agents falls back to the computed layout.
    let mut d = rigspec::templates::template("builder-reviewer", "t", "T", true).unwrap();
    d.groups = vec![group("build", "Build", 10)];
    d.agents[0].group_id = Some("build".into());
    let yaml = edit_comment(&rigspec::exchange::export_yaml(&d), |m| {
        m["groups"][0]["w"] = serde_json::json!(-260)
    });
    let back = rigspec::exchange::import_yaml(&yaml, |_| "t".into()).unwrap();
    let g = back.draft.groups.iter().find(|g| g.id == "build").unwrap();
    assert!(
        g.w > 0 && inside(back.draft.agent("builder").unwrap(), g),
        "{g:?}"
    );
    assert_eq!(
        back.dropped
            .iter()
            .map(|x| x.path.as_str())
            .collect::<Vec<_>>(),
        ["layout.groups[0]"]
    );
}

#[test]
fn a_custom_permission_policy_is_reported() {
    let base = fixture("implementation-pair");
    let member = base.replacen(
        "cwd: \".\"",
        "cwd: \".\"\n        permission_policy: builtin:locked",
        1,
    );
    let top = base.replacen("pods:", "permission_policy: policies/mine.yaml\npods:", 1);
    let standard = base.replacen(
        "cwd: \".\"",
        "cwd: \".\"\n        permission_policy: builtin:standard",
        1,
    );
    for (yaml, want) in [
        (member, vec!["pods[0].members[0].permission_policy"]),
        (top, vec!["permission_policy"]),
        (standard, vec![]),
    ] {
        let imported = rigspec::exchange::import_yaml(&yaml, |_| "x".into())
            .unwrap_or_else(|e| panic!("{e:?}\n{yaml}"));
        let got: Vec<&str> = imported
            .dropped
            .iter()
            .filter(|x| x.path.ends_with("permission_policy"))
            .map(|x| x.path.as_str())
            .collect();
        assert_eq!(got, want, "{yaml}");
    }
}

#[test]
fn a_repeated_agent_id_is_renamed_and_reported() {
    let yaml = r#"version: "0.2"
name: dup
pods:
  - id: one
    label: One
    members:
      - { id: a, agent_ref: "builtin:sculptured-agent", profile: default, runtime: stub, cwd: "." }
      - { id: a-two, agent_ref: "builtin:sculptured-agent", profile: default, runtime: stub, cwd: "." }
  - id: two
    label: Two
    members:
      - { id: a, agent_ref: "builtin:sculptured-agent", profile: default, runtime: stub, cwd: "." }
"#;
    let imported = rigspec::exchange::import_yaml(yaml, |_| "dup".into()).unwrap();
    let ids: Vec<&str> = imported
        .draft
        .agents
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(
        ids.iter().collect::<std::collections::HashSet<_>>().len() == 3,
        "ids stay unique: {ids:?}"
    );
    assert_eq!(ids[..2], ["a", "a-two"]);
    let renamed: Vec<&rigspec::Diagnostic> = imported
        .dropped
        .iter()
        .filter(|x| x.path.ends_with(".id"))
        .collect();
    assert_eq!(renamed.len(), 1, "{:?}", imported.dropped);
    assert_eq!(renamed[0].path, "pods[1].members[0].id");
    assert!(renamed[0].message.contains(ids[2]), "{:?}", renamed[0]);
    assert!(
        rigspec::validate_draft(&imported.draft)
            .iter()
            .all(|i| i.code != "agent-id-invalid")
    );
}

#[test]
fn inherited_project_folder_survives_export_and_import() {
    let mut d = rigspec::templates::template("builder-reviewer", "t", "T", false).unwrap();
    d.folder = Some("/home/me/project".into());
    d.agents[1].folder = Some("/home/me/review".into());
    let loaded =
        rigspec::exchange::import_yaml(&rigspec::exchange::export_yaml(&d), |_| "copy".into())
            .unwrap()
            .draft;
    assert_eq!(loaded.folder, d.folder);
    assert!(loaded.agents[0].folder.is_none());
    assert_eq!(loaded.agents[1].folder, d.agents[1].folder);
    assert_eq!(
        rigspec::from_draft(&loaded).unwrap().rig.seats[0].cwd,
        "/home/me/project"
    );
}
