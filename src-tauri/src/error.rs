use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// Errors returned to the frontend. Each variant maps to a stable `code` so the UI can show a
/// localized message, plus the raw `message` for details.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("IMAP error: {0}")]
    Imap(String),
    #[error("SMTP error: {0}")]
    Smtp(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("sign-in required")]
    AuthRequired,
    #[error("OAuth is not configured for {0}")]
    OAuthNotConfigured(String),
    #[error("secret store error: {0}")]
    Secrets(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("this account has no archive folder")]
    NoArchiveFolder,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Db(_) => "db",
            AppError::Imap(_) => "imap",
            AppError::Smtp(_) => "smtp",
            AppError::Network(_) => "network",
            AppError::Auth(_) => "auth",
            AppError::AuthRequired => "auth_required",
            AppError::OAuthNotConfigured(_) => "oauth_not_configured",
            AppError::Secrets(_) => "secrets",
            AppError::NotFound(_) => "not_found",
            AppError::NoArchiveFolder => "no_archive_folder",
            AppError::Cancelled => "cancelled",
            AppError::Invalid(_) => "invalid",
            AppError::Other(_) => "other",
        }
    }

    /// Errors after which a fresh connection is worth a retry.
    pub fn is_connection_error(&self) -> bool {
        matches!(self, AppError::Network(_) | AppError::Imap(_))
    }

    pub fn is_auth_error(&self) -> bool {
        matches!(self, AppError::Auth(_) | AppError::AuthRequired)
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

impl From<async_imap::error::Error> for AppError {
    fn from(e: async_imap::error::Error) -> Self {
        use async_imap::error::Error as E;
        match e {
            E::Io(io) => AppError::Network(io.to_string()),
            E::ConnectionLost => AppError::Network("connection lost".into()),
            other => AppError::Imap(other.to_string()),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Network(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Network(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Other(e.to_string())
    }
}

impl From<lettre::transport::smtp::Error> for AppError {
    fn from(e: lettre::transport::smtp::Error) -> Self {
        if e.is_permanent() && e.to_string().contains("535") {
            AppError::Auth(e.to_string())
        } else {
            AppError::Smtp(e.to_string())
        }
    }
}

impl From<lettre::error::Error> for AppError {
    fn from(e: lettre::error::Error) -> Self {
        AppError::Invalid(e.to_string())
    }
}

impl From<lettre::address::AddressError> for AppError {
    fn from(e: lettre::address::AddressError) -> Self {
        AppError::Invalid(format!("invalid address: {e}"))
    }
}

impl From<tokio::time::error::Elapsed> for AppError {
    fn from(_: tokio::time::error::Elapsed) -> Self {
        AppError::Network("timed out".into())
    }
}
