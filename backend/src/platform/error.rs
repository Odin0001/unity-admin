use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use super::password::PasswordError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid email or password")]
    InvalidCredentials,
    #[error("missing or invalid credentials")]
    Unauthorized,
    #[error("not allowed")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("too many attempts, try again in {retry_after} seconds")]
    TooManyRequests { retry_after: u64 },
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Password(#[from] PasswordError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::TooManyRequests { retry_after } = self {
            return too_many_requests(&self, retry_after);
        }

        let status = match &self {
            Self::InvalidCredentials | Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Invalid(_) => StatusCode::BAD_REQUEST,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Database(_) | Self::Password(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        // Internal details go to the log, never to the client.
        let message = if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
            "internal server error".to_string()
        } else {
            self.to_string()
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

fn too_many_requests(err: &AppError, retry_after: u64) -> Response {
    (
        StatusCode::TOO_MANY_REQUESTS,
        [(header::RETRY_AFTER, retry_after.to_string())],
        Json(json!({ "error": err.to_string() })),
    )
        .into_response()
}
