//! Child credential derivation for service registration (Phase 3).
//!
//! From the master credential's child randomness `r`, derives per-service
//! authentication keys and pseudonyms:
//!
//! ```text
//! csk_l = HKDF(r, salt=v_l, info="CRS-ASC-child-secret-key")   ← child auth secret
//! ϕ     = csk_l · g                                              ← pseudonym
//! ```
//!
//! The pseudonym `ϕ` is the user's public identity at service `l`. It is
//! unlinkable across services because `csk_l` is derived with a different
//! salt for each service.

use hkdf::Hkdf;
use k256::{
    elliptic_curve::group::GroupEncoding,
    ProjectivePoint, Scalar,
};
use sha2::Sha256;

use sha2::Digest;

use crate::core::types::{ChildRandomness, Name};
use crate::core::utils::{bytes_to_scalar, point_from_bytes, point_to_bytes, random_scalar};
use bitcoin::secp256k1;
use bitcoin::{Address, Network};

pub struct PaymentRequest {
    pub amount: u64,
    pub pseudonym: [u8; 33],
    pub proof: PaymentRequestProof,
}



pub fn create_payment_request(
    child_randomness: &ChildRandomness,
    name: &Name,
    g: &ProjectivePoint,
    amount: u64,
) -> PaymentRequest {
    let pseudonym = derive_payment_request_pseudonym(child_randomness, name, g);
    let proof = prove_payment_request(child_randomness, name, g);

    PaymentRequest {
        amount,
        pseudonym,
        proof,
    }
}

/// Derive the child secret key for a specific service provider.
///
/// ```text
/// csk_l = HKDF(IKM = r, salt = v_l, info = "CRS-ASC-child-secret-key")
/// ```
///
/// Returns a 32-byte scalar used as the authentication secret for service `l`.
pub fn derive_payment_request_secret_key(child_randomness: &ChildRandomness, name: &Name) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(
        Some(name.as_str().as_bytes()), // salt = v_l
        &child_randomness.0,            // IKM = r
    );
    let mut output = [0u8; 32];
    hk.expand(b"CRS-ASC-child-secret-key", &mut output)
        .expect("32 bytes is valid for HKDF-SHA256");
    output
}

/// Derive the pseudonym `ϕ = csk_l · g` for a specific service provider.
///
/// The pseudonym is the user's public identity at service `l`. It is a
/// 33-byte compressed secp256k1 point using the CRS base generator `g`.
///
/// ```text
/// csk_l = HKDF(r, salt=v_l, info="CRS-ASC-child-secret-key")
/// ϕ     = csk_l · g
/// ```
pub fn derive_payment_request_pseudonym(
    child_randomness: &ChildRandomness,
    name: &Name,
    g: &ProjectivePoint,
) -> [u8; 33] {
    let csk = derive_payment_request_secret_key(child_randomness, name);
    let scalar = bytes_to_scalar(&csk);
    let point = *g * scalar;
    point.to_affine().to_bytes().into()
}

/// Convert a 33-byte compressed pseudonym to a P2TR address.
///
/// The pseudonym `ϕ = csk_l · g` is also a valid secp256k1 point.
pub fn pseudonym_to_address(
    pseudonym: &[u8; 33],
    network: Network,
) -> Result<Address, String> {
    let pk = secp256k1::PublicKey::from_slice(pseudonym)
        .map_err(|e| format!("invalid pseudonym point: {e}"))?;
    let (xonly, _parity) = pk.x_only_public_key();

    let secp = secp256k1::Secp256k1::new();
    Ok(Address::p2tr(&secp, xonly, None, network))
}

// ── Non-interactive Schnorr proof of child credential ownership ─────────────
//
// From the anonymous-credential paper:
//   SP_l sends: W ←$ Z_q         (replaced by Fiat-Shamir)
//   User:       csk_l = HKDF(r, v_l)
//               σ = Schnorr.Sign(csk_l, W)
//   SP_l:       Schnorr.Verify(ϕ, W, σ) = 1
//
// Non-interactive (Fiat-Shamir):
//   1. t ←$ Z_q, R = t · g
//   2. e = H("CRS-ASC-schnorr-child-auth" || g || ϕ || R)
//   3. s = t + e · csk_l
//   Proof = (R, s)
//   Verify: s · g == R + e · ϕ

