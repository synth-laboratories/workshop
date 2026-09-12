use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mq_core::Error as CoreError;
use serde_json::json;

pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
}

impl From<CoreError> for ApiError {
    fn from(value: CoreError) -> Self {
        match value {
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
