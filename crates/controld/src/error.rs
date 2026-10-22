use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use model::DraftIssue;
use rigspec::Diagnostic;
use serde_json::json;

/// Error envelope for every API response: `{"error": {"code", "message", "diagnostics"?, "issues"?}}`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiError {
    pub status: StatusCode,
    /// Usually a constant; a connected computer's answer carries its own code (`From<LocalError>`).
    pub code: std::borrow::Cow<'static, str>,
    pub message: String,
    pub diagnostics: Option<Box<[Diagnostic]>>,
    pub issues: Option<Box<[DraftIssue]>>,
    /// What a tool answered when it could not be used, shown to the user as evidence.
    pub raw: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: std::borrow::Cow::Borrowed(code),
            message: message.into(),
            diagnostics: None,
            issues: None,
            raw: None,
        }
    }
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }
    pub fn bad_gateway(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, code, message)
    }
    pub fn unprocessable(
        code: &'static str,
        message: impl Into<String>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            diagnostics: Some(diagnostics.into_boxed_slice()),
            ..Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
        }
    }
    /// 422 for a team that cannot start: the plain-language issues travel in `error.issues`.
    #[must_use]
    pub fn invalid_team(issues: Vec<DraftIssue>) -> Self {
        let n = issues
            .iter()
            .filter(|i| i.severity == model::IssueSeverity::Error)
            .count();
        let message = format!("{n} {} to fix", if n == 1 { "thing" } else { "things" });
        Self {
            issues: Some(issues.into_boxed_slice()),
            ..Self::new(StatusCode::UNPROCESSABLE_ENTITY, "team_invalid", message)
        }
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }

    #[must_use]
    pub fn body(&self) -> serde_json::Value {
        let mut e = json!({ "code": self.code, "message": self.message });
        if let Some(d) = &self.diagnostics {
            e["diagnostics"] = serde_json::to_value(d).expect("diagnostics serialize");
        }
        if let Some(i) = &self.issues {
            e["issues"] = serde_json::to_value(i).expect("issues serialize");
        }
        if let Some(r) = &self.raw {
            e["raw"] = json!(r);
        }
        json!({ "error": e })
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body())).into_response()
    }
}

impl From<localops::LocalError> for ApiError {
    fn from(e: localops::LocalError) -> Self {
        let status = StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        Self {
            status,
            code: std::borrow::Cow::Owned(e.code),
            message: e.message,
            diagnostics: None,
            issues: None,
            raw: None,
        }
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        Self::internal(format!("store error: {e}"))
    }
}

/// A JSON body, or the error's own response.
pub(crate) fn respond<T: serde::Serialize>(r: Result<T, ApiError>) -> Response {
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => e.into_response(),
    }
}

/// `v` as JSON for an idempotent request's stored receipt.
pub(crate) fn to_value<T: serde::Serialize>(v: T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(v).map_err(|e| ApiError::internal(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ElementKind, ElementRef, IssueSeverity};
    use rigspec::Severity;

    #[test]
    fn errors_omit_absent_details_and_preserve_local_codes() {
        let error = ApiError::from(localops::LocalError::conflict(
            "changed",
            "Reload the team.",
        ));
        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(
            error.body(),
            json!({"error": {"code": "changed", "message": "Reload the team."}})
        );
    }

    #[test]
    fn diagnostics_and_raw_evidence_keep_their_wire_shape() {
        let diagnostics = vec![Diagnostic {
            path: "pods[0]".into(),
            severity: Severity::Error,
            message: "Missing an agent.".into(),
        }];
        let mut error = ApiError::unprocessable("invalid", "Cannot start.", diagnostics.clone());
        error.raw = Some("provider's exact answer".into());
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            error.body(),
            json!({"error": {
                "code": "invalid", "message": "Cannot start.", "diagnostics": diagnostics,
                "raw": "provider's exact answer",
            }})
        );
    }

    #[test]
    fn invalid_team_keeps_all_issues_but_counts_only_errors() {
        let issues: Vec<DraftIssue> = [IssueSeverity::Error, IssueSeverity::Warning]
            .into_iter()
            .map(|severity| DraftIssue {
                element: ElementRef {
                    kind: ElementKind::Team,
                    id: "team".into(),
                },
                severity,
                code: "invalid".into(),
                message: "Check the team.".into(),
                detail: None,
                spec_path: None,
            })
            .collect();
        let error = ApiError::invalid_team(issues.clone());
        assert_eq!(
            error.body(),
            json!({"error": {
                "code": "team_invalid", "message": "1 thing to fix", "issues": issues,
            }})
        );
    }
}
