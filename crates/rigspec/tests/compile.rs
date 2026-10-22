use rigspec::{CompiledRig, Diagnostic, EdgeKind, Runtime, Severity};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}.yaml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn errors(r: Result<CompiledRig, Vec<Diagnostic>>) -> Vec<Diagnostic> {
    r.expect_err("expected errors")
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

const MINI: &str = r#"
version: "0.2"
name: mini
pods:
  - id: dev
    label: Dev
    members:
      - { id: a, agent_ref: "local:agents/a", profile: default, runtime: stub, cwd: "." }
      - { id: b, agent_ref: "local:agents/b", profile: default, runtime: claude-code, cwd: "." }
    edges: EDGES
"#;

fn mini(edges: &str) -> String {
    MINI.replace("EDGES", edges)
}

#[test]
fn implementation_pair_compiles_with_launch_order() {
    let rig = rigspec::compile(&fixture("implementation-pair")).unwrap();
    let ids: Vec<_> = rig.seats.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        ["dev.impl@implementation-pair", "dev.qa@implementation-pair"]
    );
    assert_eq!(rig.seats[0].runtime, Runtime::ClaudeCode);
    assert_eq!(rig.seats[1].runtime, Runtime::Codex);
    assert_eq!(
        rig.launch_layers,
        vec![
            vec!["dev.impl@implementation-pair".to_string()],
            vec!["dev.qa@implementation-pair".to_string()]
        ]
    );
    assert_eq!(rig.edges[0].kind, EdgeKind::DelegatesTo);
    assert!(rig.edges[0].enforced);
    assert!(rig.spec_hash.starts_with("sha256:"));
}

#[test]
fn first_project_reports_culture_file_as_not_enforced() {
    let rig = rigspec::compile(&fixture("first-project")).unwrap();
    assert!(
        rig.diagnostics
            .iter()
            .any(|d| d.path == "culture_file" && d.severity == Severity::NotEnforced)
    );
    assert_eq!(rig.seats.len(), 2);
}

#[test]
fn unknown_member_key_is_rejected_with_its_name() {
    let yaml = mini("[]").replace(
        "runtime: stub, cwd: \".\" }",
        "runtime: stub, cwd: \".\", colour: red }",
    );
    let errs = errors(rigspec::compile(&yaml));
    assert!(
        errs.iter().any(|d| d.message.contains("colour")),
        "{errs:?}"
    );
}

#[test]
fn delegation_cycle_is_rejected() {
    let errs = errors(rigspec::compile(&mini(
        "[{kind: delegates_to, from: a, to: b}, {kind: delegates_to, from: b, to: a}]",
    )));
    assert!(errs.iter().any(|d| d.message.contains("cycle")), "{errs:?}");
}

#[test]
fn a_loop_entered_from_a_starting_seat_compiles_and_its_closing_edge_orders_nothing() {
    let yaml = mini("[{kind: delegates_to, from: a, to: b}, {kind: delegates_to, from: b, to: c}, {kind: delegates_to, from: c, to: b}]")
        .replace("    edges:", "      - { id: c, agent_ref: \"local:agents/c\", profile: default, runtime: stub, cwd: \".\" }\n    edges:");
    let rig = rigspec::compile(&yaml).unwrap();
    let layers: Vec<Vec<&str>> = rig
        .launch_layers
        .iter()
        .map(|l| l.iter().map(String::as_str).collect())
        .collect();
    assert_eq!(layers, [["dev.a@mini"], ["dev.b@mini"], ["dev.c@mini"]]);
}

#[test]
fn self_loop_is_a_cycle() {
    let errs = errors(rigspec::compile(&mini(
        "[{kind: spawned_by, from: a, to: a}]",
    )));
    assert!(errs.iter().any(|d| d.message.contains("cycle")));
}

#[test]
fn spawned_by_launches_parent_first() {
    let rig = rigspec::compile(&mini("[{kind: spawned_by, from: a, to: b}]")).unwrap();
    assert_eq!(
        rig.launch_layers,
        vec![
            vec!["dev.b@mini".to_string()],
            vec!["dev.a@mini".to_string()]
        ]
    );
}

