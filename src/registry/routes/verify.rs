use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::core::{MembershipProof, Nullifier, verify_membership};

use crate::registry::{error::AppError, server::AppState};

/// `POST /api/v1/verify`
///
/// Body: `{ "nullifier": "<64 hex>", "set_id": 0, "proof": "<1756 hex>" }`
///
/// Response 200: `{ "valid": true }`
#[derive(Deserialize)]
pub struct VerifyRequest {
    /// Hex-encoded 32-byte nullifier.
    pub nullifier: String,
    /// The anonymity set to verify against.
    pub set_id: u64,
    /// Hex-encoded 878-byte serialised `MembershipProof` (flat, field order:
    /// a, b, c, d, g[0..9], f[0..9], z_a, z_c, z).
    pub proof: String,
}

#[derive(Serialize)]
pub struct VerifyResponse {
    pub valid: bool,
}

pub async fn verify(
    State(state): State<AppState>,
    Json(body): Json<VerifyRequest>,
) -> Result<(StatusCode, Json<VerifyResponse>), AppError> {
    let nullifier =
        Nullifier::from_hex(&body.nullifier).map_err(|_| AppError::InvalidHex("nullifier".into()))?;

    let proof_bytes = hex::decode(&body.proof)
        .map_err(|_| AppError::InvalidHex("proof".into()))?;
    if proof_bytes.len() != 878 {
        return Err(AppError::BadRequest("proof must be 878 bytes (1756 hex chars)".into()));
    }

    let proof = deserialize_proof(&proof_bytes)
        .map_err(|e| AppError::BadRequest(e))?;

    let set = {
        let store = state.store.read().await;
        store.get_set(body.set_id)
            .map(|s| s.commitments.clone())
            .ok_or(AppError::NotFound)?
    };

    let valid = verify_membership(&set, &nullifier, &proof);

    Ok((StatusCode::OK, Json(VerifyResponse { valid })))
}

/// Deserialise a flat 878-byte blob into a `MembershipProof`.
///
/// Field layout (byte offsets):
/// ```text
///   0.. 33  a
///  33.. 66  b
///  66.. 99  c
///  99..132  d
/// 132..462  g[0..9]  (10 x 33 bytes)
/// 462..782  f[0..9]  (10 x 32 bytes)
/// 782..814  z_a
/// 814..846  z_c
/// 846..878  z
/// ```
fn deserialize_proof(b: &[u8]) -> Result<MembershipProof, String> {
    if b.len() != 878 {
        return Err(format!("expected 878 bytes, got {}", b.len()));
    }

    let mut g = [[0u8; 33]; 10];
    for (k, slot) in g.iter_mut().enumerate() {
        let start = 132 + k * 33;
        slot.copy_from_slice(&b[start..start + 33]);
    }

    let mut f = [[0u8; 32]; 10];
    for (k, slot) in f.iter_mut().enumerate() {
        let start = 462 + k * 32;
        slot.copy_from_slice(&b[start..start + 32]);
    }

    Ok(MembershipProof {
        a:  b[  0.. 33].try_into().unwrap(),
        b:  b[ 33.. 66].try_into().unwrap(),
        c:  b[ 66.. 99].try_into().unwrap(),
        d:  b[ 99..132].try_into().unwrap(),
        g,
        f,
        z_a: b[782..814].try_into().unwrap(),
        z_c: b[814..846].try_into().unwrap(),
        z:   b[846..878].try_into().unwrap(),
    })
}
