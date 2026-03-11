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
    /// Generic bad request.
    BadRequest(String),
    /// A requested resource does not exist.
    NotFound,
    /// The pseudonym has already been registered.
    PseudonymAlreadyUsed,
    /// Proof cryptographic verification failed.
    ProofVerificationFailed,
    /// The verifier is not configured on this registry instance.
    VerifierNotConfigured,
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
            AppError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                msg.clone(),
            ),
            AppError::NotFound => (
                StatusCode::NOT_FOUND,
                "not found".to_string(),
            ),
            AppError::PseudonymAlreadyUsed => (
                StatusCode::CONFLICT,
                "pseudonym already registered".to_string(),
            ),
            AppError::ProofVerificationFailed => (
                StatusCode::BAD_REQUEST,
                "proof verification failed".to_string(),
            ),
            AppError::VerifierNotConfigured => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "verifier not configured".to_string(),
            ),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}