#[test]
fn observe_edges_do_not_order_launch_and_are_not_enforced() {
    let rig = rigspec::compile(&mini("[{kind: can_observe, from: a, to: b}]")).unwrap();
    assert_eq!(rig.launch_layers.len(), 1);
    assert!(!rig.edges[0].enforced);
}

#[test]
fn pod_local_edges_must_use_bare_ids() {
    let errs = errors(rigspec::compile(&mini(
        "[{kind: delegates_to, from: dev.a, to: b}]",
    )));
    assert!(
        errs.iter().any(|d| d.path == "pods[0].edges[0].from"),
        "{errs:?}"
    );
}

#[test]
fn bare_builtin_policy_name_is_refused() {
    let yaml = mini("[]").replace("name: mini", "name: mini\npermission_policy: yolo");
    let errs = errors(rigspec::compile(&yaml));
    assert!(
        errs.iter().any(|d| d.message.contains("builtin:yolo")),
        "{errs:?}"
    );
}

#[test]
fn duplicate_member_is_rejected() {
    let yaml = mini("[]").replace("id: b,", "id: a,");
    let errs = errors(rigspec::compile(&yaml));
    assert!(
        errs.iter().any(|d| d.message.contains("duplicate")),
        "{errs:?}"
    );
}

#[test]
fn unknown_runtime_is_rejected() {
    let yaml = mini("[]").replace("runtime: stub", "runtime: gpt");
    let errs = errors(rigspec::compile(&yaml));
    assert!(errs.iter().any(|d| d.path == "pods[0].members[0].runtime"));
}

#[test]
fn yaml_syntax_error_reports_a_line() {
    let errs = errors(rigspec::compile("version: \"0.2\"\nname: [unclosed\n"));
    assert!(errs[0].path.starts_with("line "), "{errs:?}");
}

// ---- final fix wave: I1 (a spec path pointed at a secret file must not echo it) ----

const ENV_FILE: &str = "API_KEY=sk-live-SECRET123\nDB_PASSWORD=hunter2\n";
const SSH_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
QyNTUxOQAAACBTOPSECRETKEYMATERIALxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxAAA\n\
-----END OPENSSH PRIVATE KEY-----\n";

fn all_text(d: &[Diagnostic]) -> String {
    d.iter()
        .map(|d| format!("{} {}", d.path, d.message))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn env_file_is_not_a_spec_and_is_not_echoed() {
    let errs = rigspec::compile(ENV_FILE).expect_err("not a spec");
    let text = all_text(&errs);
    for secret in ["sk-live-SECRET123", "hunter2", "API_KEY", "DB_PASSWORD"] {
        assert!(!text.contains(secret), "diagnostics echo {secret}: {text}");
    }
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].message, "not a team spec (expected a YAML mapping)");
}

#[test]
fn ssh_key_file_is_not_a_spec_and_is_not_echoed() {
    let errs = rigspec::compile(SSH_KEY).expect_err("not a spec");
    let text = all_text(&errs);
    for secret in [
        "b3BlbnNzaC1rZXktdjEAAAAA",
        "SECRETKEYMATERIAL",
        "BEGIN OPENSSH",
    ] {
        assert!(!text.contains(secret), "diagnostics echo {secret}: {text}");
    }
}

#[test]
fn quoted_values_in_serde_errors_are_capped() {
    let long = format!("secret-{}", "x".repeat(200));
    // `pods` must be a list; a string there makes serde quote the value.
    let yaml = format!("version: \"0.2\"\nname: r\npods: \"{long}\"\n");
    let errs = errors(rigspec::compile(&yaml));
    let text = all_text(&errs);
    assert!(!text.contains(&long), "{text}");
    assert!(
        text.contains('…'),
        "a capped value ends with an ellipsis: {text}"
    );
    assert!(
        errs.iter().all(|d| d.message.chars().count() < 200),
        "{text}"
    );
}
