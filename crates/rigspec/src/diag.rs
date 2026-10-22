use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Severity {
    Error,
    Warning,
    /// Declared in the spec, accepted, and shown as "declared · not enforced".
    NotEnforced,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Diagnostic {
    /// Location such as `pods[0].members[1].runtime`, or `line 4:7` for syntax errors.
    pub path: String,
    pub severity: Severity,
    pub message: String,
}

pub(crate) fn error(path: impl Into<String>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        path: path.into(),
        severity: Severity::Error,
        message: message.into(),
    }
}
pub(crate) fn not_enforced(path: impl Into<String>) -> Diagnostic {
    Diagnostic {
        path: path.into(),
        severity: Severity::NotEnforced,
        message: "declared · not enforced in this version".into(),
    }
}

/// `base.field` (or `field` when `base` is empty) for each field that is present.
pub(crate) fn present_paths<'a>(
    base: &'a str,
    fields: &'a [(&'a str, bool)],
) -> impl Iterator<Item = String> + 'a {
    fields
        .iter()
        .filter(|(_, present)| *present)
        .map(move |(field, _)| {
            if base.is_empty() {
                field.to_string()
            } else {
                format!("{base}.{field}")
            }
        })
}