/// Non-interactive Schnorr proof that the prover knows `csk_l` such that `ϕ = csk_l · g`.
#[derive(Debug, Clone)]
pub struct PaymentRequestProof {
    /// Nonce commitment R = t · g (33-byte compressed point).
    pub r: [u8; 33],
    /// Response s = t + e · csk_l (32-byte scalar).
    pub s: [u8; 32],
}

/// Fiat-Shamir challenge for child credential Schnorr proof.
///
/// `e = H("CRS-ASC-schnorr-child-auth" || g || ϕ || R)`
fn payment_request_challenge(
    g: &ProjectivePoint,
    pseudonym: &[u8; 33],
    r_point: &ProjectivePoint,
) -> Scalar {
    let mut hasher = Sha256::new();
    hasher.update(b"CRS-ASC-schnorr-child-auth");
    hasher.update(point_to_bytes(g));
    hasher.update(pseudonym);
    hasher.update(point_to_bytes(r_point));
    let hash: [u8; 32] = hasher.finalize().into();
    bytes_to_scalar(&hash)
}

/// Generate a non-interactive Schnorr proof of knowledge of `csk_l`.
///
/// Proves that the prover knows the discrete log of `ϕ` with respect to `g`:
/// `ϕ = csk_l · g` where `csk_l = HKDF(r, v_l)`.
pub fn prove_payment_request(
    child_randomness: &ChildRandomness,
    name: &Name,
    g: &ProjectivePoint,
) -> PaymentRequestProof {
    let csk = derive_payment_request_secret_key(child_randomness, name);
    let csk_scalar = bytes_to_scalar(&csk);
    let pseudonym = point_to_bytes(&(*g * csk_scalar));

    let t = random_scalar();
    let r_point = *g * t;
    let e = payment_request_challenge(g, &pseudonym, &r_point);
    let s = t + e * csk_scalar;

    PaymentRequestProof {
        r: point_to_bytes(&r_point),
        s: s.to_bytes().into(),
    }
}

