use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;

use crate::types::{ErrorDetail, ErrorResponse};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("upstream copilot error: {0}")]
    Upstream(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Upstream(_) => StatusCode::BAD_GATEWAY,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn err_type(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "invalid_request_error",
            AppError::Unauthorized => "authentication_error",
            AppError::Upstream(_) => "upstream_error",
            AppError::Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let err_type = self.err_type().to_string();
        let body = ErrorResponse {
            error: ErrorDetail {
                message: self.to_string(),
                err_type,
                code: None,
            },
        };
        (status, Json(body)).into_response()
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::BadRequest(format!("json error: {e}"))
    }
}
