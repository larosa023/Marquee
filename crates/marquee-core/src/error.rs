use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors are written for the person using the app: every message should say what went wrong
/// in plain words, and `kind` lets the UI choose the right state (offline, reconnect, setup).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    /// No network, DNS failure, timeout. The UI shows the offline state and keeps cached data.
    #[error("You're offline, or {0} can't be reached right now.")]
    Offline(String),
    /// The service said the saved sign-in is no longer valid.
    #[error("{0} needs you to sign in again.")]
    Reauth(String),
    /// An API key or account the feature needs hasn't been set up.
    #[error("{0}")]
    NotConfigured(String),
    #[error("{service} returned an error ({status}): {detail}")]
    Api { service: String, status: u16, detail: String },
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("File error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Unexpected data: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn msg(s: impl Into<String>) -> Self {
        Error::Message(s.into())
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Error::Offline(_) => "offline",
            Error::Reauth(_) => "reauth",
            Error::NotConfigured(_) => "not_configured",
            Error::Api { .. } => "api",
            _ => "error",
        }
    }

    pub fn is_offline(&self) -> bool {
        matches!(self, Error::Offline(_))
    }
}

impl From<csv::Error> for Error {
    fn from(e: csv::Error) -> Self {
        Error::msg(format!("Couldn't read that CSV file: {e}"))
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(e: zip::result::ZipError) -> Self {
        Error::msg(format!("Couldn't read that zip file: {e}"))
    }
}

/// What the frontend receives when a call fails.
#[derive(Serialize)]
pub struct ErrorPayload {
    pub kind: &'static str,
    pub message: String,
}

impl From<&Error> for ErrorPayload {
    fn from(e: &Error) -> Self {
        ErrorPayload { kind: e.kind(), message: e.to_string() }
    }
}
