use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    /// The nullifier has already been registered (Sybil attempt).
    NullifierAlreadyUsed,
    /// A hex-encoded field could not be decoded.
    InvalidHex(String),
    /// A requested anonymity set does not exist.
    SetNotFound(u64),
    /// A database operation failed.
    Db(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NullifierAlreadyUsed => (
                StatusCode::CONFLICT,
                "nullifier already registered".to_string(),
            ),
            AppError::InvalidHex(field) => (
                StatusCode::BAD_REQUEST,
                format!("invalid hex in field `{field}`"),
            ),
            AppError::SetNotFound(id) => (
                StatusCode::NOT_FOUND,
                format!("anonymity set {id} not found"),
            ),
            AppError::Db(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("database error: {msg}"),
            ),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
