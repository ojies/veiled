use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::core::service_proof::deserialize_service_proof;
use crate::core::verifier::VerificationError;
use crate::registry::{error::AppError, server::AppState};

/// `POST /api/v1/verify-registration`
///
/// Body: `{ "pseudonym": "<hex>", "public_nullifier": "<hex>", "proof": "<hex>",
///          "set_id": 0, "friendly_name": "bob" }`
///
/// Phase 4 endpoint: Bob (verifier) receives `(ϕ, nul_l, π, d̂, friendly_name)`
/// from Alice and runs steps 4.1–4.8 including name verification.
#[derive(Deserialize)]
pub struct VerifyRegistrationRequest {
    /// Hex-encoded 33-byte pseudonym `ϕ = csk_l · g`.
    pub pseudonym: String,
    /// Hex-encoded 33-byte public nullifier `nul_l = s_l · g`.
    pub public_nullifier: String,
    /// Hex-encoded serialised `ServiceRegistrationProof`.
    pub proof: String,
    /// Which anonymity set to verify against.
    pub set_id: u64,
    /// The prover's revealed friendly name — verifier checks
    /// `SHA256(friendly_name) == proof.name_scalar`.
    pub friendly_name: String,
}

#[derive(Serialize)]
pub struct VerifyRegistrationResponse {
    pub registered: bool,
    pub pseudonym: String,
    pub public_nullifier: String,
    pub friendly_name: String,
}

pub async fn verify_registration(
    State(state): State<AppState>,
    Json(body): Json<VerifyRegistrationRequest>,
) -> Result<(StatusCode, Json<VerifyRegistrationResponse>), AppError> {
    // Ensure verifier is configured.
    let crs = state.crs.as_ref().ok_or(AppError::VerifierNotConfigured)?;
    let verifier_lock = state
        .verifier_state
        .as_ref()
        .ok_or(AppError::VerifierNotConfigured)?;

    // Decode hex inputs.
    let pseudonym_bytes: [u8; 33] = hex::decode(&body.pseudonym)
        .map_err(|_| AppError::InvalidHex("pseudonym".into()))?
        .try_into()
        .map_err(|_| AppError::BadRequest("pseudonym must be 33 bytes (66 hex chars)".into()))?;

    let pub_nul_bytes: [u8; 33] = hex::decode(&body.public_nullifier)
        .map_err(|_| AppError::InvalidHex("public_nullifier".into()))?
        .try_into()
        .map_err(|_| AppError::BadRequest("public_nullifier must be 33 bytes (66 hex chars)".into()))?;

    let proof_bytes = hex::decode(&body.proof)
        .map_err(|_| AppError::InvalidHex("proof".into()))?;

    let proof = deserialize_service_proof(&proof_bytes)
        .map_err(|e| AppError::BadRequest(e))?;

    // Cache the anonymity set in the verifier if not already cached.
    {
        let vs = verifier_lock.read().await;
        if vs.get_cached_set(body.set_id).is_none() {
            drop(vs);
            // Fetch from registry store and cache.
            let commitments = {
                let store = state.store.read().await;
                store
                    .get_set(body.set_id)
                    .map(|s| s.commitments.clone())
                    .ok_or(AppError::NotFound)?
            };
            let mut vs = verifier_lock.write().await;
            vs.cache_set(body.set_id, commitments);
        }
    }

    // Run steps 4.1–4.8 including name verification.
    let mut vs = verifier_lock.write().await;
    match vs.verify_and_register(
        crs,
        &pseudonym_bytes,
        &pub_nul_bytes,
        &proof,
        body.set_id,
        &body.friendly_name,
    ) {
        Ok(result) => Ok((
            StatusCode::OK,
            Json(VerifyRegistrationResponse {
                registered: true,
                pseudonym: hex::encode(result.pseudonym),
                public_nullifier: hex::encode(result.public_nullifier),
                friendly_name: result.friendly_name,
            }),
        )),
        Err(VerificationError::SetNotFound(id)) => Err(AppError::SetNotFound(id)),
        Err(VerificationError::ProofInvalid) => Err(AppError::ProofVerificationFailed),
        Err(VerificationError::NullifierAlreadyUsed) => Err(AppError::NullifierAlreadyUsed),
        Err(VerificationError::PseudonymAlreadyUsed) => Err(AppError::PseudonymAlreadyUsed),
        Err(VerificationError::NameMismatch) => Err(AppError::ProofVerificationFailed),
    }
}
