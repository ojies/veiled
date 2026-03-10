use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use veiled_core::{Commitment, Nullifier};

use crate::{error::AppError, server::AppState};

/// `POST /api/v1/register`
///
/// Body (single nullifier — legacy):
/// ```json
/// { "commitment": "<66 hex chars>", "nullifier": "<64 hex chars>" }
/// ```
///
/// Response 200:
/// ```json
/// { "set_id": 0, "index": 3 }
/// ```
///
/// Response 409 if the nullifier has already been registered.
#[derive(Deserialize)]
pub struct RegisterRequest {
    /// Hex-encoded 33-byte commitment (compressed secp256k1 point).
    pub commitment: String,
    /// Hex-encoded 32-byte nullifier.
    pub nullifier: String,
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub set_id: u64,
    pub index: usize,
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AppError> {
    let commitment =
        Commitment::from_hex(&body.commitment).map_err(|_| AppError::InvalidHex("commitment".into()))?;
    let nullifier =
        Nullifier::from_hex(&body.nullifier).map_err(|_| AppError::InvalidHex("nullifier".into()))?;

    let result = {
        let mut store = state.store.write().await;
        store.register(commitment, nullifier).map_err(|_| AppError::NullifierAlreadyUsed)?
    };

    // Persist to SQLite.  Do this after releasing the store write-lock so we
    // don't hold it during I/O.
    if result.new_set_opened {
        let cap = state.store.read().await.set_capacity;
        state.db.persist_new_set(result.set_id, cap)
            .map_err(|e| AppError::Db(e.to_string()))?;
    }
    state.db.persist_registration(result.set_id, result.index, &commitment, &nullifier)
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok((StatusCode::OK, Json(RegisterResponse { set_id: result.set_id, index: result.index })))
}

// ── ASC identity registration ───────────────────────────────────────────────

/// `POST /api/v1/register-identity`
///
/// Body (multi-nullifier — ASC protocol):
/// ```json
/// { "commitment": "<66 hex chars>", "nullifiers": ["<64 hex>", ...] }
/// ```
///
/// The commitment is the multi-value Pedersen master identity Φ.
/// The nullifiers are ALL L per-service-provider nullifiers for Sybil resistance.
///
/// Response 200:
/// ```json
/// { "set_id": 0, "index": 3 }
/// ```
///
/// Response 409 if ANY nullifier has already been registered.
#[derive(Deserialize)]
pub struct RegisterIdentityRequest {
    /// Hex-encoded 33-byte master identity commitment (compressed secp256k1 point).
    pub commitment: String,
    /// Hex-encoded 32-byte nullifiers, one per service provider.
    pub nullifiers: Vec<String>,
}

pub async fn register_identity(
    State(state): State<AppState>,
    Json(body): Json<RegisterIdentityRequest>,
) -> Result<(StatusCode, Json<RegisterResponse>), AppError> {
    let commitment =
        Commitment::from_hex(&body.commitment).map_err(|_| AppError::InvalidHex("commitment".into()))?;
    let nullifiers: Vec<Nullifier> = body
        .nullifiers
        .iter()
        .enumerate()
        .map(|(i, hex)| {
            Nullifier::from_hex(hex)
                .map_err(|_| AppError::InvalidHex(format!("nullifier[{i}]")))
        })
        .collect::<Result<_, _>>()?;

    let result = {
        let mut store = state.store.write().await;
        store
            .register_identity(commitment, nullifiers.clone())
            .map_err(|_| AppError::NullifierAlreadyUsed)?
    };

    // Persist to SQLite.
    if result.new_set_opened {
        let cap = state.store.read().await.set_capacity;
        state
            .db
            .persist_new_set(result.set_id, cap)
            .map_err(|e| AppError::Db(e.to_string()))?;
    }
    state
        .db
        .persist_identity_registration(result.set_id, result.index, &commitment, &nullifiers)
        .map_err(|e| AppError::Db(e.to_string()))?;

    Ok((
        StatusCode::OK,
        Json(RegisterResponse {
            set_id: result.set_id,
            index: result.index,
        }),
    ))
}
