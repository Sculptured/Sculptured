use super::{
    ApiError, AppState, Deserialize, Diagnostic, Imported, Json, Path, Query, Response, Serialize,
    Severity, State, TeamImport,
};

/// The team as a MADL document.
pub fn export(
    st: &AppState,
    id: &str,
    include_folders: bool,
) -> Result<madl::model::Document, ApiError> {
    Ok(crate::portable::madl::to_madl(
        &crate::portable::team::export_team(st, id, include_folders)?,
    ))
}

pub(super) fn diagnostics(found: impl IntoIterator<Item = madl::Diagnostic>) -> Vec<Diagnostic> {
    let severity = |s| {
        if s == madl::Severity::Error {
            Severity::Error
        } else {
            Severity::Warning
        }
    };
    found
        .into_iter()
        .map(|d| Diagnostic {
            path: d.path,
            severity: severity(d.severity),
            message: format!("{} ({})", d.message, d.code),
        })
        .collect()
}

/// Parse (JSON or YAML), validate and convert MADL text. A file that refers to other files can't be
/// read here: upload it as one file (`madl expand` joins them).
pub fn read(text: &str) -> Result<Imported, ApiError> {
    if let Some(file) = legacy(text)? {
        return Ok(Imported {
            file,
            dropped: Vec::new(),
        });
    }
    let nowhere = std::env::temp_dir().join("sculptured-madl-upload-has-no-folder");
    let doc = madl::from_str(text, &nowhere, &madl::Options::default())
        .map_err(|e| invalid(diagnostics([e.0])))?;
    read_document(&doc)
}

/// A `sculptured.team/v1` file, exported before teams were MADL documents: still read, never written.
pub(super) fn legacy(text: &str) -> Result<Option<crate::portable::team::PortableTeam>, ApiError> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Ok(None);
    };
    if v.get("format").and_then(|f| f.as_str()) != Some("sculptured.team/v1") {
        return Ok(None);
    }
    serde_json::from_value(v).map(Some).map_err(|e| {
        ApiError::bad_request(
            "team_file_invalid",
            format!("This team file could not be read: {e}"),
        )
    })
}

/// Validate and convert a document already parsed.
pub fn read_document(doc: &madl::model::Document) -> Result<Imported, ApiError> {
    let report = madl::validate(doc);
    if !report.is_ok() {
        return Err(invalid(diagnostics(report.diagnostics)));
    }
    Ok(crate::portable::madl::from_madl(&madl::expand(doc)))
}

pub(super) fn invalid(d: Vec<Diagnostic>) -> ApiError {
    ApiError::unprocessable("madl_invalid", "This MADL file has errors", d)
}

#[derive(Deserialize)]
pub(in super::super) struct FileQuery {
    #[serde(default)]
    pub(super) folders: bool,
}

pub(in super::super) async fn get_madl(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<FileQuery>,
) -> Response {
    crate::portable::madl::respond(export(&st, &id, q.folders))
}

#[derive(Deserialize, Serialize)]
pub(in super::super) struct ImportReq {
    #[serde(default)]
    pub(super) request_id: String,
    /// A MADL document in JSON or YAML.
    pub(super) text: String,
}

pub(in super::super) async fn import_madl(
    State(st): State<AppState>,
    Json(req): Json<ImportReq>,
) -> Response {
    let fp = crate::portable::madl::fingerprint("import-madl", &req);
    let request_id = req.request_id.clone();
    let st2 = st.clone();
    crate::portable::madl::idempotent(&st, &request_id, fp, move || async move {
        let Imported { file, dropped } = read(&req.text)?;
        let (file, warnings) = crate::portable::team::check(&st2, file)?;
        let draft = crate::portable::team::create(&st2, file)?;
        crate::analytics::emit(
            &st2,
            "team.created",
            serde_json::json!({ "template": "file" }),
        );
        crate::portable::madl::to_value(&TeamImport {
            team: crate::teams::record(&st2, draft)?,
            dropped,
            warnings,
        })
    })
    .await
}

#[derive(Deserialize)]
pub(in super::super) struct CheckReq {
    /// A MADL document in JSON or YAML.
    pub(super) text: String,
}

#[derive(Serialize)]
pub(super) struct Checked {
    /// What Design would show on the team.
    pub(super) issues: Vec<model::DraftIssue>,
    /// What the document says that Sculptured could not keep.
    pub(super) dropped: Vec<Diagnostic>,
}

/// The import's checks, without creating anything.
pub(in super::super) async fn check_madl(
    State(st): State<AppState>,
    Json(req): Json<CheckReq>,
) -> Response {
    let checked = read(&req.text).and_then(|Imported { file, dropped }| {
        let (_, issues) = crate::portable::team::check(&st, file)?;
        Ok(Checked { issues, dropped })
    });
    crate::portable::madl::respond(checked)
}
