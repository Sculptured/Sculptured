use super::{
    CompiledRig, Diagnostic, DraftIssue, IssueSeverity, RigSpec, Severity, TeamDraft, workflow,
};

pub const SPEC_VERSION: &str = "0.2";
pub const AGENT_REF: &str = "builtin:sculptured-agent";
pub const RESTRICTED_POLICY: &str = "builtin:standard";
pub const GENERIC_AGENT: &str = "This agent has a configuration problem (details under Advanced)";
pub const GENERIC_TEAM: &str = "This team has a configuration problem (details under Advanced)";
pub const GENERIC_GROUP: &str = "This group has a configuration problem (details under Advanced)";
pub const GENERIC_CONNECTION: &str =
    "This connection has a configuration problem (details under Advanced)";

/// The RigSpec a draft produces (also for drafts that do not validate).
#[must_use]
pub fn draft_spec(d: &TeamDraft) -> RigSpec {
    crate::draft::build(d).0
}

pub(super) fn strip_nulls(v: &mut serde_yaml_ng::Value) {
    match v {
        serde_yaml_ng::Value::Mapping(m) => m.retain(|_, x| {
            strip_nulls(x);
            !x.is_null()
        }),
        serde_yaml_ng::Value::Sequence(sequence) => {
            for value in sequence {
                strip_nulls(value);
            }
        }
        _ => {}
    }
}

/// Canonical YAML without `null` fields, in RigSpec field order.
#[must_use]
pub fn spec_yaml(spec: &RigSpec) -> String {
    let mut v = serde_yaml_ng::to_value(spec).expect("a RigSpec always serializes");
    strip_nulls(&mut v);
    serde_yaml_ng::to_string(&v).expect("a YAML value always serializes")
}

/// Maps one compiler diagnostic to the draft element it concerns, in plain words.
#[must_use]
pub fn map_diagnostic(d: &TeamDraft, diag: &Diagnostic) -> DraftIssue {
    let (_, index) = crate::draft::build(d);
    crate::draft::map_with(d, &index, diag)
}

/// A compiled draft. `warnings` are non-blocking issues (Codex, restricted posture).
#[derive(Clone, Debug)]
pub struct DraftBuild {
    pub spec: RigSpec,
    pub yaml: String,
    pub rig: CompiledRig,
    pub warnings: Vec<DraftIssue>,
}

/// Checks the draft, builds its RigSpec and compiles it with the normal rules.
/// Errors stop before compiling; compiler errors are mapped back to draft elements.
pub fn from_draft(d: &TeamDraft) -> Result<DraftBuild, Vec<DraftIssue>> {
    let mut issues = crate::draft::check_draft(d);
    issues.extend(workflow::check(d));
    issues.extend(workflow::check_schedules(d));
    if issues.iter().any(|i| i.severity == IssueSeverity::Error) {
        return Err(issues);
    }
    let (spec, index) = crate::draft::build(d);
    let yaml = spec_yaml(&spec);
    match crate::draft::compile(&yaml) {
        Ok(rig) => {
            issues.extend(
                rig.diagnostics
                    .iter()
                    .filter(|x| x.severity == Severity::Warning)
                    .map(|x| crate::draft::map_with(d, &index, x)),
            );
            Ok(DraftBuild {
                spec,
                yaml,
                rig,
                warnings: issues,
            })
        }
        Err(diags) => {
            issues.extend(
                diags
                    .iter()
                    .filter(|x| x.severity != Severity::NotEnforced)
                    .map(|x| crate::draft::map_with(d, &index, x)),
            );
            Err(issues)
        }
    }
}

/// Every issue for a draft: errors when it would not start, otherwise its warnings.
#[must_use]
pub fn validate_draft(d: &TeamDraft) -> Vec<DraftIssue> {
    match from_draft(d) {
        Ok(b) => b.warnings,
        Err(issues) => issues,
    }
}
