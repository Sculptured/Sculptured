//! MADL (the Multi-Agent Descriptor Language) is Sculptured's team file: a team exports as a
//! `.madl.json` document, and MADL documents in JSON or YAML import as teams. What changes while a
//! team runs stays in Sculptured's store; everything that describes the team is in MADL's core.
mod agents;
mod care;
mod export;
mod import;
mod limits;
mod links;
mod tools;
mod workflow;

pub use export::{to_madl, MADL_SCHEMA_URL};
pub use import::{from_madl, Imported};

use crate::api::{fingerprint, idempotent};
use crate::error::{respond, to_value, ApiError};
use crate::extract::{Json, Path, Query};
use crate::state::AppState;
use crate::views::TeamImport;
use axum::extract::State;
use axum::response::Response;
use rigspec::{Diagnostic, Severity};
use serde::{Deserialize, Serialize};

/// The team as a MADL document.
pub fn export(st: &AppState, id: &str, include_folders: bool) -> Result<madl::model::Document, ApiError> {
    Ok(to_madl(&crate::portable::team::export_team(st, id, include_folders)?))
}

fn diagnostics(found: impl IntoIterator<Item = madl::Diagnostic>) -> Vec<Diagnostic> {
    let severity = |s| if s == madl::Severity::Error { Severity::Error } else { Severity::Warning };
    found.into_iter().map(|d| Diagnostic { path: d.path, severity: severity(d.severity), message: format!("{} ({})", d.message, d.code) }).collect()
}

/// Parse (JSON or YAML), validate and convert MADL text. A file that refers to other files can't be
/// read here: upload it as one file (`madl expand` joins them).
pub fn read(text: &str) -> Result<Imported, ApiError> {
    let nowhere = std::env::temp_dir().join("sculptured-madl-upload-has-no-folder");
    let doc = madl::from_str(text, &nowhere, &madl::Options::default()).map_err(|e| invalid(diagnostics([e.0])))?;
    read_document(&doc)
}

/// Validate and convert a document already parsed.
pub fn read_document(doc: &madl::model::Document) -> Result<Imported, ApiError> {
    let report = madl::validate(doc);
    if !report.is_ok() {
        return Err(invalid(diagnostics(report.diagnostics)));
    }
    Ok(from_madl(&madl::expand(doc)))
}

fn invalid(d: Vec<Diagnostic>) -> ApiError {
    ApiError::unprocessable("madl_invalid", "This MADL file has errors", d)
}

#[derive(Deserialize)]
pub(super) struct FileQuery {
    #[serde(default)]
    folders: bool,
}

pub(super) async fn get_madl(State(st): State<AppState>, Path(id): Path<String>, Query(q): Query<FileQuery>) -> Response {
    respond(export(&st, &id, q.folders))
}

#[derive(Deserialize, Serialize)]
pub(super) struct ImportReq {
    #[serde(default)]
    request_id: String,
    /// A MADL document in JSON or YAML.
    text: String,
}

pub(super) async fn import_madl(State(st): State<AppState>, Json(req): Json<ImportReq>) -> Response {
    let fp = fingerprint("import-madl", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    idempotent(&st, &request_id, fp, move || async move {
        let Imported { file, dropped } = read(&req.text)?;
        let (file, warnings) = crate::portable::team::check(&st2, file)?;
        let draft = crate::portable::team::create(&st2, file)?;
        crate::analytics::emit(&st2, "team.created", serde_json::json!({ "template": "file" }));
        to_value(&TeamImport { team: crate::teams::record(&st2, draft)?, dropped, warnings })
    })
    .await
}

#[derive(Deserialize)]
pub(super) struct CheckReq {
    /// A MADL document in JSON or YAML.
    text: String,
}

#[derive(Serialize)]
struct Checked {
    /// What Design would show on the team.
    issues: Vec<model::DraftIssue>,
    /// What the document says that Sculptured could not keep.
    dropped: Vec<Diagnostic>,
}

/// The import's checks, without creating anything.
pub(super) async fn check_madl(State(st): State<AppState>, Json(req): Json<CheckReq>) -> Response {
    let checked = read(&req.text).and_then(|Imported { file, dropped }| {
        let (_, issues) = crate::portable::team::check(&st, file)?;
        Ok(Checked { issues, dropped })
    });
    respond(checked)
}
