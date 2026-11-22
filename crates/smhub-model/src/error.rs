//! The one error type of the daemon. Each variant maps to a stable code that the web UI
//! translates (API contract, schema `Error`) and to an HTTP status.

use std::fmt;

/// Result with the daemon's error type.
pub type Result<T> = std::result::Result<T, Error>;

/// An error with a stable code. Messages are English detail for logs and experts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Invalid { field: Option<String>, message: String },
    /// A refusal common enough to have its own code and translated text (e.g. `wifi_not_found`).
    Specific { code: &'static str, field: Option<&'static str>, message: String },
    Conflict(String),
    Busy(String),
    Unavailable(String),
    AuthFailed(String),
    RateLimited(String),
    TooLarge(String),
    NotSupported(String),
    Internal(String),
}

impl Error {
    /// An invalid value in the named input field.
    pub fn invalid(field: &str, message: impl Into<String>) -> Self {
        Self::Invalid { field: Some(field.to_owned()), message: message.into() }
    }

    /// An invalid request that is not about one field.
    pub fn bad(message: impl Into<String>) -> Self {
        Self::Invalid { field: None, message: message.into() }
    }

    pub fn not_found(what: impl Into<String>) -> Self {
        Self::NotFound(what.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// The stable code from the API contract.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(_) => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Invalid { .. } => "invalid_value",
            Self::Specific { code, .. } => code,
            Self::Conflict(_) => "conflict",
            Self::Busy(_) => "busy",
            Self::Unavailable(_) => "unavailable",
            Self::AuthFailed(_) => "auth_failed",
            Self::RateLimited(_) => "rate_limited",
            Self::TooLarge(_) => "too_large",
            Self::NotSupported(_) => "not_supported",
            Self::Internal(_) => "internal",
        }
    }

    /// The HTTP status for this error.
    pub fn status(&self) -> u16 {
        match self {
            Self::Unauthorized(_) | Self::AuthFailed(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::Invalid { .. } | Self::Specific { .. } => 422,
            Self::Conflict(_) | Self::Busy(_) => 409,
            Self::Unavailable(_) => 503,
            Self::RateLimited(_) => 429,
            Self::TooLarge(_) => 413,
            Self::NotSupported(_) => 501,
            Self::Internal(_) => 500,
        }
    }

    /// The input field at fault, if any.
    pub fn field(&self) -> Option<&str> {
        match self {
            Self::Invalid { field, .. } => field.as_deref(),
            Self::Specific { field, .. } => *field,
            _ => None,
        }
    }

    /// The English message.
    pub fn message(&self) -> &str {
        match self {
            Self::Invalid { message, .. } | Self::Specific { message, .. } => message,
            Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::NotFound(m)
            | Self::Conflict(m)
            | Self::Busy(m)
            | Self::Unavailable(m)
            | Self::AuthFailed(m)
            | Self::RateLimited(m)
            | Self::TooLarge(m)
            | Self::NotSupported(m)
            | Self::Internal(m) => m,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.field() {
            Some(field) => write!(f, "{}: {} ({field})", self.code(), self.message()),
            None => write!(f, "{}: {}", self.code(), self.message()),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound(e.to_string()),
            std::io::ErrorKind::PermissionDenied => Self::Forbidden(e.to_string()),
            _ => Self::Internal(e.to_string()),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(format!("json: {e}"))
    }
}
