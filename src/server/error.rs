use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use tracing::error;

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

#[allow(dead_code)]
pub enum ServerError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
    Database(String),
}

impl std::fmt::Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerError::NotFound(msg) => write!(f, "Not found: {}", msg),
            ServerError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            ServerError::Internal(msg) => write!(f, "Internal error: {}", msg),
            ServerError::Database(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ServerError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ServerError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ServerError::Internal(msg) => {
                error!("Internal server error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal server error".to_string(),
                )
            }
            ServerError::Database(msg) => {
                error!("Database error: {}", msg);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Database error".to_string(),
                )
            }
        };

        let body = Json(ErrorResponse {
            error: message,
            code: status.as_u16(),
        });

        (status, body).into_response()
    }
}

impl From<sqlx::Error> for ServerError {
    fn from(e: sqlx::Error) -> Self {
        ServerError::Database(e.to_string())
    }
}

impl From<crate::error::StorageError> for ServerError {
    fn from(e: crate::error::StorageError) -> Self {
        match e {
            crate::error::StorageError::NotFound(msg) => ServerError::NotFound(msg),
            crate::error::StorageError::Database(msg) => ServerError::Database(msg),
            crate::error::StorageError::Migration(msg) => {
                ServerError::Database(format!("migration: {}", msg))
            }
            crate::error::StorageError::LlmOperation { operation, message } => {
                ServerError::Internal(format!("{}: {}", operation, message))
            }
        }
    }
}
