use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use publaryn_core::error::Error as CoreError;
use serde_json::json;

/// API-level error type, maps domain errors to HTTP responses.
#[derive(Debug)]
pub struct ApiError(pub CoreError);

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            CoreError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            CoreError::AlreadyExists(msg) => (StatusCode::CONFLICT, msg.clone()),
            CoreError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            CoreError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            CoreError::Validation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            CoreError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            CoreError::PolicyViolation(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            CoreError::Database(e) => {
                tracing::error!("Database error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_owned(),
                )
            }
            CoreError::Internal(msg) => {
                tracing::error!("Internal error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_owned(),
                )
            }
        };

        let body = json!({ "error": message });
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