/// Verify a non-interactive Schnorr proof that `ϕ = csk_l · g`.
///
/// Checks: `s · g == R + e · ϕ`
pub fn verify_payment_request(
    g: &ProjectivePoint,
    pseudonym: &[u8; 33],
    proof: &PaymentRequestProof,
) -> bool {
    let r_point = match point_from_bytes(&proof.r) {
        Some(p) => p,
        None => return false,
    };
    let phi = match point_from_bytes(pseudonym) {
        Some(p) => p,
        None => return false,
    };

    let e = payment_request_challenge(g, pseudonym, &r_point);
    let s = bytes_to_scalar(&proof.s);

    let lhs = *g * s;
    let rhs = r_point + phi * e;
    lhs.to_affine() == rhs.to_affine()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::credential::derive_child_randomness;
    use k256::{
        elliptic_curve::hash2curve::{ExpandMsgXmd, GroupDigest},
        Secp256k1,
    };

    fn crs_g() -> ProjectivePoint {
        Secp256k1::hash_from_bytes::<ExpandMsgXmd<sha2::Sha256>>(
            &[b"CRS-ASC-generator-0"],
            &[b"CRS-ASC-v1"],
        )
        .expect("hash_to_curve never fails")
    }

    fn sample_r() -> ChildRandomness {
        let real_rand = [0x42u8; 32];
        derive_child_randomness(&real_rand, &Name::new("alice"))
    }

    #[test]
    fn child_secret_key_deterministic() {
        let r = sample_r();
        let name = Name::new("twitter.com");
        assert_eq!(
            derive_payment_request_secret_key(&r, &name),
            derive_payment_request_secret_key(&r, &name)
        );
    }

    #[test]
    fn different_services_give_different_child_keys() {
        let r = sample_r();
        let csk1 = derive_payment_request_secret_key(&r, &Name::new("twitter.com"));
        let csk2 = derive_payment_request_secret_key(&r, &Name::new("github.com"));
        assert_ne!(csk1, csk2);
    }

    #[test]
    fn different_randomness_gives_different_child_keys() {
        let r1 = ChildRandomness([0x01; 32]);
        let r2 = ChildRandomness([0x02; 32]);
        let name = Name::new("twitter.com");
        assert_ne!(
            derive_payment_request_secret_key(&r1, &name),
            derive_payment_request_secret_key(&r2, &name)
        );
    }

    #[test]
    fn pseudonym_is_33_bytes() {
        let g = crs_g();
        let r = sample_r();
        let pnym = derive_payment_request_pseudonym(&r, &Name::new("twitter.com"), &g);
        assert_eq!(pnym.len(), 33);
        assert!(pnym[0] == 0x02 || pnym[0] == 0x03);
    }

    #[test]
    fn pseudonym_deterministic() {
        let g = crs_g();
        let r = sample_r();
        let name = Name::new("twitter.com");
        assert_eq!(
            derive_payment_request_pseudonym(&r, &name, &g),
            derive_payment_request_pseudonym(&r, &name, &g)
        );
    }

    #[test]
    fn different_services_give_different_pseudonyms() {
        let g = crs_g();
        let r = sample_r();
        let p1 = derive_payment_request_pseudonym(&r, &Name::new("twitter.com"), &g);
        let p2 = derive_payment_request_pseudonym(&r, &Name::new("github.com"), &g);
        assert_ne!(p1, p2);
    }

    #[test]
    fn child_key_independent_from_nullifier() {
        // Child secret key (from r) and nullifier (from sk) must be independent
        use crate::core::nullifier::derive_nullifier;
        use crate::core::types::MasterSecret;

        let r = sample_r();
        let sk = MasterSecret([0x42u8; 32]);
        let name = Name::new("twitter.com");

        let csk = derive_payment_request_secret_key(&r, &name);
        let nul = derive_nullifier(&sk, &name);

        // They should never be equal (different HKDF domains)
        assert_ne!(csk, nul.0);
    }

    // ── Child auth proof tests ──────────────────────────────────────────────

    #[test]
    fn child_auth_proof_verifies() {
        let g = crs_g();
        let r = sample_r();
        let name = Name::new("twitter.com");
        let pseudonym = derive_payment_request_pseudonym(&r, &name, &g);

        let proof = prove_payment_request(&r, &name, &g);
        assert!(verify_payment_request(&g, &pseudonym, &proof));
    }

    #[test]
    fn child_auth_wrong_pseudonym_fails() {
        let g = crs_g();
        let r = sample_r();
        let name = Name::new("twitter.com");
        let wrong_pseudonym = derive_payment_request_pseudonym(&r, &Name::new("github.com"), &g);

        let proof = prove_payment_request(&r, &name, &g);
        assert!(!verify_payment_request(&g, &wrong_pseudonym, &proof));
    }

    #[test]
    fn child_auth_wrong_key_fails() {
        let g = crs_g();
        let r = sample_r();
        let name = Name::new("twitter.com");
        let pseudonym = derive_payment_request_pseudonym(&r, &name, &g);

        // Prove with different child randomness
        let wrong_r = ChildRandomness([0xFF; 32]);
        let proof = prove_payment_request(&wrong_r, &name, &g);
        assert!(!verify_payment_request(&g, &pseudonym, &proof));
    }

    #[test]
    fn child_auth_tampered_response_fails() {
        let g = crs_g();
        let r = sample_r();
        let name = Name::new("twitter.com");
        let pseudonym = derive_payment_request_pseudonym(&r, &name, &g);

        let mut proof = prove_payment_request(&r, &name, &g);
        proof.s[0] ^= 0xFF;
        assert!(!verify_payment_request(&g, &pseudonym, &proof));
    }
}
