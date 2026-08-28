//! Typed errors at the Tauri command edge.
//!
//! Paragon: [`crate::codex::CodexTurnFailure`] — stable `code` + user `message` +
//! developer `detail`, so the renderer never string-matches prose.
//!
//! Marker causes below travel inside `anyhow` chains so transport/IPC layers can
//! classify without substring matching on rendered error text.

use serde::Serialize;
use std::fmt;

/// Informative error payload serialized across the Tauri boundary.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
    /// Developer-facing text. Keep out of user toasts.
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<crate::platform::failure::FailureView>,
    /// The typed failure this error was classified from, kept so a loopback
    /// boundary can re-raise it instead of flattening a machine code back to
    /// prose. Never serialized: the renderer reads `code`, and `detail`
    /// already carries the rendered payload.
    #[serde(skip)]
    #[specta(skip)]
    pub structured: Option<StructuredFailure>,
}

pub const CODE_INTERNAL: &str = "internal";
pub const CODE_INVALID_ARGUMENT: &str = "invalid_argument";
pub const CODE_NOT_FOUND: &str = "not_found";
pub const CODE_UNAUTHORIZED: &str = "unauthorized";
pub const CODE_PROTOCOL_MISMATCH: &str = "protocol_mismatch";
pub const CODE_CONFLICT: &str = "conflict";
pub const CODE_IO: &str = "io";
pub const CODE_CANCELLED: &str = "cancelled";
pub const CODE_DATABASE_LOCKED: &str = "database_locked";
pub const CODE_APPROVAL_EXPIRED: &str = "approval_expired";

impl AppError {
    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            code: code.into(),
            message: message.clone(),
            detail: message,
            failure: None,
            structured: None,
        }
    }

    pub fn internal(error: impl fmt::Display + fmt::Debug) -> Self {
        Self {
            code: CODE_INTERNAL.into(),
            message: error.to_string(),
            detail: format!("{error:?}"),
            failure: None,
            structured: None,
        }
    }

    pub fn from_view(view: crate::platform::failure::FailureView) -> Self {
        Self {
            code: view.code.clone(),
            message: view.message.clone(),
            detail: view.diagnostic_reference.clone(),
            failure: Some(view),
            structured: None,
        }
    }

    pub fn from_occurrence(failure: &crate::platform::failure::OperationalFailure) -> Self {
        Self::from_view(crate::platform::failure::FailureView::from_occurrence(
            failure,
        ))
    }

    /// Boundary helper for remaining untyped command edges. New code must raise
    /// a `FailureKind` instead; `scripts/check-failure-runtime.sh` rejects new
    /// call sites outside `error.rs`.
    pub fn untyped(message: impl Into<String>) -> Self {
        Self::coded(CODE_INTERNAL, message)
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::coded(CODE_INVALID_ARGUMENT, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::coded(CODE_NOT_FOUND, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::coded(CODE_UNAUTHORIZED, message)
    }

    pub fn protocol_mismatch(message: impl Into<String>) -> Self {
        Self::coded(CODE_PROTOCOL_MISMATCH, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::coded(CODE_CONFLICT, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::coded(CODE_IO, message)
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::coded(CODE_CANCELLED, message)
    }

    /// Re-raise across an `anyhow` boundary without losing the machine code.
    ///
    /// A loopback hop that rebuilds the error from `to_string()` turns
    /// `approval_expired` into a sentence, and the agent on the far side can
    /// then only guess at what happened.
    pub fn into_anyhow(self) -> anyhow::Error {
        match self.structured.clone() {
            Some(failure) => anyhow::Error::new(failure),
            None => anyhow::anyhow!(self.message.clone()),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        if error_is::<Unauthorized>(&error) {
            return Self::unauthorized(error.to_string()).with_detail(format!("{error:?}"));
        }
        if error_is::<ProtocolMismatch>(&error) {
            return Self::protocol_mismatch(error.to_string()).with_detail(format!("{error:?}"));
        }
        if error_is::<DatabaseLocked>(&error) {
            return Self::coded(CODE_DATABASE_LOCKED, error.to_string())
                .with_detail(format!("{error:?}"));
        }
        if error_is::<ApprovalExpired>(&error) {
            return Self::coded(CODE_APPROVAL_EXPIRED, error.to_string())
                .with_detail(format!("{error:?}"));
        }
        if error_is::<StructuredFailure>(&error) {
            if let Some(failure) = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<StructuredFailure>())
            {
                return Self {
                    code: failure.code.to_string(),
                    message: failure.message.clone(),
                    detail: failure.to_json().to_string(),
                    failure: None,
                    structured: Some(failure.clone()),
                };
            }
        }
        if let Some(failure) = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<crate::secrets::lease::CredentialError>())
        {
            return Self {
                code: failure.code.clone(),
                message: failure.message.clone(),
                detail: serde_json::to_string(failure).unwrap_or_else(|_| failure.to_string()),
                failure: None,
            };
        }
        Self::internal(error)
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::io(error.to_string()).with_detail(format!("{error:?}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::invalid_argument(error.to_string()).with_detail(format!("{error:?}"))
    }
}

impl From<reqwest::Error> for AppError {
    fn from(error: reqwest::Error) -> Self {
        Self::internal(error)
    }
}

impl From<tokio::sync::oneshot::error::RecvError> for AppError {
    fn from(error: tokio::sync::oneshot::error::RecvError) -> Self {
        Self::cancelled(error.to_string()).with_detail(format!("{error:?}"))
    }
}

impl From<tauri_plugin_opener::Error> for AppError {
    fn from(error: tauri_plugin_opener::Error) -> Self {
        Self::internal(error)
    }
}

impl From<std::net::AddrParseError> for AppError {
    fn from(error: std::net::AddrParseError) -> Self {
        Self::invalid_argument(error.to_string()).with_detail(format!("{error:?}"))
    }
}

/// IPC / loopback auth failure. Prefer `anyhow!(Unauthorized)` over prose bail.
#[derive(Debug)]
pub struct Unauthorized;

impl fmt::Display for Unauthorized {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("unauthorized")
    }
}

impl std::error::Error for Unauthorized {}

/// Eval-driver protocol version disagreement.
#[derive(Debug)]
pub struct ProtocolMismatch {
    pub expected: &'static str,
    pub got: String,
}

impl fmt::Display for ProtocolMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "protocol mismatch: expected {}, got {}",
            self.expected, self.got
        )
    }
}

