use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use crate::core::{Name, compute_nullifier, PublicKey};

use crate::registry::{error::AppError, server::AppState};

/// `POST /api/v1/has`
///
/// Body:
/// ```json
/// { "pub_key": "<64 hex chars>", "name": "alice" }
/// ```
///
/// Response:
/// ```json
/// { "present": true, "nullifier": "<64 hex chars>" }
/// ```
#[derive(Deserialize)]
pub struct HasRequest {
    /// Hex-encoded 32-byte public key.
    pub pub_key: String,
    /// The human-readable name (username / handle).
    pub name: Name,
}

#[derive(Serialize)]
pub struct HasResponse {
    pub present: bool,
    pub nullifier: String,
}

pub async fn has(
    State(state): State<AppState>,
    Json(body): Json<HasRequest>,
) -> Result<Json<HasResponse>, AppError> {
    let pub_key =
        PublicKey::from_hex(&body.pub_key).map_err(|_| AppError::InvalidHex("pub_key".into()))?;

    let nullifier = compute_nullifier(&pub_key, &body.name);
    let store = state.store.read().await;
    let present = store.has_nullifier(&nullifier);

    Ok(Json(HasResponse {
        present,
        nullifier: nullifier.to_hex(),
    }))
}
