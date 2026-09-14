use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mq_core::Error as CoreError;
use serde_json::json;

pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
}

/// Stable client-actionable codes from docs/WORKSHOP_GRANT_CONTRACT.md §7.
/// Other codes stay generic so internal detail and existence are not leaked.
const PUBLIC_CODES: &[&str] = &[
    "grant_revoked",
    "grant_expired",
    "grant_generation_stale",
    "grant_incarnation_fenced",
    "grant_operation_denied",
    "grant_membership_required",
    "invite_required",
    "enrollment_owner_must_be_human",
    "enrollment_revoked",
    "grant_exists",
    "history_cursor_before_floor",
    "invalid_operations",
    "invalid_ttl",
    "invalid_history_bound",
    "invalid_device_identity",
    "invalid_incarnation",
    "recipient_grant_inactive",
    "delivery_verifier_required",
];

fn public(code: &'static str) -> Option<&'static str> {
    PUBLIC_CODES.contains(&code).then_some(code)
}

impl From<CoreError> for ApiError {
    fn from(value: CoreError) -> Self {
        match value {
            CoreError::Forbidden(code) if public(code).is_some() => Self {
                status: StatusCode::FORBIDDEN,
                code,
            },
            CoreError::Conflict(code) if public(code).is_some() => Self {
                status: StatusCode::CONFLICT,
                code,
            },
            CoreError::Invalid(code) if public(code).is_some() => Self {
                status: StatusCode::BAD_REQUEST,
                code,
            },
            CoreError::Unauthenticated => Self {
                status: StatusCode::UNAUTHORIZED,
                code: "unauthenticated",
            },
            CoreError::Forbidden(_) => Self {
                status: StatusCode::FORBIDDEN,
                code: "forbidden",
            },
            CoreError::NotFound(_) => Self {
                status: StatusCode::NOT_FOUND,
                code: "not_found",
            },
            CoreError::Conflict(_) => Self {
                status: StatusCode::CONFLICT,
                code: "conflict",
            },
            CoreError::Invalid(_) => Self {
                status: StatusCode::BAD_REQUEST,
                code: "invalid",
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.code }))).into_response()
    }
}
