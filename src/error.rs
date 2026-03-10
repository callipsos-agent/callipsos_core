use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Inspects a sqlx::Error for known constraint violations and maps them
    /// to appropriate application errors instead of a blanket 500.
    ///
    /// - SQLSTATE 23505 (unique_violation) → Conflict (409)
    /// - SQLSTATE 23503 (foreign_key_violation) → NotFound (404)
    /// - Everything else → Database (500)
    pub fn from_db(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(ref db_err) = e {
            match db_err.code().as_deref() {
                Some("23505") => return AppError::Conflict("Resource already exists".into()),
                Some("23503") => {
                    return AppError::NotFound("Referenced resource not found".into())
                }
                _ => {}
            }
        }
        AppError::Database(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::Database(e) => {
                tracing::error!("Database error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            AppError::Internal(msg) => {
                tracing::error!("Internal error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error".to_string())
            }
        };

        let body = axum::Json(json!({ "error": message }));
        (status, body).into_response()
    }
}