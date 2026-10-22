/// A refusal or failure, with the HTTP status and code controld answers with. Serializable, so a
/// connected computer's answer crosses the wire unchanged.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[error("{message}")]
pub struct LocalError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

impl LocalError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn bad(code: &str, m: impl Into<String>) -> Self {
        Self::new(400, code, m)
    }
    pub fn forbidden(code: &str, m: impl Into<String>) -> Self {
        Self::new(403, code, m)
    }
    pub fn not_found(code: &str, m: impl Into<String>) -> Self {
        Self::new(404, code, m)
    }
    pub fn conflict(code: &str, m: impl Into<String>) -> Self {
        Self::new(409, code, m)
    }
    pub fn failed(code: &str, m: impl Into<String>) -> Self {
        Self::new(500, code, m)
    }
}
