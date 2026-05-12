// FeralCTF - Error definitions
// Implements FERALCTF_SPEC.md section 5.2

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// Application error type
#[derive(Debug)]
pub enum AppError {
    /// Database error
    Database(String),

    /// Authentication error
    Authentication(String),

    /// Validation error
    Validation(String),

    /// Not found error
    NotFound(String),

    /// Internal server error
    Internal(String),

    /// Custom error with status code
    Custom(StatusCode, String),

    /// Handler error for API routes
    Handler(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::Database(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    error: "Database error".to_string(),
                    message: msg,
                }),
            )
                .into_response(),
            Self::Authentication(msg) => (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    status: StatusCode::UNAUTHORIZED.as_u16(),
                    error: "Authentication error".to_string(),
                    message: msg,
                }),
            )
                .into_response(),
            Self::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    status: StatusCode::BAD_REQUEST.as_u16(),
                    error: "Validation error".to_string(),
                    message: msg,
                }),
            )
                .into_response(),
            Self::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    status: StatusCode::NOT_FOUND.as_u16(),
                    error: "Not found".to_string(),
                    message: msg,
                }),
            )
                .into_response(),
            Self::Internal(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    error: "Internal error".to_string(),
                    message: msg,
                }),
            )
                .into_response(),
            Self::Custom(status, msg) => (
                status,
                Json(ErrorResponse {
                    status: status.as_u16(),
                    error: "Custom error".to_string(),
                    message: msg,
                }),
            )
                .into_response(),
            Self::Handler(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                    error: "Handler error".to_string(),
                    message: msg,
                }),
            )
                .into_response(),
        }
    }
}

/// Generic error response structure
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub status: u16,
    pub error: String,
    pub message: String,
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(self),
        )
            .into_response()
    }
}

/// Validation error for individual fields
#[derive(Debug, Serialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

/// Handler result type for API routes
pub type HandlerResult<T> = Result<T, AppError>;

/// Handler response type for string responses
pub type HandlerResponse = String;

/// Helper trait to convert &str to HandlerResult<HandlerResponse>
pub trait IntoHandlerResponse {
    fn into_handler_response(self) -> HandlerResult<HandlerResponse>;
}

impl IntoHandlerResponse for &str {
    fn into_handler_response(self) -> HandlerResult<HandlerResponse> {
        Ok(self.to_string())
    }
}

impl IntoHandlerResponse for String {
    fn into_handler_response(self) -> HandlerResult<HandlerResponse> {
        Ok(self)
    }
}

impl IntoHandlerResponse for () {
    fn into_handler_response(self) -> HandlerResult<HandlerResponse> {
        Ok("".to_string())
    }
}
