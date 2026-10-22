use super::{Diagnostic, Severity};

pub const META_PREFIX: &str = "# sculptured: ";
pub const NOT_IMPORTED: &str = "not imported: the team builder does not support this yet";

#[derive(Clone, Debug)]
pub struct Imported {
    pub draft: model::TeamDraft,
    /// Parts of the spec the builder cannot represent, as warnings with their paths.
    pub dropped: Vec<Diagnostic>,
}

pub(super) fn dropped_with(path: String, message: &str) -> Diagnostic {
    Diagnostic {
        path,
        severity: Severity::Warning,
        message: message.into(),
    }
}

pub(super) fn dropped(path: String) -> Diagnostic {
    dropped_with(path, NOT_IMPORTED)
}