impl std::error::Error for ProtocolMismatch {}

/// Codex app-server reported SQLite contention. Classified once at the
/// transport boundary; callers retry via [`error_is`].
#[derive(Debug)]
pub struct DatabaseLocked;

impl fmt::Display for DatabaseLocked {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("database is locked")
    }
}

impl std::error::Error for DatabaseLocked {}

#[derive(Debug)]
pub struct ApprovalExpired {
    pub approval_id: String,
}

impl fmt::Display for ApprovalExpired {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "approval_expired: paid-compute approval {} expired before settlement",
            self.approval_id
        )
    }
}

impl std::error::Error for ApprovalExpired {}

/// A failure that carries a stable machine code and a remediation across a
/// loopback IPC boundary.
///
/// Prose that crosses a process boundary arrives as prose: the agent on the
/// other side can only string-match it, the tool-loop breaker cannot tell one
/// root cause from another, and the renderer has nothing to show but the
/// sentence. Bail with one of these wherever the caller has a decision to make.
#[derive(Debug, Clone)]
pub struct StructuredFailure {
    pub code: &'static str,
    pub message: String,
    pub remediation: String,
    pub retryable: bool,
    pub details: serde_json::Value,
}

impl StructuredFailure {
    pub fn new(
        code: &'static str,
        message: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            remediation: remediation.into(),
            retryable: false,
            details: serde_json::Value::Null,
        }
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }

    pub fn to_json(&self) -> serde_json::Value {
        let mut body = serde_json::json!({
            "code": self.code,
            "error": self.message,
            "remediation": self.remediation,
            "retryable": self.retryable,
        });
        if !self.details.is_null() {
            if let Some(fields) = self.details.as_object() {
                for (key, value) in fields {
                    if !body
                        .as_object()
                        .is_some_and(|object| object.contains_key(key))
                    {
                        body[key] = value.clone();
                    }
                }
            }
            body["details"] = self.details.clone();
        }
        body
    }
}

impl fmt::Display for StructuredFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {} — {}",
            self.code, self.message, self.remediation
        )
    }
}

impl std::error::Error for StructuredFailure {}

pub fn error_is<E: std::error::Error + 'static>(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<E>())
}
